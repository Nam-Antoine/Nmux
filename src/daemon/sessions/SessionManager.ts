/**
 * Owns every session: metadata + (optionally) a live PtySession.
 *
 * Transport-agnostic on purpose: it emits events and exposes plain methods.
 * `src/daemon/server.ts` exposes it over the daemon pipe; the Electron app
 * talks to it through `src/main/daemon/client.ts`.
 */
import { EventEmitter } from 'node:events'
import { randomUUID } from 'node:crypto'
import { basename } from 'node:path'
import {
  DEFAULT_COLS,
  DEFAULT_ROWS,
  LIMITS,
  SCROLLBACK_LINES,
  type AttachResult,
  type CreateSessionRequest,
  type SessionMeta
} from '@shared/session'
import type { SessionStore } from '../store/SessionStore'
import { PtySession } from './PtySession'
import { claudeLaunch, cloudLaunch, sessionEnv, shellLaunch, type LaunchSpec } from './launch'
import { CloudIdSniffer } from './cloudId'
import { transcriptState } from './claudeProjects'

export interface SessionManagerEvents {
  data: (sessionId: string, data: string, seq: number) => void
  changed: (sessions: SessionMeta[]) => void
}

interface Entry {
  meta: SessionMeta
  pty?: PtySession
  /** Screen at the moment the process exited, so attach() can still show it. */
  lastScreen?: AttachResult
  /** Cloud sessions whose id is not known yet: watches output for it. */
  sniffer?: CloudIdSniffer
}

export class SessionManager extends EventEmitter {
  private readonly entries = new Map<string, Entry>()
  private saveTimer: NodeJS.Timeout | null = null

  constructor(private readonly store: SessionStore) {
    super()
    // A process cannot outlive the daemon that spawned it, so after a daemon
    // start every persisted session is `exited` and can be resumed/restarted.
    for (const meta of store.load()) {
      this.entries.set(meta.id, {
        meta: { ...meta, status: 'exited', pid: undefined }
      })
    }
  }

  override on<E extends keyof SessionManagerEvents>(event: E, listener: SessionManagerEvents[E]): this {
    return super.on(event, listener)
  }

  // ---- queries -------------------------------------------------------------

  list(): SessionMeta[] {
    return [...this.entries.values()]
      .map((e) => e.meta)
      .sort((a, b) => b.lastActiveAt - a.lastActiveAt)
  }

  runningCount(): number {
    return [...this.entries.values()].filter((e) => e.meta.status === 'running').length
  }

  // ---- lifecycle -----------------------------------------------------------

  create(req: CreateSessionRequest): SessionMeta {
    if (this.entries.size >= LIMITS.sessions) {
      throw new Error(`Session limit reached (${LIMITS.sessions}). Remove old sessions first.`)
    }
    const now = Date.now()
    const id = randomUUID()
    const claudeSessionId =
      req.kind === 'claude' ? (req.resumeClaudeSessionId ?? randomUUID()) : undefined

    const meta: SessionMeta = {
      id,
      name: req.name?.trim() || defaultName(req),
      kind: req.kind,
      cwd: req.cwd,
      claudeSessionId,
      claudeArgs: req.claudeArgs ?? [],
      skipPermissions: req.kind === 'claude' && req.skipPermissions ? true : undefined,
      cloudTask: req.kind === 'cloud' ? req.cloudTask : undefined,
      cloudSessionId: req.kind === 'cloud' ? req.attachCloudSession : undefined,
      createdAt: now,
      lastActiveAt: now,
      status: 'exited'
    }
    const entry: Entry = { meta }
    this.entries.set(id, entry)
    this.spawn(entry, { resume: Boolean(req.resumeClaudeSessionId) })
    return entry.meta
  }

  /**
   * Resume a Claude conversation or respawn a shell in its cwd.
   *
   * `claude --resume` exits with "No conversation found" when the transcript
   * has no messages (the session was never used, or only holds a Remote
   * Control bridge record), so Nmux starts a fresh conversation in that case
   * instead of dying with a blank screen.
   */
  restart(id: string): SessionMeta {
    const entry = this.mustGet(id)
    const { meta } = entry
    if (meta.status === 'running') return meta
    meta.notice = undefined
    let resume = true
    if (meta.kind === 'claude' && meta.claudeSessionId) {
      const state = transcriptState(meta.cwd, meta.claudeSessionId)
      if (state === 'missing' || state === 'empty') {
        resume = false
        // Claude refuses `--session-id` for an id that already has a file,
        // even one without messages, so the empty case needs a new id.
        if (state === 'empty') meta.claudeSessionId = randomUUID()
        meta.notice = 'Nothing to resume: the previous conversation had no messages, so this is a new one.'
      }
    }
    if (meta.kind === 'cloud' && !meta.cloudSessionId) {
      // The first start never reported a session (e.g. the CLI failed before
      // creating one), so the only thing to do is ask again.
      meta.notice = 'No cloud session id was seen last time, so this asks for a new cloud session with the same task.'
    }
    this.spawn(entry, { resume })
    return meta
  }

