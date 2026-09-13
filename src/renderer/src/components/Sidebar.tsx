import { useMemo, useState } from 'react'
import type { SessionMeta } from '@shared/session'
import { activityOf, useNow } from '../lib/activity'
import { groupByFolder, type FolderGroup } from '../lib/folders'
import { useSessions } from '../store/sessions'

const DOT_CLASS = { busy: 'busy', idle: 'running', exited: 'exited' } as const
const ACTIVITY_LABEL = { busy: 'working', idle: 'idle', exited: 'exited' } as const

export function Sidebar(): React.JSX.Element {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const collapsed = useSessions((s) => s.collapsedFolders)
  const folded = useSessions((s) => s.sidebarFolded)
  const overviewOpen = useSessions((s) => s.overviewOpen)
  const openDialog = useSessions((s) => s.openDialog)
  const toggleSidebar = useSessions((s) => s.toggleSidebar)
  const toggleOverview = useSessions((s) => s.toggleOverview)
  const select = useSessions((s) => s.select)
  const now = useNow(1000)

  const groups = useMemo(() => groupByFolder(sessions), [sessions])

  if (folded) {
    return (
      <aside className="sidebar folded">
        <header className="sidebar-header">
          <button className="icon-btn" onClick={toggleSidebar} title="Expand sidebar (Ctrl+B)" aria-label="Expand sidebar">
            »
          </button>
        </header>
        <button className="icon-btn rail-new" onClick={() => openDialog()} title="New session (Ctrl+Shift+N)" aria-label="New session">
          +
        </button>
        <button
          className={`icon-btn rail-new${overviewOpen ? ' on' : ''}`}
          onClick={toggleOverview}
          title="Session overview (Ctrl+Shift+O)"
          aria-label="Session overview"
        >
          ⊞
        </button>
        <ul className="rail-list">
          {sessions.map((session) => {
            const activity = activityOf(session, now)
            return (
              <li key={session.id}>
                <button
                  className={`rail-item${session.id === activeId ? ' active' : ''}`}
                  onClick={() => select(session.id)}
                  title={`${session.name} — ${ACTIVITY_LABEL[activity]}`}
                >
                  <span className={`status-dot ${DOT_CLASS[activity]}`} />
                </button>
              </li>
            )
          })}
        </ul>
      </aside>
    )
  }

  return (
    <aside className="sidebar">
      <header className="sidebar-header">
        <span className="brand">Nmux</span>
        <div className="row" style={{ gap: 4 }}>
          <button className="btn primary small" onClick={() => openDialog()} title="New session (Ctrl+Shift+N)">
            + New
          </button>
          <button
            className={`icon-btn${overviewOpen ? ' on' : ''}`}
            onClick={toggleOverview}
            title="Session overview (Ctrl+Shift+O)"
            aria-label="Session overview"
            aria-pressed={overviewOpen}
          >
            ⊞
          </button>
          <button className="icon-btn" onClick={toggleSidebar} title="Collapse sidebar (Ctrl+B)" aria-label="Collapse sidebar">
            «
          </button>
        </div>
      </header>

      <div className="folder-list">
        {groups.map((group) => (
          <FolderSection
            key={group.key}
            group={group}
            collapsed={collapsed.includes(group.key)}
            activeId={activeId}
            now={now}
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
  activeId,
  now
}: {
  group: FolderGroup
  collapsed: boolean
  activeId: string | null
  now: number
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
        <span
          className={`folder-count muted${group.running > 0 ? ' has-running' : ''}`}
          title={`${group.running} running / ${group.sessions.length} total`}
        >
          {group.running}/{group.sessions.length}
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
            <SessionItem key={session.id} session={session} active={session.id === activeId} now={now} />
          ))}
        </ul>
      )}
    </section>
  )
}

function SessionItem({ session, active, now }: { session: SessionMeta; active: boolean; now: number }): React.JSX.Element {
  const select = useSessions((s) => s.select)
  const restart = useSessions((s) => s.restart)
  const kill = useSessions((s) => s.kill)
  const remove = useSessions((s) => s.remove)
  const rename = useSessions((s) => s.rename)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(session.name)

  const running = session.status === 'running'
  const activity = activityOf(session, now)

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
      <span className={`status-dot ${DOT_CLASS[activity]}`} title={ACTIVITY_LABEL[activity]} />
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
          <span className={`tag${running ? ' ok' : ''}${activity === 'busy' ? ' live' : ''}`}>{ACTIVITY_LABEL[activity]}</span>
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
