/**
 * The app's side of the daemon protocol: finds the running daemon through
 * `daemon.json`, starts one when there is none, authenticates, and keeps
 * reconnecting for as long as the app lives. Method calls mirror
 * `SessionManager`; events mirror its `data` / `changed` events.
 */
import { spawn } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { closeSync, existsSync, openSync, readFileSync, statSync, truncateSync } from 'node:fs'
import { connect, type Socket } from 'node:net'
import {
  DAEMON_LIMITS,
  DAEMON_PROTOCOL_VERSION,
  type DaemonEvents,
  type DaemonFile,
  type DaemonInfo,
  type DaemonMethod,
  type DaemonMethods,
  type EventMessage,
  type ResponseMessage
} from '@shared/daemon'
import { daemonFilePath, daemonLogPath } from '../../daemon/endpoint'
import { LineSplitter, encodeFrame } from '../../daemon/framing'

export interface DaemonClientOptions {
  userDataDir: string
  /** Script to run with ELECTRON_RUN_AS_NODE=1, i.e. `out/main/daemon.js`. */
  script: string
  /** Binary that runs the script: `process.execPath` (Electron or Nmux.exe). */
  execPath: string
  appVersion: string
  log?: (message: string) => void
}

export interface DaemonClientEvents extends DaemonEvents {
  /** Authenticated with a daemon (first time or after a reconnect). */
  connected: (info: DaemonInfo) => void
  /** The connection dropped; a reconnect is already scheduled. */
  disconnected: () => void
}

interface Pending {
  resolve(value: unknown): void
  reject(error: Error): void
  timer: NodeJS.Timeout
}

const MAX_LOG_BYTES = 5 * 1024 * 1024
const RECONNECT_MIN_MS = 500
const RECONNECT_MAX_MS = 5000
const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))

export class DaemonClient extends EventEmitter {
  private socket: Socket | null = null
  private nextId = 1
  private readonly pending = new Map<number, Pending>()
  private closed = false
  private expectClose = false
  private reconnectTimer: NodeJS.Timeout | null = null
  private reconnectDelay = RECONNECT_MIN_MS
  /** What the current daemon said in `hello`; null while disconnected. */
  info: DaemonInfo | null = null

  constructor(private readonly opts: DaemonClientOptions) {
    super()
  }

  override on<E extends keyof DaemonClientEvents>(event: E, listener: DaemonClientEvents[E]): this {
    return super.on(event, listener)
  }

  get connected(): boolean {
    return this.socket !== null && this.info !== null
  }

  // ---- lifecycle ----------------------------------------------------------------

  /**
   * Connect to the running daemon or start one. A daemon from an older Nmux
   * is replaced when it has nothing running; otherwise it is kept so an
   * upgrade never kills live sessions.
   */
  async start(): Promise<DaemonInfo> {
    let info = await this.connectOrSpawn()
    if (info.version !== this.opts.appVersion || info.protocolVersion !== DAEMON_PROTOCOL_VERSION) {
      if (info.runningCount === 0) {
        this.log(`daemon ${info.version} is not ${this.opts.appVersion}, replacing it`)
        await this.stopDaemon()
        info = await this.connectOrSpawn()
      } else {
        this.log(`daemon ${info.version} has ${info.runningCount} running session(s); keeping it`)
      }
    }
    return info
  }

