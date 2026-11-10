import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { openCloudUrl } from '../api'
import { activeTab, activeWorkspace, leaves, rollup, run, tabLabel, update, useStore } from '../store'
import { terminals } from '../terminals'
import type { LayoutNode, Pane, PaneId, Tab, Workspace } from '../types'
import { Close, KIND_LABEL, KindIcon, Plus, SidebarIcon, SplitDown, SplitRight, StateDot, Zoom } from './icons'
import { InlineEdit } from './InlineEdit'

function TabBar({ ws, current }: { ws: Workspace; current: Tab }): React.JSX.Element {
  const s = useStore((x) => x)
  const [menu, setMenu] = useState(false)
  const [renaming, setRenaming] = useState<number | null>(null)

  const closeTab = (tab: Tab): void => {
    const running = leaves(tab.layout).filter((id) => s.panes.get(id)?.status === 'running').length
    if (running === 0 || window.confirm(`Close this tab? This stops ${running} running pane${running > 1 ? 's' : ''}.`)) {
      void run({ op: 'closeTab', tab: tab.id })
    }
  }
  const add = (pane: Parameters<typeof run>[0] & { op: 'createTab' }): void => {
    setMenu(false)
    void run(pane)
  }

  return (
    <div className="tabbar">
      {!s.sidebarOpen && (
        <button className="icon-btn" title="Show sidebar (Ctrl+B, b)" onClick={() => update({ sidebarOpen: true })}>
          <SidebarIcon />
        </button>
      )}
      {ws.tabs.map((tab, i) => {
        const ids = leaves(tab.layout)
        const focused = s.panes.get(tab.focused)
        const anyRunning = ids.some((id) => s.panes.get(id)?.status === 'running')
        return (
          <div
            key={tab.id}
            className={`tab${tab.id === current.id ? ' active' : ''}`}
            onClick={() => void run({ op: 'selectTab', tab: tab.id })}
            onDoubleClick={() => setRenaming(tab.id)}
            onAuxClick={(e) => e.button === 1 && closeTab(tab)}
          >
            <KindIcon kind={focused?.kind ?? 'shell'} />
            {renaming === tab.id ? (
              <InlineEdit
                value={tabLabel(s, tab, i)}
                onDone={(name) => {
                  setRenaming(null)
                  if (name !== null) void run({ op: 'renameTab', tab: tab.id, name })
                }}
              />
            ) : (
              <span className="tab-name">{tabLabel(s, tab, i)}</span>
            )}
            {ids.length > 1 && <span className="tab-count">{ids.length}</span>}
            <StateDot state={rollup(s, ids)} exited={!anyRunning} />
            <button
              className="icon-btn tab-close"
              title="Close tab"
              onClick={(e) => {
                e.stopPropagation()
                closeTab(tab)
              }}
            >
              <Close />
            </button>
          </div>
        )
      })}
      <div className="tab-add">
        <button className="icon-btn" title="New tab" onClick={() => setMenu(!menu)}>
          <Plus />
        </button>
        {menu && (
          <>
            <div className="menu-backdrop" onClick={() => setMenu(false)} />
            <div className="menu">
              <button onClick={() => add({ op: 'createTab', workspace: ws.id, pane: { kind: 'claude' } })}>
                <KindIcon kind="claude" /> Claude Code
              </button>
              <button
                onClick={() => add({ op: 'createTab', workspace: ws.id, pane: { kind: 'claude', skipPermissions: true } })}
              >
                <KindIcon kind="claude" /> Claude Code, skip permission prompts
              </button>
              <button onClick={() => add({ op: 'createTab', workspace: ws.id, pane: { kind: 'shell' } })}>
                <KindIcon kind="shell" /> Shell
              </button>
              <button
                onClick={() => {
                  setMenu(false)
                  update({ dialog: { type: 'tab', workspace: ws.id, kind: 'cloud' } })
                }}
              >
                <KindIcon kind="cloud" /> Cloud task…
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

function ExitedOverlay({ pane }: { pane: Pane }): React.JSX.Element {
  const cloudReady = pane.kind === 'cloud' && Boolean(pane.cloudSessionId)
  const label = pane.kind === 'claude' ? 'Resume conversation' : pane.kind === 'shell' ? 'Start shell' : 'Run task again'
  return (
    <div className="pane-overlay">
      <div className="overlay-card">
        <strong>{cloudReady ? 'Cloud session created' : 'Not running'}</strong>
        {pane.exitCode !== undefined && pane.exitCode !== 0 && <span className="muted"> (exit code {pane.exitCode})</span>}
        <div className="row">
          {cloudReady ? (
            <button className="btn primary" onClick={() => void run({ op: 'teleportPane', pane: pane.id })}>
              Teleport into a new pane
            </button>
          ) : (
            <button className="btn primary" onClick={() => void run({ op: 'restartPane', pane: pane.id })}>
              {label}
            </button>
          )}
          {pane.cloudUrl && (
            <button className="btn" onClick={() => openCloudUrl(pane.cloudUrl!)}>
              Open on claude.ai
            </button>
          )}
          <button className="btn" onClick={() => void run({ op: 'closePane', pane: pane.id })}>
            Close pane
          </button>
        </div>
        {pane.kind === 'cloud' && (
          <div className="muted small">
            {cloudReady
              ? 'It runs in the cloud on its own. Teleport opens its conversation here as a local Claude pane.'
              : 'No cloud session was created. Check the message above, then try again.'}
          </div>
        )}
      </div>
    </div>
  )
}

function PaneView({ id, focused, solo }: { id: PaneId; focused: boolean; solo: boolean }): React.JSX.Element | null {
  const pane = useStore((s) => s.panes.get(id))
  const zoomed = useStore((s) => activeTab(s)?.zoomed ?? false)
  const hostRef = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) return
    terminals.mount(id, host)
    return () => terminals.unmount(id, host)
  }, [id])

  useEffect(() => {
    if (focused) terminals.focus(id)
  }, [focused, id, pane?.status])

  if (!pane) return null
  const close = (): void => {
    if (pane.status !== 'running' || window.confirm(`Close "${pane.name}"? This stops it.`)) {
      void run({ op: 'closePane', pane: id })
    }
  }

  return (
    <div
      className={`pane${focused ? ' focused' : ''}${solo ? ' solo' : ''}`}
      onMouseDownCapture={() => {
        if (!focused) void run({ op: 'focusPane', pane: id })
      }}
    >
      <div className="pane-head">
        <KindIcon kind={pane.kind} />
        <span className="pane-kind">{KIND_LABEL[pane.kind]}</span>
        <span className="pane-name">{pane.name}</span>
        <span className="pane-cwd">{pane.cwd}</span>
        {pane.skipPermissions && <span className="badge" title="Runs with --dangerously-skip-permissions">skip-permissions</span>}
        <StateDot state={pane.agent} exited={pane.status === 'exited'} />
        <span className="pane-actions">
          <button className="icon-btn" title="Split right (Ctrl+B, v)" onClick={() => void run({ op: 'splitPane', target: id, dir: 'right', pane: { kind: 'shell' } })}>
            <SplitRight />
          </button>
          <button className="icon-btn" title="Split down (Ctrl+B, -)" onClick={() => void run({ op: 'splitPane', target: id, dir: 'down', pane: { kind: 'shell' } })}>
            <SplitDown />
          </button>
          {(!solo || zoomed) && (
            <button className={`icon-btn${zoomed ? ' on' : ''}`} title="Zoom (Ctrl+B, z)" onClick={() => void run({ op: 'toggleZoom', pane: id })}>
              <Zoom />
            </button>
          )}
          <button className="icon-btn" title="Close pane (Ctrl+B, x)" onClick={close}>
            <Close />
          </button>
        </span>
      </div>
      <div className="pane-body">
        <div ref={hostRef} className="pane-term" />
        {pane.status === 'exited' && <ExitedOverlay pane={pane} />}
      </div>
    </div>
  )
}

function Split({ node, path, tab }: { node: Extract<LayoutNode, { type: 'split' }>; path: number[]; tab: Tab }): React.JSX.Element {
  const ref = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<number | null>(null)
  const ratio = drag ?? node.ratio
  const horizontal = node.dir === 'right'

  const ratioAt = (e: React.PointerEvent): number => {
    const box = ref.current!.getBoundingClientRect()
    const raw = horizontal ? (e.clientX - box.left) / box.width : (e.clientY - box.top) / box.height
    return Math.min(0.9, Math.max(0.1, raw))
  }

  return (
    <div ref={ref} className={`split ${horizontal ? 'h' : 'v'}`}>
      <div className="split-child" style={{ flex: `${ratio} 1 0` }}>
        <Tree node={node.a} path={[...path, 0]} tab={tab} />
      </div>
      <div
        className={`divider${drag !== null ? ' dragging' : ''}`}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          setDrag(ratioAt(e))
        }}
        onPointerMove={(e) => drag !== null && setDrag(ratioAt(e))}
        onPointerUp={(e) => {
          if (drag === null) return
          e.currentTarget.releasePointerCapture(e.pointerId)
          void run({ op: 'setRatio', tab: tab.id, path, ratio: drag }).then(() => setDrag(null))
        }}
        onDoubleClick={() => void run({ op: 'setRatio', tab: tab.id, path, ratio: 0.5 })}
      />
      <div className="split-child" style={{ flex: `${1 - ratio} 1 0` }}>
        <Tree node={node.b} path={[...path, 1]} tab={tab} />
      </div>
    </div>
  )
}

