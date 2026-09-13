/**
 * Routes the single `data` push event to the terminal that owns it, so each
 * TerminalPane does not have to filter every chunk from every session, and
 * records output activity for the status UI.
 */
import { api } from '../api'
import { noteOutput } from './activity'

type Handler = (data: string, seq: number) => void

const handlers = new Map<string, Handler>()
let started = false

/** Idempotent. The app calls it once so activity is tracked even for sessions without a pane. */
export function startPtyStream(): void {
  if (started) return
  started = true
  api.on('data', (sessionId, data, seq) => {
    noteOutput(sessionId)
    handlers.get(sessionId)?.(data, seq)
  })
}

export function subscribePty(sessionId: string, handler: Handler): () => void {
  startPtyStream()
  handlers.set(sessionId, handler)
  return () => {
    if (handlers.get(sessionId) === handler) handlers.delete(sessionId)
  }
}
