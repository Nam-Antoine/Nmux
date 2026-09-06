import { useState } from 'react'
import type { SessionMeta } from '@shared/session'
import { useSessions } from '../store/sessions'

export function Sidebar(): React.JSX.Element {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const openDialog = useSessions((s) => s.openDialog)

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <span className="brand">Nmux</span>
        <button className="btn primary small" onClick={openDialog} title="New session (Ctrl+Shift+N)">
          + New
        </button>
      </header>

      <ul className="session-list">
        {sessions.map((session) => (
          <SessionItem key={session.id} session={session} active={session.id === activeId} />
        ))}
        {sessions.length === 0 && <li className="muted session-empty">No sessions yet.</li>}
      </ul>

      <footer className="sidebar-footer muted">
        Ctrl+PgUp / Ctrl+PgDn to switch
      </footer>
    </aside>
  )
}

function SessionItem({ session, active }: { session: SessionMeta; active: boolean }): React.JSX.Element {
  const select = useSessions((s) => s.select)
  const restart = useSessions((s) => s.restart)
  const kill = useSessions((s) => s.kill)
  const remove = useSessions((s) => s.remove)
  const rename = useSessions((s) => s.rename)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(session.name)

  const running = session.status === 'running'

  const commitRename = (): void => {
    setEditing(false)
    if (draft.trim() && draft.trim() !== session.name) void rename(session.id, draft)
  }

  return (
    <li
      className={`session-item${active ? ' active' : ''}`}
      onClick={() => select(session.id)}
      onDoubleClick={() => {
        setDraft(session.name)
        setEditing(true)
      }}
    >
      <span className={`status-dot ${running ? 'running' : 'exited'}`} title={session.status} />
      <div className="session-text">
        {editing ? (
          <input
            className="rename-input"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename()
              if (e.key === 'Escape') setEditing(false)
            }}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span className="session-name">{session.name}</span>
        )}
        <span className="session-sub muted">
          {session.kind === 'claude' ? 'claude' : 'shell'} · {basename(session.cwd)}
        </span>
      </div>
      <div className="session-actions" onClick={(e) => e.stopPropagation()}>
        {running ? (
          <button className="icon-btn" title="Stop process" onClick={() => void kill(session.id)}>
            ■
          </button>
        ) : (
          <button
            className="icon-btn"
            title={session.kind === 'claude' ? 'Resume conversation' : 'Restart shell'}
            onClick={() => void restart(session.id)}
          >
            ▶
          </button>
        )}
        <button
          className="icon-btn danger"
          title="Remove from list"
          onClick={() => {
            if (!running || window.confirm('This session is running. Stop it and remove?')) {
              void remove(session.id)
            }
          }}
        >
          ×
        </button>
      </div>
    </li>
  )
}

function basename(p: string): string {
  const parts = p.replace(/[\\/]+$/, '').split(/[\\/]/)
  return parts[parts.length - 1] || p
}