function Tree({ node, path, tab }: { node: LayoutNode; path: number[]; tab: Tab }): React.JSX.Element {
  if (node.type === 'leaf') {
    return <PaneView id={node.pane} focused={tab.focused === node.pane} solo={tab.layout.type === 'leaf'} />
  }
  return <Split node={node} path={path} tab={tab} />
}

function Empty(): React.JSX.Element {
  const sidebarOpen = useStore((s) => s.sidebarOpen)
  return (
    <div className="empty">
      {!sidebarOpen && (
        <button className="icon-btn corner" title="Show sidebar" onClick={() => update({ sidebarOpen: true })}>
          <SidebarIcon />
        </button>
      )}
      <div className="empty-card">
        <h1>Nmux</h1>
        <p>Run Claude Code in panes that keep going when this window is closed.</p>
        <button className="btn primary" onClick={() => update({ dialog: { type: 'workspace' } })}>
          Add a project folder
        </button>
        <p className="muted small">Ctrl+B then ? shows the keyboard shortcuts.</p>
      </div>
    </div>
  )
}

export function Main(): React.JSX.Element {
  const s = useStore((x) => x)
  const ws = activeWorkspace(s)
  const tab = activeTab(s)
  if (!ws || !tab) return <Empty />
  return (
    <main className="main">
      <TabBar ws={ws} current={tab} />
      <div className="tab-body" key={tab.id}>
        {tab.zoomed ? (
          <PaneView id={tab.focused} focused solo />
        ) : (
          <Tree node={tab.layout} path={[]} tab={tab} />
        )}
      </div>
    </main>
  )
}