  /** Ask the daemon to stop every session and exit, and wait until it is gone. */
  async stopDaemon(): Promise<void> {
    if (!this.socket) return
    const socket = this.socket
    this.expectClose = true
    this.notify('shutdown')
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        socket.destroy()
        resolve()
      }, 5000)
      socket.once('close', () => {
        clearTimeout(timer)
        resolve()
      })
    })
  }

  /** Drop the connection for good (app quit). The daemon keeps running. */
  close(): void {
    this.closed = true
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.reconnectTimer = null
    this.socket?.destroy()
  }

  // ---- calls ------------------------------------------------------------------------

  call<M extends DaemonMethod>(
    method: M,
    ...args: Parameters<DaemonMethods[M]>
  ): Promise<ReturnType<DaemonMethods[M]>> {
    return new Promise((resolve, reject) => {
      if (!this.socket || this.socket.destroyed) {
        reject(new Error('Nmux is not connected to its session daemon'))
        return
      }
      const id = this.nextId++
      const timer = setTimeout(() => {
        this.pending.delete(id)
        reject(new Error(`daemon did not answer ${method} in time`))
      }, DAEMON_LIMITS.requestTimeoutMs)
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, timer })
      this.socket.write(encodeFrame({ id, method, args }))
    })
  }

  /** Fire-and-forget (hot path: keystrokes, resizes). Silently dropped while disconnected. */
  notify<M extends DaemonMethod>(method: M, ...args: Parameters<DaemonMethods[M]>): void {
    if (!this.socket || this.socket.destroyed) return
    this.socket.write(encodeFrame({ method, args }))
  }

  // ---- connection management --------------------------------------------------------

  private async connectOrSpawn(): Promise<DaemonInfo> {
    try {
      return await this.connectExisting()
    } catch (err) {
      this.log(`no daemon to attach to (${err instanceof Error ? err.message : err}); starting one`)
    }
    const pid = this.spawnDaemon()
    const deadline = Date.now() + DAEMON_LIMITS.spawnTimeoutMs
    let lastError = 'not ready'
    while (Date.now() < deadline) {
      await sleep(150)
      try {
        const file = readDaemonFile(this.opts.userDataDir)
        // Ignore a stale file from a previous daemon.
        if (!file || file.pid !== pid) continue
        return await this.connectExisting()
      } catch (err) {
        lastError = err instanceof Error ? err.message : String(err)
      }
    }
    throw new Error(`session daemon did not start within ${DAEMON_LIMITS.spawnTimeoutMs} ms (${lastError})`)
  }

  /** Connect to the daemon named in `daemon.json` and authenticate. */
  private connectExisting(): Promise<DaemonInfo> {
    const file = readDaemonFile(this.opts.userDataDir)
    if (!file) return Promise.reject(new Error('daemon.json not found'))
    return new Promise((resolve, reject) => {
      let settled = false
      const socket = connect(file.pipe)
      socket.setEncoding('utf8')
      socket.setNoDelay(true)

      const splitter = new LineSplitter(
        DAEMON_LIMITS.maxFrameChars,
        (frame) => this.onFrame(frame),
        () => {
          this.log('daemon sent an oversized frame, reconnecting')
          socket.destroy()
        }
      )
      socket.on('data', (chunk: string) => splitter.push(chunk))
      socket.on('error', (err) => {
        if (!settled) {
          settled = true
          reject(err)
        } else {
          this.log(`connection error: ${err.message}`)
        }
      })
      socket.on('connect', () => {
        this.socket = socket
        this.call('hello', file.token, `nmux-app ${this.opts.appVersion}`)
          .then((info) => {
            settled = true
            this.info = info
            this.reconnectDelay = RECONNECT_MIN_MS
            this.log(`attached to daemon ${info.version} (pid ${info.pid}, ${info.runningCount} running)`)
            this.emit('connected', info)
            resolve(info)
          })
          .catch((err: Error) => {
            settled = true
            socket.destroy()
            reject(err)
          })
      })
      socket.on('close', () => {
        if (this.socket === socket) this.onDisconnected()
        if (!settled) {
          settled = true
          reject(new Error('connection closed before hello completed'))
        }
      })
    })
  }

  private onDisconnected(): void {
    this.socket = null
    const hadInfo = this.info !== null
    this.info = null
    for (const [id, p] of this.pending) {
      clearTimeout(p.timer)
      p.reject(new Error('lost the connection to the session daemon'))
      this.pending.delete(id)
    }
    if (hadInfo) {
      this.log(this.expectClose ? 'daemon stopped' : 'connection to the daemon was lost')
      this.emit('disconnected')
    }
    if (this.closed || this.expectClose) {
      this.expectClose = false
      return
    }
    this.scheduleReconnect()
  }

  private scheduleReconnect(): void {
    if (this.closed || this.reconnectTimer) return
    const delay = this.reconnectDelay
    this.reconnectDelay = Math.min(this.reconnectDelay * 2, RECONNECT_MAX_MS)
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      if (this.closed) return
      this.connectOrSpawn().catch((err: Error) => {
        this.log(`reconnect failed: ${err.message}`)
        this.scheduleReconnect()
      })
    }, delay)
  }

  private onFrame(frame: unknown): void {
    if (typeof frame !== 'object' || frame === null) return
    const msg = frame as Partial<ResponseMessage & EventMessage>
    if (typeof msg.id === 'number') {
      const p = this.pending.get(msg.id)
      if (!p) return
      this.pending.delete(msg.id)
      clearTimeout(p.timer)
      if (msg.error !== undefined) p.reject(new Error(msg.error))
      else p.resolve(msg.result)
      return
    }
    if (msg.event === 'data' || msg.event === 'changed') {
      const args = Array.isArray(msg.args) ? msg.args : []
      this.emit(msg.event, ...args)
    }
  }

  // ---- spawning ---------------------------------------------------------------------

  private spawnDaemon(): number {
    const logPath = daemonLogPath(this.opts.userDataDir)
    try {
      if (existsSync(logPath) && statSync(logPath).size > MAX_LOG_BYTES) truncateSync(logPath, 0)
    } catch {
      /* ignore */
    }
    const fd = openSync(logPath, 'a')
    try {
      // Note: on Windows the child also inherits every inheritable handle of
      // this process (libuv limitation). Only the dev DevTools socket is
      // affected in practice; see ARCHITECTURE.md "Known limitations".
      const child = spawn(
        this.opts.execPath,
        [this.opts.script, '--user-data', this.opts.userDataDir, '--app-version', this.opts.appVersion],
        {
          env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' },
          cwd: this.opts.userDataDir,
          detached: true,
          stdio: ['ignore', fd, fd],
          windowsHide: true
        }
      )
      child.on('error', (err) => this.log(`failed to spawn the daemon: ${err.message}`))
      child.unref()
      this.log(`spawned daemon pid ${child.pid} (${this.opts.script})`)
      return child.pid ?? -1
    } finally {
      closeSync(fd)
    }
  }

  private log(message: string): void {
    this.opts.log?.(message)
  }
}

function readDaemonFile(userDataDir: string): DaemonFile | null {
  const path = daemonFilePath(userDataDir)
  if (!existsSync(path)) return null
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8')) as Partial<DaemonFile>
    if (parsed.version !== 1 || typeof parsed.pipe !== 'string' || typeof parsed.token !== 'string') return null
    return parsed as DaemonFile
  } catch {
    return null
  }
}
