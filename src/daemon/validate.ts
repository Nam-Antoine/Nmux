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
  if (value !== 'claude' && value !== 'shell') throw new TypeError('kind must be "claude" or "shell"')
  return value
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

export function writeChunk(value: unknown): string {
  return str(value, 'data', LIMITS.writeChunkLength)
}

export function cols(value: unknown): number {
  return int(value, 'cols', 2, LIMITS.maxCols)
}

export function rows(value: unknown): number {
  return int(value, 'rows', 1, LIMITS.maxRows)
}
