/**
 * Routes the single `data` push event to the terminal that owns it, so each
 * TerminalPane does not have to filter every chunk from every session.
 */
import { api } from '../api'

type Handler = (data: string, seq: number) => void

const handlers = new Map<string, Handler>()
let started = false

function ensureStarted(): void {
  if (started) return
  started = true
  api.on('data', (sessionId, data, seq) => {
    handlers.get(sessionId)?.(data, seq)
  })
}

export function subscribePty(sessionId: string, handler: Handler): () => void {
  ensureStarted()
  handlers.set(sessionId, handler)
  return () => {
    if (handlers.get(sessionId) === handler) handlers.delete(sessionId)
  }
}
