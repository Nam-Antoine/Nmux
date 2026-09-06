/**
 * Session model shared by main, preload and renderer.
 * This file must stay free of Node and Electron imports.
 */

export type SessionKind = 'claude' | 'shell'

/**
 * running  - a live PTY process is attached
 * exited   - the process ended (or Nmux restarted). Claude sessions can be
 *            resumed with `claude --resume`, shell sessions can be restarted.
 */
export type SessionStatus = 'running' | 'exited'

export interface SessionMeta {
  /** Nmux's own id. Stable across restarts. */
  id: string
  name: string
  kind: SessionKind
  /** Working directory the process was started in. Sessions are grouped by it in the UI. */
  cwd: string
  /**
   * UUID handed to `claude --session-id`. Nmux owns it, so the conversation
   * can be resumed later with `claude --resume <claudeSessionId>`.
   */
  claudeSessionId?: string
  /** Extra CLI args appended to the claude command (e.g. `--model`, `--add-dir`). */
  claudeArgs: string[]
  /**
   * Launch claude with `--dangerously-skip-permissions` (every tool call is
   * auto-approved). Kept out of `claudeArgs` so the UI can show it and so a
   * resume keeps the same mode. Only meaningful for `kind === 'claude'`.
   */
  skipPermissions?: boolean
  /**
   * One-line message from Nmux about the last start, e.g. why a resume became
   * a fresh conversation. Cleared on the next start.
   */
  notice?: string
  createdAt: number
  lastActiveAt: number
  status: SessionStatus
  exitCode?: number
  pid?: number
}

export interface CreateSessionRequest {
  kind: SessionKind
  cwd: string
  name?: string
  claudeArgs?: string[]
  /** See `SessionMeta.skipPermissions`. */
  skipPermissions?: boolean
  /** Attach to an existing Claude conversation instead of starting a new one. */
  resumeClaudeSessionId?: string
}

/** Screen snapshot returned when the UI attaches to a session. */
export interface AttachResult {
  /** Serialized terminal state (ANSI) to replay into a fresh xterm. */
  snapshot: string
  /** Sequence number of the last output chunk included in the snapshot. */
  seq: number
  cols: number
  rows: number
}

export interface AppInfo {
  version: string
  /** `process.platform` value, e.g. "win32". */
  platform: string
  userDataDir: string
  homeDir: string
  /** Resolved path of the `claude` binary, or null when it is not on PATH. */
  claudeBinary: string | null
}

export const DEFAULT_COLS = 120
export const DEFAULT_ROWS = 30
export const SCROLLBACK_LINES = 5000

/** Input limits enforced at the IPC boundary (main/ipc.ts) and by the manager. */
export const LIMITS = {
  /** Live + exited sessions kept in the list. */
  sessions: 64,
  nameLength: 80,
  pathLength: 4096,
  claudeArgsCount: 32,
  claudeArgLength: 512,
  /** One keyboard/paste chunk. Large pastes are split by xterm anyway. */
  writeChunkLength: 1024 * 1024,
  maxCols: 1000,
  maxRows: 1000
} as const
