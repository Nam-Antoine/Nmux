import { useState } from 'react'
import { sure } from '../api'
import { rollup, run, update, useStore, workspacePanes } from '../store'
import type { AgentState, Machine, Pane, Workspace } from '../types'
import { Avatar, Chevron, Clock, Close, FilesIcon, Keyboard, Pencil, PortsIcon, KindIcon, Monitor, Play, Plus, Search, ServerIcon, SidebarIcon, shownKind, StateDot } from './icons'
import { ContextMenu } from './ContextMenu'
import { InlineEdit } from './InlineEdit'

function tail(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean)
  return parts.slice(-2).join('/')
}

function WorkspaceRow({ ws, active }: { ws: Workspace; active: boolean }): React.JSX.Element {
  const s = useStore((x) => x)
  const [renaming, setRenaming] = useState(false)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const ids = workspacePanes(ws)
  const running = ids.filter((id) => s.panes.get(id)?.status === 'running').length
  const state = rollup(s, ids)

  const remote = ws.machine !== undefined
  const forwards = remote ? s.forwards.filter((f) => f.host === s.state.machines.find((m) => m.id === ws.machine)?.target && f.state === 'active').length : 0
  const panel = (which: 'files' | 'ports') => (e: React.MouseEvent): void => {
    e.stopPropagation()
    if (active && s.panel === which) return update({ panel: null })
    if (!active) void run({ op: 'selectWorkspace', workspace: ws.id })
    update({ panel: which })
  }

  const close = (): void => {
    const warn = running > 0 ? `Close "${ws.name}"? This stops ${running} running pane${running > 1 ? 's' : ''}.` : `Close "${ws.name}"?`
    void sure(warn).then((yes) => (yes ? run({ op: 'closeWorkspace', workspace: ws.id }) : null))
  }

  return (
    <div
      className={`space-row${active ? ' active' : ''}`}
      onClick={() => void run({ op: 'selectWorkspace', workspace: ws.id })}
      onDoubleClick={() => setRenaming(true)}
      onContextMenu={(e) => {
        e.preventDefault()
        setMenu({ x: e.clientX, y: e.clientY })
      }}
      title={`n${ws.number} · ${ws.cwd}`}
    >
      {menu && (
        <ContextMenu at={menu} onClose={() => setMenu(null)}>
            <button onClick={() => void run({ op: 'createTab', workspace: ws.id, pane: { kind: 'claude' } })}>
              <KindIcon kind="claude" /> New Claude Code tab
            </button>
            <button onClick={() => void run({ op: 'createTab', workspace: ws.id, pane: { kind: 'shell' } })}>
              <KindIcon kind="shell" /> New shell tab
            </button>
            <button onClick={() => setRenaming(true)}>
              <Pencil /> Rename
            </button>
            <button className="danger" onClick={close}>
              <Close /> Close workspace
            </button>
        </ContextMenu>
      )}
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
      <span className="space-tools">
        <button className={`icon-btn${active && s.panel === 'files' ? ' on' : ''}`} title={`Files in ${ws.name}`} onClick={panel('files')}>
          <FilesIcon />
        </button>
        <button
          className={`icon-btn${active && s.panel === 'ports' ? ' on' : ''}${forwards > 0 ? ' live' : ''}`}
          title={remote ? `Ports and forwards on this machine${forwards > 0 ? ` (${forwards} forwarded)` : ''}` : `Ports opened by ${ws.name}`}
          onClick={panel('ports')}
        >
          <PortsIcon />
        </button>
      </span>
      {running > 0 ? (
        <StateDot state={state === 'unknown' ? 'idle' : state} />
      ) : (
        <button
          className="icon-btn resume"
          title="Resume: start this workspace's pane again"
          onClick={(e) => {
            e.stopPropagation()
            const tab = ws.tabs.find((t) => t.id === ws.activeTab) ?? ws.tabs[0]
            void run({ op: 'selectWorkspace', workspace: ws.id })
            if (tab) void run({ op: 'restartPane', pane: tab.focused })
          }}
        >
          <Play />
        </button>
      )}
    </div>
  )
}

const MACHINE_TITLE: Record<Machine['status'], string> = {
  unknown: 'Not checked yet. Click to check.',
  checking: 'Connecting…',
  online: 'Connected. Click to check again.',
  auth: 'Needs a password or key. Panes can still ask for it. Click to check again.',
  error: 'Cannot connect. Click to try again.'
}

