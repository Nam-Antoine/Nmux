import { useState } from 'react'
import { rollup, run, update, useStore, workspacePanes } from '../store'
import type { AgentState, Pane, Workspace } from '../types'
import { Avatar, Chevron, Close, KindIcon, Plus, SidebarIcon, StateDot } from './icons'
import { InlineEdit } from './InlineEdit'

function tail(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean)
  return parts.slice(-2).join('/')
}

function WorkspaceRow({ ws, active }: { ws: Workspace; active: boolean }): React.JSX.Element {
  const s = useStore((x) => x)
  const [renaming, setRenaming] = useState(false)
  const ids = workspacePanes(ws)
  const running = ids.filter((id) => s.panes.get(id)?.status === 'running').length
  const state = rollup(s, ids)

  const close = (): void => {
    const warn = running > 0 ? `Close "${ws.name}"? This stops ${running} running pane${running > 1 ? 's' : ''}.` : `Close "${ws.name}"?`
    if (window.confirm(warn)) void run({ op: 'closeWorkspace', workspace: ws.id })
  }

  return (
    <div
      className={`space-row${active ? ' active' : ''}`}
      onClick={() => void run({ op: 'selectWorkspace', workspace: ws.id })}
      onDoubleClick={() => setRenaming(true)}
      title={ws.cwd}
    >
      <Avatar name={ws.name} />
      {renaming ? (
        <InlineEdit
          value={ws.name}
          onDone={(name) => {
            setRenaming(false)
            if (name && name !== ws.name) void run({ op: 'renameWorkspace', workspace: ws.id, name })
          }}
        />
      ) : (
        <>
          <span className="space-name">{ws.name}</span>
          <span className="space-sub">{tail(ws.cwd)}</span>
        </>
      )}
      <span className="row-actions">
        <button
          className="icon-btn"
          title="New Claude tab in this workspace"
          onClick={(e) => {
            e.stopPropagation()
            void run({ op: 'createTab', workspace: ws.id, pane: { kind: 'claude' } })
          }}
        >
          <Plus />
        </button>
        <button
          className="icon-btn"
          title="Close workspace"
          onClick={(e) => {
            e.stopPropagation()
            close()
          }}
        >
          <Close />
        </button>
      </span>
      {running > 0 && <StateDot state={state === 'unknown' ? 'idle' : state} />}
    </div>
  )
}

const GROUPS: Array<{ key: string; label: string; match: (p: Pane) => boolean }> = [
  { key: 'blocked', label: 'Needs you', match: (p) => p.status === 'running' && p.agent === 'blocked' },
  { key: 'working', label: 'Working', match: (p) => p.status === 'running' && p.agent === 'working' },
  { key: 'done', label: 'Done', match: (p) => p.status === 'running' && p.agent === 'done' },
  { key: 'idle', label: 'Idle', match: (p) => p.status === 'running' && (p.agent === 'idle' || p.agent === 'unknown') },
  { key: 'stopped', label: 'Not running', match: (p) => p.status === 'exited' }
]

function AgentRow({ pane, where, focused }: { pane: Pane; where: string; focused: boolean }): React.JSX.Element {
  const state: AgentState = pane.agent
  return (
    <div
      className={`agent-row${focused ? ' active' : ''}`}
      onClick={() => void run({ op: 'focusPane', pane: pane.id })}
      title={pane.cwd}
    >
      <KindIcon kind={pane.kind} />
      <span className="agent-name">{pane.name}</span>
      <span className="agent-where">{where}</span>
      <StateDot state={state} exited={pane.status === 'exited'} />
    </div>
  )
}

function Agents(): React.JSX.Element {
  const s = useStore((x) => x)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({ stopped: true })
  const where = new Map<number, string>()
  let focused: number | undefined
  for (const ws of s.state.workspaces) {
    for (const id of workspacePanes(ws)) where.set(id, ws.name)
    if (ws.id === s.state.activeWorkspace) focused = ws.tabs.find((t) => t.id === ws.activeTab)?.focused
  }
  const agents = s.state.panes.filter((p) => p.kind !== 'shell')

  return (
    <div className="agents">
      <div className="section-head">
        <span>Agents</span>
        <span className="count">{agents.filter((p) => p.status === 'running').length}</span>
      </div>
      <div className="agents-list">
        {agents.length === 0 && <div className="empty-hint">No agents yet.</div>}
        {GROUPS.map((group) => {
          const rows = agents.filter(group.match)
          if (rows.length === 0) return null
          const closed = collapsed[group.key] ?? false
          return (
            <div key={group.key}>
              <div
                className={`group-head ${group.key}`}
                onClick={() => setCollapsed({ ...collapsed, [group.key]: !closed })}
              >
                <Chevron open={!closed} />
                <span>{group.label}</span>
                <span className="count">{rows.length}</span>
              </div>
              {!closed &&
                rows.map((p) => (
                  <AgentRow key={p.id} pane={p} where={where.get(p.id) ?? ''} focused={p.id === focused} />
                ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function Sidebar(): React.JSX.Element {
  const s = useStore((x) => x)
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark">N</span>
        <span className="brand-name">Nmux</span>
        <button className="icon-btn" title="Hide sidebar (Ctrl+B, b)" onClick={() => update({ sidebarOpen: false })}>
          <SidebarIcon />
        </button>
      </div>
      <div className="spaces">
        <div className="section-head">
          <span>Spaces</span>
          <button className="icon-btn" title="New workspace (Ctrl+B, N)" onClick={() => update({ dialog: { type: 'workspace' } })}>
            <Plus />
          </button>
        </div>
        <div className="spaces-list">
          {s.state.workspaces.length === 0 && <div className="empty-hint">Add a project folder to start.</div>}
          {s.state.workspaces.map((ws) => (
            <WorkspaceRow key={ws.id} ws={ws} active={ws.id === s.state.activeWorkspace} />
          ))}
        </div>
      </div>
      <Agents />
    </aside>
  )
}
