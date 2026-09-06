/**
 * Groups sessions by their project folder for the sidebar.
 * Pure functions, no React.
 */
import type { SessionMeta } from '@shared/session'

export interface FolderGroup {
  /** Normalised path, used as the React key and for collapse state. */
  key: string
  /** Path as the first session in the group stored it (display / prefill). */
  path: string
  /** Last path segment. */
  name: string
  /** Oldest first, so rows do not reorder while you work. */
  sessions: SessionMeta[]
  running: number
}

const WINDOWS_PATH_RE = /^([a-zA-Z]:|\\\\)/

/** Trailing separators stripped; Windows paths are case-insensitive. */
export function folderKey(cwd: string): string {
  let p = cwd.trim().replace(/[\\/]+$/, '')
  if (WINDOWS_PATH_RE.test(p)) p = p.replace(/\//g, '\\').toLowerCase()
  return p || cwd
}

export function folderName(cwd: string): string {
  const parts = cwd.replace(/[\\/]+$/, '').split(/[\\/]/)
  return parts[parts.length - 1] || cwd
}

/**
 * Folders come out in the order their most recently active session appears in
 * `sessions` (main sorts by lastActiveAt, newest first). Inside a folder the
 * order is by creation time so rows stay put.
 */
export function groupByFolder(sessions: SessionMeta[]): FolderGroup[] {
  const groups = new Map<string, FolderGroup>()
  for (const session of sessions) {
    const key = folderKey(session.cwd)
    let group = groups.get(key)
    if (!group) {
      group = { key, path: session.cwd, name: folderName(session.cwd), sessions: [], running: 0 }
      groups.set(key, group)
    }
    group.sessions.push(session)
    if (session.status === 'running') group.running++
  }
  for (const group of groups.values()) {
    group.sessions.sort((a, b) => a.createdAt - b.createdAt)
  }
  return [...groups.values()]
}

/** Every session in sidebar order, for keyboard navigation. */
export function visualOrder(sessions: SessionMeta[]): SessionMeta[] {
  return groupByFolder(sessions).flatMap((g) => g.sessions)
}