function MachineGroup({ machine, spaces, activeId, running }: { machine?: Machine; spaces: Workspace[]; activeId?: number; running: boolean }): React.JSX.Element {
  const [open, setOpen] = useState(true)
  const [renaming, setRenaming] = useState(false)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const add = (): void => update({ dialog: { type: 'workspace', machine: machine?.id } })
  const status = machine?.status ?? 'online'
  const trouble = status === 'auth' || status === 'error'

  const remove = (): void => {
    if (!machine) return
    if (spaces.length > 0) return void window.alert(`Close the workspaces on "${machine.label}" first.`)
    void sure(`Remove "${machine.label}"? Nothing is stopped on the machine.`).then((yes) => (yes ? run({ op: 'removeMachine', machine: machine.id }) : null))
  }

  const trust = (): void => {
    if (!machine) return
    const set = (trusted: boolean): Promise<unknown> => run({ op: 'trustMachine', machine: machine.id, trusted })
    if (machine.trusted) return void set(false)
    const warn = `Let nc on "${machine.label}" reach every pane? Anyone who can use that account can then read and type into the panes on this computer.`
    void sure(warn).then((yes) => (yes ? set(true) : null))
  }

  return (
    <>
      <div
        className={`machine-row${trouble ? ' trouble' : ''}`}
        onClick={() => setOpen(!open)}
        onDoubleClick={() => machine && setRenaming(true)}
        onContextMenu={(e) => {
          if (!machine) return
          e.preventDefault()
          setMenu({ x: e.clientX, y: e.clientY })
        }}
        title={machine ? `ssh ${machine.target}${machine.detail ? `\n${machine.detail}` : ''}` : undefined}
      >
        {menu && (
          <ContextMenu at={menu} onClose={() => setMenu(null)}>
            <button onClick={add}>
              <Plus /> New workspace
            </button>
            <button onClick={() => setRenaming(true)}>
              <Pencil /> Rename
            </button>
            <button onClick={trust}>
              <Keyboard /> {machine?.trusted ? 'Keep its nc to its own panes' : 'Let its nc reach every pane'}
            </button>
            <button className="danger" onClick={remove}>
              <Close /> Remove machine
            </button>
          </ContextMenu>
        )}
        {machine ? <ServerIcon /> : <Monitor />}
        {machine && renaming ? (
          <InlineEdit
            value={machine.label}
            onDone={(label) => {
              setRenaming(false)
              if (label && label !== machine.label) void run({ op: 'renameMachine', machine: machine.id, label })
            }}
          />
        ) : (
          <span className="machine-name">{machine ? machine.label : 'This computer'}</span>
        )}
        <button
          className="icon-btn"
          title={machine ? `New workspace on ${machine.label}` : 'Add a project folder'}
          onClick={(e) => {
            e.stopPropagation()
            add()
          }}
        >
          <Plus />
        </button>
        {machine ? (
          <button
            className={`machine-state ${status}`}
            title={`${MACHINE_TITLE[status]}${machine.detail ? `\n${machine.detail}` : ''}`}
            onClick={(e) => {
              e.stopPropagation()
              void run({ op: 'checkMachine', machine: machine.id })
            }}
          >
            {status === 'auth' ? '! auth' : status === 'error' ? '! error' : <span className={`machine-dot ${status}`} />}
          </button>
        ) : (
          <span className={`machine-dot${running ? '' : ' off'}`} title={running ? 'Panes are running' : 'Nothing is running'} />
        )}
        <Chevron open={open} />
      </div>
      {open && spaces.map((ws) => <WorkspaceRow key={ws.id} ws={ws} active={ws.id === activeId} />)}
    </>
  )
}

const GROUPS: Array<{ key: string; label: string; match: (p: Pane) => boolean }> = [
  { key: 'blocked', label: 'Needs you', match: (p) => p.status === 'running' && p.agent === 'blocked' },
  { key: 'working', label: 'Working', match: (p) => p.status === 'running' && p.agent === 'working' },
  { key: 'done', label: 'Done', match: (p) => p.status === 'running' && p.agent === 'done' },
  { key: 'idle', label: 'Idle', match: (p) => p.status === 'running' && (p.agent === 'idle' || p.agent === 'unknown') },
  { key: 'stopped', label: 'Not running', match: (p) => p.status === 'exited' }
]

