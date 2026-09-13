/**
 * Dashboard of every session: which ones are working right now, which are
 * idle, which have exited. Cards are grouped by folder; clicking one opens
 * it in the focused pane.
 */
import { useEffect, useMemo } from 'react'
import type { SessionMeta } from '@shared/session'
import { activityOf, ago, duration, lastOutput, useNow, type Activity } from '../lib/activity'
import { groupByFolder } from '../lib/folders'
import { useSessions } from '../store/sessions'

const LABEL: Record<Activity, string> = { busy: 'working', idle: 'idle', exited: 'exited' }

export function Overview(): React.JSX.Element {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const closeOverview = useSessions((s) => s.closeOverview)
  const openDialog = useSessions((s) => s.openDialog)
  const now = useNow(1000)

  const groups = useMemo(() => groupByFolder(sessions), [sessions])
  const counts = useMemo(() => {
    const c = { busy: 0, idle: 0, exited: 0 }
    for (const s of sessions) c[activityOf(s, now)]++
    return c
  }, [sessions, now])

  useEffect(() => {
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key === 'Escape') {
        ev.preventDefault()
        closeOverview()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [closeOverview])

  return (
    <div className="overview" role="dialog" aria-label="Session overview">
      <header className="overview-header">
        <h2>Sessions</h2>
        <span className={`pill busy${counts.busy ? ' on' : ''}`} title="Producing output right now">
          <span className="status-dot busy" /> {counts.busy} working
        </span>
        <span className={`pill idle${counts.idle ? ' on' : ''}`} title="Running, waiting for input">
          <span className="status-dot running" /> {counts.idle} idle
        </span>
        <span className="pill exited" title="Process ended; can be resumed or restarted">
          <span className="status-dot exited" /> {counts.exited} exited
        </span>
        <span className="grow" />
        <button className="btn primary small" onClick={() => openDialog()} title="New session (Ctrl+Shift+N)">
          + New
        </button>
        <button className="icon-btn" onClick={closeOverview} title="Close (Esc)" aria-label="Close overview">
          ×
        </button>
      </header>

      <div className="overview-body">
        {groups.length === 0 && (
          <div className="overview-empty muted">
            <p>No sessions yet.</p>
            <button className="btn primary" onClick={() => openDialog()}>
              Start one
            </button>
          </div>
        )}
        {groups.map((group) => (
          <section key={group.key} className="overview-folder">
            <h3 title={group.path}>
              {group.name}
              <span className="muted"> · {group.running}/{group.sessions.length} running</span>
            </h3>
            <div className="card-grid">
              {group.sessions.map((session) => (
                <SessionCard key={session.id} session={session} active={session.id === activeId} now={now} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  )
}

function SessionCard({ session, active, now }: { session: SessionMeta; active: boolean; now: number }): React.JSX.Element {
  const select = useSessions((s) => s.select)
  const closeOverview = useSessions((s) => s.closeOverview)
  const restart = useSessions((s) => s.restart)
  const kill = useSessions((s) => s.kill)
  const remove = useSessions((s) => s.remove)

  const activity = activityOf(session, now)
  const running = session.status === 'running'
  const last = lastOutput(session.id)

  const statusLine = (): string => {
    if (activity === 'busy') return 'working · output just now'
    if (activity === 'idle') return last ? `idle · last output ${ago(now - last)}` : 'idle · no output yet'
    const code = session.exitCode
    return `exited${code !== undefined ? ` · code ${code}` : ''} · ${ago(now - session.lastActiveAt)}`
  }

  const open = (): void => {
    select(session.id)
    closeOverview()
  }

  return (
    <article
      className={`session-card ${activity}${active ? ' active' : ''}`}
      data-session={session.id}
      onClick={open}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter') open()
      }}
    >
      <div className="card-title">
        <span className={`status-dot ${activity === 'busy' ? 'busy' : running ? 'running' : 'exited'}`} />
        <span className="ellipsis">{session.name}</span>
        <span className={`tag${running ? ' ok' : ''}`}>{LABEL[activity]}</span>
      </div>
      <div className={`card-status ${activity}`}>{statusLine()}</div>
      <div className="card-meta">
        <span>{session.kind === 'claude' ? 'claude' : 'shell'}</span>
        {session.skipPermissions && <span className="tag warn">skips permissions</span>}
        {running && session.pid !== undefined && <span>pid {session.pid}</span>}
        {running && <span title="Since this process started">up {duration(now - session.lastActiveAt)}</span>}
        {session.claudeSessionId && <span title={session.claudeSessionId}>id {session.claudeSessionId.slice(0, 8)}…</span>}
      </div>
      <div className="card-actions" onClick={(e) => e.stopPropagation()}>
        <button className="btn small" onClick={open}>
          Open
        </button>
        {running ? (
          <button className="btn small" onClick={() => void kill(session.id)}>
            Stop
          </button>
        ) : (
          <button className="btn small" onClick={() => void restart(session.id)}>
            {session.kind === 'claude' ? 'Resume' : 'Restart'}
          </button>
        )}
        <button
          className="btn small danger"
          onClick={() => {
            if (!running || window.confirm('This session is running. Stop it and remove?')) void remove(session.id)
          }}
        >
          Remove
        </button>
      </div>
    </article>
  )
}
