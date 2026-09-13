/**
 * Wire contract between the Electron app (client) and the session daemon.
 *
 * Transport: newline-delimited JSON over a named pipe (Windows) or a Unix
 * socket. The first message on a connection must be `hello` with the token
 * from `<userData>/daemon.json`; anything else closes the connection.
 *
 * This file must stay free of Node and Electron imports.
 */
import type { AttachResult, CreateSessionRequest, SessionMeta } from './session'

export const DAEMON_PROTOCOL_VERSION = 1

/** What the daemon says about itself in reply to `hello` and `info`. */
export interface DaemonInfo {
  protocolVersion: number
  /** Nmux version the daemon was started from. */
  version: string
  pid: number
  startedAt: number
  runningCount: number
  /** Resolved path of the `claude` binary, or null when it is not on PATH. */
  claudeBinary: string | null
}

/** Contents of `<userData>/daemon.json`, written by the daemon once it listens. */
export interface DaemonFile {
  version: 1
  pid: number
  /** Named pipe (`\\.\pipe\...`) or Unix socket path to connect to. */
  pipe: string
  /** Random secret a client must present in `hello`. New on every daemon start. */
  token: string
  appVersion: string
  startedAt: number
}

/** Every request a client can make. Mirrors `SessionManager` plus lifecycle. */
export interface DaemonMethods {
  hello(token: string, client: string): DaemonInfo
  info(): DaemonInfo
  list(): SessionMeta[]
  create(req: CreateSessionRequest): SessionMeta
  restart(id: string): SessionMeta
  kill(id: string): void
  remove(id: string): void
  rename(id: string, name: string): void
  attach(id: string): AttachResult
  write(id: string, data: string): void
  resize(id: string, cols: number, rows: number): void
  /** Stop every session, then exit. */
  shutdown(): void
}

export type DaemonMethod = keyof DaemonMethods

/** Push events from the daemon to every authenticated client. */
export interface DaemonEvents {
  data: (sessionId: string, data: string, seq: number) => void
  changed: (sessions: SessionMeta[]) => void
}

/** Client → daemon. Without `id` the call is fire-and-forget (no reply). */
export interface RequestMessage {
  id?: number
  method: string
  args?: unknown[]
}

/** Daemon → client, reply to a request that carried an `id`. */
export interface ResponseMessage {
  id: number
  result?: unknown
  error?: string
}

/** Daemon → client, unsolicited. */
export interface EventMessage {
  event: keyof DaemonEvents
  args: unknown[]
}

export const DAEMON_LIMITS = {
  /** Longest single line either side will buffer before dropping the connection. */
  maxFrameChars: 8 * 1024 * 1024,
  /** Milliseconds a client waits for a reply before failing the call. */
  requestTimeoutMs: 30_000,
  /** Milliseconds the app waits for a freshly spawned daemon to accept connections. */
  spawnTimeoutMs: 15_000
} as const
