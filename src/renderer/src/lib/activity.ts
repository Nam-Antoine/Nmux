/**
 * "Is this session doing something right now?" derived from output flow.
 * Claude Code redraws while it works, so recent output means busy; a
 * running session with no output for a few seconds is idle (waiting for
 * input or a long-running command that prints nothing).
 */
import { useEffect, useState } from 'react'
import type { SessionMeta } from '@shared/session'

export type Activity = 'busy' | 'idle' | 'exited'

/** Output younger than this counts as "busy". */
export const BUSY_MS = 4000

const lastOutputAt = new Map<string, number>()

/** Called by the PTY stream for every chunk. */
export function noteOutput(sessionId: string): void {
  lastOutputAt.set(sessionId, Date.now())
}

export function lastOutput(sessionId: string): number | undefined {
  return lastOutputAt.get(sessionId)
}

export function activityOf(session: SessionMeta, now = Date.now()): Activity {
  if (session.status !== 'running') return 'exited'
  const at = lastOutputAt.get(session.id)
  return at !== undefined && now - at < BUSY_MS ? 'busy' : 'idle'
}

/** Re-renders the caller every `intervalMs` so activity states age. */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs)
    return () => clearInterval(timer)
  }, [intervalMs])
  return now
}

export function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000))
  if (s < 5) return 'just now'
  if (s < 60) return `${s}s ago`
  const m = Math.round(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.round(m / 60)
  if (h < 48) return `${h}h ago`
  return `${Math.round(h / 24)}d ago`
}

export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s % 60}s`
  return `${s}s`
}
