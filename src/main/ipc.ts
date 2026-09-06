/**
 * Adapts SessionManager to Electron IPC. This is the only file that knows
 * both about `ipcMain` and about sessions, and it is the trust boundary:
 * every message is checked for a trusted sender and validated before it
 * reaches the manager.
 */
import { statSync } from 'node:fs'
import { isAbsolute } from 'node:path'
import {
  BrowserWindow,
  app,
  dialog,
  ipcMain,
  type IpcMainEvent,
  type IpcMainInvokeEvent
} from 'electron'
import { IPC } from '@shared/ipc'
import {
  LIMITS,
  type AppInfo,
  type CreateSessionRequest,
  type SessionKind
} from '@shared/session'
import { isTrustedRendererUrl } from './protocol'
import type { SessionManager } from './sessions/SessionManager'
import { findOnPath } from './sessions/launch'

// ---- sender trust -------------------------------------------------------------

/** Only the main frame of our own renderer may talk to the main process. */
function isTrustedSender(event: IpcMainInvokeEvent | IpcMainEvent): boolean {
  const frame = event.senderFrame
  if (!frame || frame !== event.sender.mainFrame) return false
  return isTrustedRendererUrl(frame.url)
}

function handle(channel: string, fn: (event: IpcMainInvokeEvent, ...args: unknown[]) => unknown): void {
  ipcMain.handle(channel, (event, ...args) => {
    if (!isTrustedSender(event)) throw new Error('Rejected IPC call from untrusted sender')
    return fn(event, ...args)
  })
}

function on(channel: string, fn: (...args: unknown[]) => void): void {
  ipcMain.on(channel, (event, ...args) => {
    if (!isTrustedSender(event)) return
    try {
      fn(...args)
    } catch (err) {
      // Hot path: log and drop, never throw back.
      console.warn(`[ipc] ${channel} dropped:`, err instanceof Error ? err.message : err)
    }
  })
}

// ---- validation ----------------------------------------------------------------

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
// eslint-disable-next-line no-control-regex
const CONTROL_CHARS_RE = /[\x00-\x1f\x7f]/

function str(value: unknown, what: string, maxLength: number): string {
  if (typeof value !== 'string') throw new TypeError(`${what} must be a string`)
  if (value.length > maxLength) throw new RangeError(`${what} is too long (max ${maxLength})`)
  return value
}

function uuid(value: unknown, what: string): string {
  const s = str(value, what, 36)
  if (!UUID_RE.test(s)) throw new TypeError(`${what} must be a UUID`)
  return s
}

function int(value: unknown, what: string, min: number, max: number): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < min || value > max) {
    throw new RangeError(`${what} must be an integer in [${min}, ${max}]`)
  }
  return value
}

function bool(value: unknown, what: string): boolean {
  if (typeof value !== 'boolean') throw new TypeError(`${what} must be a boolean`)
  return value
}

function label(value: unknown, what: string): string {
  const s = str(value, what, LIMITS.nameLength)
  if (CONTROL_CHARS_RE.test(s)) throw new TypeError(`${what} must not contain control characters`)
  return s
}

function directory(value: unknown, what: string): string {
  const s = str(value, what, LIMITS.pathLength)
  if (!isAbsolute(s)) throw new TypeError(`${what} must be an absolute path`)
  let isDir = false
  try {
    isDir = statSync(s).isDirectory()
  } catch {
    /* fall through */
  }
  if (!isDir) throw new Error(`${what} is not an existing directory: ${s}`)
  return s
}

function kind(value: unknown): SessionKind {
  if (value !== 'claude' && value !== 'shell') throw new TypeError('kind must be "claude" or "shell"')
  return value
}

function args(value: unknown): string[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new TypeError('claudeArgs must be an array')
  if (value.length > LIMITS.claudeArgsCount) throw new RangeError('too many claudeArgs')
  return value.map((a, i) => {
    const s = str(a, `claudeArgs[${i}]`, LIMITS.claudeArgLength)
    if (CONTROL_CHARS_RE.test(s)) throw new TypeError(`claudeArgs[${i}] must not contain control characters`)
    return s
  })
}

function createRequest(value: unknown): CreateSessionRequest {
  if (typeof value !== 'object' || value === null) throw new TypeError('request must be an object')
  const v = value as Record<string, unknown>
  return {
    kind: kind(v.kind),
    cwd: directory(v.cwd, 'cwd'),
    name: v.name === undefined ? undefined : label(v.name, 'name'),
    claudeArgs: args(v.claudeArgs),
    skipPermissions: v.skipPermissions === undefined ? undefined : bool(v.skipPermissions, 'skipPermissions'),
    resumeClaudeSessionId:
      v.resumeClaudeSessionId === undefined ? undefined : uuid(v.resumeClaudeSessionId, 'resumeClaudeSessionId')
  }
}

// ---- registration --------------------------------------------------------------

export function registerIpc(manager: SessionManager): void {
  handle(IPC.sessionsList, () => manager.list())
  handle(IPC.sessionsCreate, (_e, req) => manager.create(createRequest(req)))
  handle(IPC.sessionsRestart, (_e, id) => manager.restart(uuid(id, 'id')))
  handle(IPC.sessionsKill, (_e, id) => manager.kill(uuid(id, 'id')))
  handle(IPC.sessionsRemove, (_e, id) => manager.remove(uuid(id, 'id')))
  handle(IPC.sessionsRename, (_e, id, name) => manager.rename(uuid(id, 'id'), label(name, 'name')))
  handle(IPC.sessionsAttach, (_e, id) => manager.attach(uuid(id, 'id')))

  on(IPC.sessionsWrite, (id, data) => {
    manager.write(uuid(id, 'id'), str(data, 'data', LIMITS.writeChunkLength))
  })
  on(IPC.sessionsResize, (id, cols, rows) => {
    manager.resize(uuid(id, 'id'), int(cols, 'cols', 2, LIMITS.maxCols), int(rows, 'rows', 1, LIMITS.maxRows))
  })

  handle(IPC.dialogPickFolder, async (event) => {
    const owner = BrowserWindow.fromWebContents(event.sender)
    const options = {
      title: 'Choose a project folder',
      properties: ['openDirectory', 'createDirectory'] as Array<'openDirectory' | 'createDirectory'>
    }
    const result = owner
      ? await dialog.showOpenDialog(owner, options)
      : await dialog.showOpenDialog(options)
    return result.canceled ? null : (result.filePaths[0] ?? null)
  })

  handle(
    IPC.appInfo,
    (): AppInfo => ({
      version: app.getVersion(),
      platform: process.platform,
      userDataDir: app.getPath('userData'),
      homeDir: app.getPath('home'),
      claudeBinary: findOnPath('claude')
    })
  )

  const broadcast = (channel: string, ...payload: unknown[]): void => {
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) win.webContents.send(channel, ...payload)
    }
  }
  manager.on('data', (id, data, seq) => broadcast(IPC.evData, id, data, seq))
  manager.on('changed', (sessions) => broadcast(IPC.evSessionsChanged, sessions))
}