function age(since: number): string {
  const minutes = Math.max(0, Math.floor((Date.now() - since) / 60000))
  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`
  return `${Math.floor(minutes / 60 / 24)}d`
}

function AgentRow({ pane, where, focused }: { pane: Pane; where: string; focused: boolean }): React.JSX.Element {
  const state: AgentState = pane.agent
  const exited = pane.status === 'exited'
  const quiet = exited || state === 'idle' || state === 'unknown'
  return (
    <div
      className={`agent-row${focused ? ' active' : ''}${quiet ? ' quiet' : ''}`}
      onClick={() => void run({ op: 'focusPane', pane: pane.id })}
      title={pane.cwd}
    >
      <KindIcon kind={shownKind(pane)} />
      <span className="agent-name">{pane.name}</span>
      <span className="agent-where">{where}</span>
      <span className="agent-age" title="Time since this agent was created">{age(pane.createdAt)}</span>
      {exited ? (
        <button
          className="icon-btn resume"
          title={pane.kind === 'claude' ? 'Resume conversation' : 'Start again'}
          onClick={(e) => {
            e.stopPropagation()
            void run({ op: 'focusPane', pane: pane.id })
            void run({ op: 'restartPane', pane: pane.id })
          }}
        >
          <Play />
        </button>
      ) : (
        <StateDot state={state === 'unknown' ? 'idle' : state} />
      )}
    </div>
  )
}

function Agents({ filter }: { filter: string }): React.JSX.Element {
  const s = useStore((x) => x)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [history, setHistory] = useState(true)
  const where = new Map<number, string>()
  let focused: number | undefined
  for (const ws of s.state.workspaces) {
    const label = s.state.machines.find((m) => m.id === ws.machine)?.label ?? ws.name
    for (const id of workspacePanes(ws)) where.set(id, label)
    if (ws.id === s.state.activeWorkspace) focused = ws.tabs.find((t) => t.id === ws.activeTab)?.focused
  }
  const agents = s.state.panes.filter(
    (p) => shownKind(p) !== 'shell' && (!filter || `${p.name} ${where.get(p.id) ?? ''}`.toLowerCase().includes(filter))
  )
  const stopped = agents.filter((p) => p.status === 'exited').length

  return (
    <div className="agents">
      <div className="section-head">
        <span>Agents</span>
        <button
          className={`icon-btn${history ? ' on' : ''}`}
          title={history ? 'Hide agents that are not running' : `Show agents that are not running (${stopped})`}
          onClick={() => setHistory(!history)}
        >
          <Clock />
        </button>
      </div>
      <div className="agents-list">
        {agents.length === 0 && <div className="empty-hint">{filter ? 'No agent matches.' : 'No agents yet.'}</div>}
        {agents.length > 0 && agents.length === stopped && !history && (
          <div className="empty-hint">Nothing is running. The clock shows {stopped} stopped.</div>
        )}
        {GROUPS.map((group) => {
          if (group.key === 'stopped' && !history) return null
          const rows = agents.filter(group.match)
          if (rows.length === 0) return null
          const closed = collapsed[group.key] ?? false
          return (
            <div key={group.key}>
              <div
                className={`group-head ${group.key}${closed ? ' closed' : ''}`}
                onClick={() => setCollapsed({ ...collapsed, [group.key]: !closed })}
              >
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
  const [searching, setSearching] = useState(false)
  const [query, setQuery] = useState('')
  const filter = searching ? query.trim().toLowerCase() : ''
  const spaces = s.state.workspaces.filter((ws) => !filter || `${ws.name} ${ws.cwd}`.toLowerCase().includes(filter))
  const anyRunning = s.state.panes.some((p) => p.status === 'running')
  const newWorkspace = (): void => update({ dialog: { type: 'workspace' } })

  return (
    <aside className="sidebar">
      <div className="brand" data-tauri-drag-region>
        <span className="brand-mark">N</span>
        <span className="brand-name" data-tauri-drag-region>
          Nmux
        </span>
        <button
          className={`icon-btn${searching ? ' on' : ''}`}
          title="Search spaces and agents"
          onClick={() => {
            setSearching(!searching)
            setQuery('')
          }}
        >
          <Search />
        </button>
        <button className="icon-btn" title="Hide sidebar (Ctrl+B, b)" onClick={() => update({ sidebarOpen: false })}>
          <SidebarIcon />
        </button>
      </div>
      {searching && (
        <div className="side-search">
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Escape' && setSearching(false)}
            placeholder="Search"
            spellCheck={false}
          />
        </div>
      )}
      <div className="spaces">
        <div className="section-head">
          <span>Spaces</span>
          <button className="icon-btn" title="New workspace (Ctrl+B, N)" onClick={newWorkspace}>
            <Plus />
          </button>
        </div>
        <div className="spaces-list">
          <MachineGroup
            spaces={spaces.filter((ws) => ws.machine === undefined)}
            activeId={s.state.activeWorkspace}
            running={anyRunning}
          />
          {s.state.workspaces.length === 0 && <div className="empty-hint">Add a project folder to start.</div>}
          {s.state.machines.map((machine) => (
            <MachineGroup
              key={machine.id}
              machine={machine}
              spaces={spaces.filter((ws) => ws.machine === machine.id)}
              activeId={s.state.activeWorkspace}
              running={false}
            />
          ))}
        </div>
      </div>
      <Agents filter={filter} />
      <div className="side-foot">
        <button className="foot-row" onClick={() => update({ dialog: { type: 'help' } })}>
          <Keyboard /> Shortcuts
        </button>
        <button className="foot-row dim" onClick={() => update({ dialog: { type: 'machine' } })}>
          <Plus /> Add machine
        </button>
      </div>
    </aside>
  )
}
