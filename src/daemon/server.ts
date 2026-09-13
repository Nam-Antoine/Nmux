/**
 * Exposes a SessionManager over a local pipe. This is the daemon's trust
 * boundary: every connection must present the token before any other
 * message, and every argument is validated before it reaches the manager.
 */
import { timingSafeEqual } from 'node:crypto'
import { existsSync, unlinkSync } from 'node:fs'
import { createServer, connect, type Server, type Socket } from 'node:net'
import { DAEMON_LIMITS, type DaemonInfo, type EventMessage, type RequestMessage } from '@shared/daemon'
import { LineSplitter, encodeFrame } from './framing'
import type { SessionManager } from './sessions/SessionManager'
import { cols, createRequest, label, rows, str, uuid, writeChunk } from './validate'

export interface DaemonServerOptions {
  token: string
  info(): DaemonInfo
  /** Called for `shutdown`: stop every session and exit. */
  onShutdown(): void
  /** Called when no client is connected and no session is running. */
  onIdle(): void
  log(message: string): void
}

const IDLE_GRACE_MS = 3000

interface Client {
  socket: Socket
  name: string
  authed: boolean
}

export class DaemonServer {
  private server: Server | null = null
  private readonly clients = new Set<Client>()
  private idleTimer: NodeJS.Timeout | null = null
  private closing = false

  constructor(
    private readonly manager: SessionManager,
    private readonly opts: DaemonServerOptions
  ) {
    manager.on('data', (id, data, seq) => this.broadcast({ event: 'data', args: [id, data, seq] }))
    manager.on('changed', (sessions) => {
      this.broadcast({ event: 'changed', args: [sessions] })
      this.checkIdle()
    })
  }

  /** Resolves once listening. Rejects with `code === 'EADDRINUSE'` if a daemon already owns the pipe. */
  listen(pipe: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const server = createServer((socket) => this.accept(socket))
      server.on('error', (err: NodeJS.ErrnoException) => {
        if (err.code === 'EADDRINUSE' && process.platform !== 'win32' && existsSync(pipe)) {
          // A stale socket file from a crashed daemon: if nobody answers, take it over.
          const probe = connect(pipe)
          probe.once('connect', () => {
            probe.destroy()
            reject(err)
          })
          probe.once('error', () => {
            try {
              unlinkSync(pipe)
            } catch {
              /* ignore */
            }
            server.removeAllListeners('error')
            server.once('error', reject)
            server.listen(pipe, () => resolve())
          })
          return
        }
        reject(err)
      })
      server.listen(pipe, () => {
        this.server = server
        this.checkIdle()
        resolve()
      })
    })
  }

  get clientCount(): number {
    return this.clients.size
  }

  close(): void {
    this.closing = true
    if (this.idleTimer) clearTimeout(this.idleTimer)
    for (const client of this.clients) client.socket.destroy()
    this.clients.clear()
    this.server?.close()
    this.server = null
  }

  // ---- connections -----------------------------------------------------------

  private accept(socket: Socket): void {
    const client: Client = { socket, name: '?', authed: false }
    this.clients.add(client)
    if (this.idleTimer) {
      clearTimeout(this.idleTimer)
      this.idleTimer = null
    }
    socket.setEncoding('utf8')
    socket.setNoDelay(true)

    const splitter = new LineSplitter(
      DAEMON_LIMITS.maxFrameChars,
      (frame) => this.onFrame(client, frame),
      () => {
        this.opts.log(`client ${client.name}: frame too large, disconnecting`)
        socket.destroy()
      }
    )
    socket.on('data', (chunk: string) => splitter.push(chunk))
    socket.on('error', (err) => this.opts.log(`client ${client.name}: ${err.message}`))
    socket.on('close', () => {
      this.clients.delete(client)
      if (client.authed) this.opts.log(`client ${client.name} disconnected`)
      this.checkIdle()
    })
  }

  private onFrame(client: Client, frame: unknown): void {
    if (typeof frame !== 'object' || frame === null || typeof (frame as RequestMessage).method !== 'string') {
      client.socket.destroy()
      return
    }
    const req = frame as RequestMessage
    const args = Array.isArray(req.args) ? req.args : []
    const id = typeof req.id === 'number' && Number.isInteger(req.id) ? req.id : undefined

    if (!client.authed) {
      if (req.method !== 'hello' || !this.tokenMatches(args[0])) {
        this.opts.log('rejected a connection with a bad token')
        client.socket.destroy()
        return
      }
      client.authed = true
      client.name = typeof args[1] === 'string' ? args[1].slice(0, 32) : 'client'
      this.opts.log(`client ${client.name} connected`)
      this.reply(client, id, { result: this.opts.info() })
      return
    }

    try {
      const result = this.invoke(req.method, args)
      this.reply(client, id, { result })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      if (id === undefined) this.opts.log(`${req.method} dropped: ${message}`)
      this.reply(client, id, { error: message })
    }
  }

  private invoke(method: string, args: unknown[]): unknown {
    const m = this.manager
    switch (method) {
      case 'hello':
      case 'info':
        return this.opts.info()
      case 'list':
        return m.list()
      case 'create':
        return m.create(createRequest(args[0]))
      case 'restart':
        return m.restart(uuid(args[0], 'id'))
      case 'kill':
        return m.kill(uuid(args[0], 'id'))
      case 'remove':
        return m.remove(uuid(args[0], 'id'))
      case 'rename':
        return m.rename(uuid(args[0], 'id'), label(args[1], 'name'))
      case 'attach':
        return m.attach(uuid(args[0], 'id'))
      case 'write':
        return m.write(uuid(args[0], 'id'), writeChunk(args[1]))
      case 'resize':
        return m.resize(uuid(args[0], 'id'), cols(args[1]), rows(args[2]))
      case 'shutdown':
        this.opts.log('shutdown requested')
        // Let the reply flush before the process goes away.
        setImmediate(() => this.opts.onShutdown())
        return undefined
      default:
        throw new Error(`Unknown method: ${str(method, 'method', 64)}`)
    }
  }

  private tokenMatches(value: unknown): boolean {
    if (typeof value !== 'string') return false
    const expected = Buffer.from(this.opts.token)
    const given = Buffer.from(value)
    return expected.length === given.length && timingSafeEqual(expected, given)
  }

  private reply(client: Client, id: number | undefined, body: { result?: unknown; error?: string }): void {
    if (id === undefined) return
    this.send(client, encodeFrame({ id, ...body }))
  }

  private broadcast(event: EventMessage): void {
    if (this.clients.size === 0) return
    const line = encodeFrame(event)
    for (const client of this.clients) if (client.authed) this.send(client, line)
  }

  private send(client: Client, line: string): void {
    if (client.socket.destroyed) return
    client.socket.write(line)
  }

  // ---- idle exit --------------------------------------------------------------

  /** Like tmux: with nothing attached and nothing running there is no reason to stay. */
  private checkIdle(): void {
    if (this.closing) return
    if (this.idleTimer) {
      clearTimeout(this.idleTimer)
      this.idleTimer = null
    }
    if (this.clients.size > 0 || this.manager.runningCount() > 0) return
    this.idleTimer = setTimeout(() => {
      this.idleTimer = null
      if (this.clients.size === 0 && this.manager.runningCount() === 0) this.opts.onIdle()
    }, IDLE_GRACE_MS)
  }
}
