/**
 * Input validation for everything that reaches `SessionManager`. Used at both
 * trust boundaries: the Electron IPC adapter (renderer → main) and the daemon
 * server (client → daemon). Pure Node, no Electron.
 */
import { statSync } from 'node:fs'
import { isAbsolute } from 'node:path'
import { LIMITS, type CreateSessionRequest, type SessionKind } from '@shared/session'

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
// eslint-disable-next-line no-control-regex
const CONTROL_CHARS_RE = /[\x00-\x1f\x7f]/

export function str(value: unknown, what: string, maxLength: number): string {
  if (typeof value !== 'string') throw new TypeError(`${what} must be a string`)
  if (value.length > maxLength) throw new RangeError(`${what} is too long (max ${maxLength})`)
  return value
}

export function uuid(value: unknown, what: string): string {
  const s = str(value, what, 36)
  if (!UUID_RE.test(s)) throw new TypeError(`${what} must be a UUID`)
  return s
}

export function int(value: unknown, what: string, min: number, max: number): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < min || value > max) {
    throw new RangeError(`${what} must be an integer in [${min}, ${max}]`)
  }
  return value
}

export function bool(value: unknown, what: string): boolean {
  if (typeof value !== 'boolean') throw new TypeError(`${what} must be a boolean`)
  return value
}

export function label(value: unknown, what: string): string {
  const s = str(value, what, LIMITS.nameLength)
  if (CONTROL_CHARS_RE.test(s)) throw new TypeError(`${what} must not contain control characters`)
  return s
}

export function directory(value: unknown, what: string): string {
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

export function kind(value: unknown): SessionKind {
  if (value !== 'claude' && value !== 'shell' && value !== 'cloud') {
    throw new TypeError('kind must be "claude", "shell" or "cloud"')
  }
  return value
}

const CLOUD_ID_RE = /^(?:session|cse)_[A-Za-z0-9_-]{1,120}$/

/**
 * A cloud session to attach to, as a bare id or a claude.ai/code URL that
 * ends in one. Returns the id: only that reaches the command line.
 */
export function cloudSession(value: unknown, what: string): string {
  const s = str(value, what, 2048).trim()
  if (CLOUD_ID_RE.test(s)) return s
  let url: URL
  try {
    url = new URL(s)
  } catch {
    throw new TypeError(`${what} must be a cloud session id (session_...) or a claude.ai/code URL`)
  }
  const last = url.pathname.split('/').filter(Boolean).pop() ?? ''
  if (url.protocol !== 'https:' || url.hostname !== 'claude.ai' || !CLOUD_ID_RE.test(last)) {
    throw new TypeError(`${what} must be a cloud session id (session_...) or a claude.ai/code URL`)
  }
  return last
}

/** Free text for a cloud task. Newlines are fine; other control characters are not. */
export function cloudTask(value: unknown, what: string): string {
  const s = str(value, what, LIMITS.cloudTaskLength)
  // The CLI would read a leading dash as another option.
  if (s.trimStart().startsWith('-')) throw new TypeError(`${what} must not start with "-"`)
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x09\x0b-\x1f\x7f]/.test(s)) throw new TypeError(`${what} must not contain control characters`)
  return s
}

export function args(value: unknown): string[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new TypeError('claudeArgs must be an array')
  if (value.length > LIMITS.claudeArgsCount) throw new RangeError('too many claudeArgs')
  return value.map((a, i) => {
    const s = str(a, `claudeArgs[${i}]`, LIMITS.claudeArgLength)
    if (CONTROL_CHARS_RE.test(s)) throw new TypeError(`claudeArgs[${i}] must not contain control characters`)
    return s
  })
}

export function createRequest(value: unknown): CreateSessionRequest {
  if (typeof value !== 'object' || value === null) throw new TypeError('request must be an object')
  const v = value as Record<string, unknown>
  const k = kind(v.kind)
  const task = v.cloudTask === undefined ? undefined : cloudTask(v.cloudTask, 'cloudTask').trim() || undefined
  const attach =
    v.attachCloudSession === undefined ? undefined : cloudSession(v.attachCloudSession, 'attachCloudSession')
  if (k === 'cloud' && !task && !attach) throw new TypeError('a cloud session needs a task or a session to attach to')
  if (k !== 'cloud' && (task || attach)) throw new TypeError('cloudTask and attachCloudSession are only for cloud sessions')
  return {
    kind: k,
    cwd: directory(v.cwd, 'cwd'),
    name: v.name === undefined ? undefined : label(v.name, 'name'),
    claudeArgs: args(v.claudeArgs),
    skipPermissions: v.skipPermissions === undefined ? undefined : bool(v.skipPermissions, 'skipPermissions'),
    resumeClaudeSessionId:
      v.resumeClaudeSessionId === undefined ? undefined : uuid(v.resumeClaudeSessionId, 'resumeClaudeSessionId'),
    cloudTask: attach ? undefined : task,
    attachCloudSession: attach
  }
}

export function writeChunk(value: unknown): string {
  return str(value, 'data', LIMITS.writeChunkLength)
}

export function cols(value: unknown): number {
  return int(value, 'cols', 2, LIMITS.maxCols)
}

export function rows(value: unknown): number {
  return int(value, 'rows', 1, LIMITS.maxRows)
}