  kill(id: string): void {
    const entry = this.mustGet(id)
    entry.pty?.kill()
  }

  remove(id: string): void {
    const entry = this.mustGet(id)
    entry.pty?.dispose()
    this.entries.delete(id)
    this.notifyChanged()
  }

  rename(id: string, name: string): void {
    const entry = this.mustGet(id)
    const trimmed = name.trim()
    if (!trimmed) return
    entry.meta.name = trimmed
    this.notifyChanged()
  }

  // ---- terminal I/O --------------------------------------------------------

  attach(id: string): AttachResult {
    const entry = this.mustGet(id)
    if (!entry.pty) {
      return entry.lastScreen ?? { snapshot: '', seq: 0, cols: DEFAULT_COLS, rows: DEFAULT_ROWS }
    }
    return entry.pty.snapshot()
  }

  write(id: string, data: string): void {
    const entry = this.entries.get(id)
    if (!entry?.pty) return
    entry.pty.write(data)
    entry.meta.lastActiveAt = Date.now()
    this.scheduleSave()
  }

  resize(id: string, cols: number, rows: number): void {
    this.entries.get(id)?.pty?.resize(cols, rows)
  }

  /** Kill everything. Called when the daemon is told to stop. */
  dispose(): void {
    for (const entry of this.entries.values()) {
      entry.pty?.dispose()
      entry.pty = undefined
      if (entry.meta.status === 'running') entry.meta.status = 'exited'
    }
    this.flushSave()
  }

  // ---- internals -----------------------------------------------------------

  private spawn(entry: Entry, opts: { resume: boolean }): void {
    const { meta } = entry
    const launch: LaunchSpec =
      meta.kind === 'claude'
        ? claudeLaunch({
            claudeSessionId: meta.claudeSessionId!,
            resume: opts.resume,
            extraArgs: meta.claudeArgs,
            skipPermissions: meta.skipPermissions
          })
        : meta.kind === 'cloud'
          ? cloudLaunch({ target: meta.cloudSessionId ?? meta.cloudTask ?? '', extraArgs: meta.claudeArgs })
          : shellLaunch()
    entry.sniffer =
      meta.kind === 'cloud' && (!meta.cloudSessionId || !meta.cloudUrl) ? new CloudIdSniffer() : undefined

    const pty = new PtySession({
      launch,
      cwd: meta.cwd,
      env: sessionEnv(meta.id),
      cols: DEFAULT_COLS,
      rows: DEFAULT_ROWS,
      scrollback: SCROLLBACK_LINES
    })

    pty.on('data', (data, seq) => {
      this.emit('data', meta.id, data, seq)
      const found = entry.sniffer?.push(data)
      if (found && (!meta.cloudSessionId || (!meta.cloudUrl && found.url))) {
        meta.cloudSessionId ??= found.id
        meta.cloudUrl ??= found.url
        // Keep watching while the link is missing: it may be printed later.
        if (meta.cloudUrl) entry.sniffer = undefined
        this.notifyChanged()
      }
    })
    pty.on('exit', (exitCode) => {
      meta.status = 'exited'
      meta.exitCode = exitCode
      meta.pid = undefined
      meta.lastActiveAt = Date.now()
      entry.pty = undefined
      // Keep what was on screen (e.g. the error that ended the process).
      entry.lastScreen = pty.snapshot()
      pty.dispose()
      this.notifyChanged()
    })

    entry.pty = pty
    entry.lastScreen = undefined
    if (meta.notice) pty.note(meta.notice)
    meta.status = 'running'
    meta.pid = pty.pid
    meta.exitCode = undefined
    meta.lastActiveAt = Date.now()
    this.notifyChanged()
  }

  private mustGet(id: string): Entry {
    const entry = this.entries.get(id)
    if (!entry) throw new Error(`Unknown session: ${id}`)
    return entry
  }

  private notifyChanged(): void {
    this.emit('changed', this.list())
    this.scheduleSave()
  }

  private scheduleSave(): void {
    if (this.saveTimer) return
    this.saveTimer = setTimeout(() => this.flushSave(), 500)
  }

  /** Write the session list now instead of after the debounce. */
  flushSave(): void {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer)
      this.saveTimer = null
    }
    try {
      this.store.save(this.list())
    } catch (err) {
      console.error('[SessionManager] save failed:', err)
    }
  }
}

function defaultName(req: CreateSessionRequest): string {
  const folder = basename(req.cwd) || req.cwd
  if (req.kind === 'shell') return `${folder} (shell)`
  if (req.kind === 'cloud') {
    const task = req.cloudTask?.replace(/\s+/g, ' ').trim()
    return task ? `${task.slice(0, 48)}${task.length > 48 ? '…' : ''}` : `${folder} (cloud)`
  }
  return folder
}
