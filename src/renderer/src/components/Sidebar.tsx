import { useMemo, useState } from 'react'
import type { SessionMeta } from '@shared/session'
import { groupByFolder, type FolderGroup } from '../lib/folders'
import { useSessions } from '../store/sessions'

export function Sidebar(): React.JSX.Element {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const collapsed = useSessions((s) => s.collapsedFolders)
  const openDialog = useSessions((s) => s.openDialog)

  const groups = useMemo(() => groupByFolder(sessions), [sessions])

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <span className="brand">Nmux</span>
        <button className="btn primary small" onClick={() => openDialog()} title="New session (Ctrl+Shift+N)">
          + New
        </button>
      </header>

      <div className="folder-list">
        {groups.map((group) => (
          <FolderSection
            key={group.key}
            group={group}
            collapsed={collapsed.includes(group.key)}
            activeId={activeId}
          />
        ))}
        {groups.length === 0 && <div className="muted session-empty">No sessions yet.</div>}
      </div>

      <footer className="sidebar-footer muted">
        Ctrl+PgUp / Ctrl+PgDn to switch
      </footer>
    </aside>
  )
}

function FolderSection({
  group,
  collapsed,
  activeId
}: {
  group: FolderGroup
  collapsed: boolean
  activeId: string | null
}): React.JSX.Element {
  const toggleFolder = useSessions((s) => s.toggleFolder)
  const openDialog = useSessions((s) => s.openDialog)
  const containsActive = group.sessions.some((s) => s.id === activeId)

  return (
    <section className={`folder${collapsed ? ' collapsed' : ''}${containsActive ? ' has-active' : ''}`}>
      <div
        className="folder-header"
        role="button"
        tabIndex={0}
        aria-expanded={!collapsed}
        title={group.path}
        onClick={() => toggleFolder(group.key)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            toggleFolder(group.key)
          }
        }}
      >
        <span className="chevron" aria-hidden>
          {collapsed ? '▸' : '▾'}
        </span>
        <span className="folder-name">{group.name}</span>
        <span className="folder-count muted" title={`${group.running} running / ${group.sessions.length} total`}>
          {group.running > 0 ? `${group.running}/${group.sessions.length}` : group.sessions.length}
        </span>
        <button
          className="icon-btn folder-add"
          title={`New session in ${group.name}`}
          aria-label={`New session in ${group.name}`}
          onClick={(e) => {
            e.stopPropagation()
            openDialog({ cwd: group.path })
          }}
        >
          +
        </button>
      </div>

      {!collapsed && (
        <ul className="session-list">
          {group.sessions.map((session) => (
            <SessionItem key={session.id} session={session} active={session.id === activeId} />
          ))}
        </ul>
      )}
    </section>
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
          {session.kind === 'claude' ? 'claude' : 'shell'}
          {session.skipPermissions && (
            <span className="tag warn" title="Started with --dangerously-skip-permissions">
              skips permissions
            </span>
          )}
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
