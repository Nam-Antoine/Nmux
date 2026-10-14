import type { SessionMeta } from '@shared/session'

/** Short label shown next to a session. */
export function kindLabel(kind: SessionMeta['kind']): string {
  return kind === 'cloud' ? 'cloud' : kind === 'claude' ? 'claude' : 'shell'
}

/** What the restart action does for an exited session, long and short form. */
export function restartLabel(session: SessionMeta): { long: string; short: string } {
  if (session.kind === 'claude') return { long: 'Resume conversation', short: 'Resume' }
  if (session.kind === 'cloud') {
    return session.cloudSessionId
      ? { long: 'Reattach to cloud session', short: 'Reattach' }
      : { long: 'Start cloud session again', short: 'Retry' }
  }
  return { long: 'Restart shell', short: 'Restart' }
}
