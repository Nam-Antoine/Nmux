import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { appWindow, openCloudUrl } from '../api'
import { activeTab, activeWorkspace, leaves, paneAddress, rollup, run, tabLabel, togglePanel, update, useStore } from '../store'
import { terminals } from '../terminals'
import type { LayoutNode, Pane, PaneId, Side, Tab, Workspace } from '../types'
import { BranchIcon, Close, FilesIcon, FolderIcon, KIND_LABEL, KindIcon, Monitor, Play, Plus, ServerIcon, PortsIcon, SidebarIcon, shownKind, SplitDown, SplitRight, StateDot, WinClose, WinMax, WinMin, Zoom } from './icons'
import { ContextMenu } from './ContextMenu'
import { InlineEdit } from './InlineEdit'
import { SidePanel } from './SidePanel'

function WindowControls(): React.JSX.Element {
  return (
    <div className="win-controls">
      <button className="win-btn" title="Minimize" onClick={appWindow.minimize}>
        <WinMin />
      </button>
      <button className="win-btn" title="Maximize" onClick={appWindow.toggleMaximize}>
        <WinMax />
      </button>
      <button className="win-btn close" title="Close to the tray. Panes keep running." onClick={appWindow.close}>
        <WinClose />
      </button>
    </div>
  )
}

function StatusBar({ ws }: { ws: Workspace }): React.JSX.Element {
  const s = useStore((x) => x)
  const agents = s.state.panes.filter((p) => shownKind(p) !== 'shell' && p.status === 'running')
  const blocked = agents.filter((p) => p.agent === 'blocked').length
  const machine = s.state.machines.find((m) => m.id === ws.machine)
  return (
    <div className="statusbar">
      <span className="status-item">
        {machine ? <ServerIcon /> : <Monitor />} {machine ? machine.label : 'This computer'}
      </span>
      <span className="status-item path" title={ws.cwd}>
        <FolderIcon /> <span>{ws.cwd}</span>
      </span>
      <span className="status-item">
        <span className="status-bullet" /> {agents.length} agent{agents.length === 1 ? '' : 's'}
      </span>
      {blocked > 0 && (
        <span className="status-item warn">
          <span className="status-bullet" /> {blocked} need{blocked === 1 ? 's' : ''} you
        </span>
      )}
      <span className="grow" />
      <span className="status-hint" onClick={() => update({ dialog: { type: 'help' } })}>
        Ctrl+B ?&nbsp; shortcuts
      </span>
    </div>
  )
}

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
    <div className="tabbar" data-tauri-drag-region>
      {!s.sidebarOpen && (
        <button className="icon-btn" title="Show sidebar (Ctrl+B, b)" onClick={() => update({ sidebarOpen: true })}>
          <SidebarIcon />
        </button>
      )}
      <div className="tabs">
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
            <KindIcon kind={focused ? shownKind(focused) : 'shell'} />
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
      </div>
      <div className="drag" data-tauri-drag-region />
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
                <KindIcon kind="claude" /> Claude Code, skip permission
              </button>
              <button onClick={() => add({ op: 'createTab', workspace: ws.id, pane: { kind: 'shell' } })}>
                <KindIcon kind="shell" /> Shell
              </button>
              {ws.machine === undefined && (
                <button
                  onClick={() => {
                    setMenu(false)
                    update({ dialog: { type: 'tab', workspace: ws.id, kind: 'cloud' } })
                  }}
                >
                  <KindIcon kind="cloud" /> Cloud task…
                </button>
              )}
            </div>
          </>
        )}
      </div>
      <button className={`bar-btn${s.panel === 'files' ? ' on' : ''}`} title="Files (Ctrl+B, f)" onClick={() => togglePanel('files')}>
        <FilesIcon /> Files
      </button>
      <button className={`bar-btn${s.panel === 'ports' ? ' on' : ''}`} title="Ports and SSH forwards (Ctrl+B, o)" onClick={() => togglePanel('ports')}>
        <PortsIcon /> Ports
      </button>
      <button className={`bar-btn${s.panel === 'changes' ? ' on' : ''}`} title="Git changes (Ctrl+B, g)" onClick={() => togglePanel('changes')}>
        <BranchIcon /> Changes
      </button>
      <WindowControls />
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

interface Drop {
  target: PaneId
  side: Side | 'center'
  box: { left: number; top: number; width: number; height: number }
}

function dropAt(x: number, y: number, moving: PaneId): Drop | null {
  const el = document
    .elementsFromPoint(x, y)
    .map((e) => e.closest<HTMLElement>('.pane[data-pane]'))
    .find(Boolean)
  const target = Number(el?.dataset.pane)
  if (!el || target === moving) return null
  const r = el.getBoundingClientRect()
  const fx = (x - r.left) / r.width
  const fy = (y - r.top) / r.height
  const edges: Array<[number, Side]> = [
    [fx, 'left'],
    [1 - fx, 'right'],
    [fy, 'up'],
    [1 - fy, 'down']
  ]
  const [distance, side] = edges.reduce((near, edge) => (edge[0] < near[0] ? edge : near))
  if (distance > 0.25) return { target, side: 'center', box: { left: r.left, top: r.top, width: r.width, height: r.height } }
  const across = side === 'left' || side === 'right'
  return {
    target,
    side,
    box: {
      left: side === 'right' ? r.left + r.width / 2 : r.left,
      top: side === 'down' ? r.top + r.height / 2 : r.top,
      width: across ? r.width / 2 : r.width,
      height: across ? r.height : r.height / 2
    }
  }
}

function Grip({ pane }: { pane: PaneId }): React.JSX.Element {
  const [moving, setMoving] = useState(false)
  const [drop, setDrop] = useState<Drop | null>(null)
  const stop = (): void => {
    setMoving(false)
    setDrop(null)
  }

  useEffect(() => {
    if (!moving) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      stop()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [moving])

  return (
    <>
      <div
        className={`pane-grip${moving ? ' moving' : ''}`}
        title="Drag onto another pane: the middle swaps them, an edge places this pane beside it"
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          setMoving(true)
        }}
        onPointerMove={(e) => moving && setDrop(dropAt(e.clientX, e.clientY, pane))}
        onPointerCancel={stop}
        onPointerUp={() => {
          if (moving && drop) {
            void run(drop.side === 'center' ? { op: 'swapPanes', a: pane, b: drop.target } : { op: 'movePane', pane, target: drop.target, side: drop.side })
          }
          stop()
        }}
      />
      {drop && <div className="pane-drop" style={drop.box} />}
    </>
  )
}

function PaneView({ id, focused, solo }: { id: PaneId; focused: boolean; solo: boolean }): React.JSX.Element | null {
  const pane = useStore((s) => s.panes.get(id))
  const zoomed = useStore((s) => activeTab(s)?.zoomed ?? false)
  const address = useStore((s) => paneAddress(s, id))
  const hostRef = useRef<HTMLDivElement>(null)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)

  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) return
    terminals.mount(id, host)
    return () => terminals.unmount(id, host)
  }, [id])

  useEffect(() => {
    if (focused) terminals.focus(id)
  }, [focused, id, pane?.status])

  const status = pane?.status
  useEffect(() => {
    if (status === 'running') terminals.resync(id)
  }, [status, id])

  if (!pane) return null
  const close = (): void => {
    if (pane.status !== 'running' || window.confirm(`Close "${pane.name}"? This stops it.`)) {
      void run({ op: 'closePane', pane: id })
    }
  }
  const split = (dir: 'right' | 'down'): void => void run({ op: 'splitPane', target: id, dir, pane: { kind: 'shell' } })
  const zoom = (): void => void run({ op: 'toggleZoom', pane: id })
  const canZoom = !solo || zoomed

  return (
    <div
      className={`pane${focused ? ' focused' : ''}${solo ? ' solo' : ''}`}
      data-pane={id}
      onMouseDownCapture={() => {
        if (!focused) void run({ op: 'focusPane', pane: id })
      }}
    >
      <div
        className="pane-head"
        onContextMenu={(e) => {
          e.preventDefault()
          setMenu({ x: e.clientX, y: e.clientY })
        }}
      >
        {!solo && !zoomed && <Grip pane={id} />}
        {menu && (
          <ContextMenu at={menu} onClose={() => setMenu(null)}>
            <button onClick={() => split('right')}>
              <SplitRight /> Split right
            </button>
            <button onClick={() => split('down')}>
              <SplitDown /> Split down
            </button>
            {canZoom && (
              <button onClick={zoom}>
                <Zoom /> {zoomed ? 'Unzoom' : 'Zoom'}
              </button>
            )}
            <button className="danger" onClick={close}>
              <Close /> Close pane
            </button>
          </ContextMenu>
        )}
        <KindIcon kind={shownKind(pane)} />
        <span className="pane-kind">{KIND_LABEL[shownKind(pane)]}</span>
        <span className="pane-cwd">{pane.cwd}</span>
        {address && (
          <span className="pane-address" title={`Other panes reach this one with: nc send ${address} "…"`}>
            {address}
          </span>
        )}
        {pane.skipPermissions && <span className="badge" title="Runs with --dangerously-skip-permissions">skip permission</span>}
        {pane.status === 'exited' && !(pane.kind === 'cloud' && pane.cloudSessionId) ? (
          <button className="btn small-btn resume-btn" onClick={() => void run({ op: 'restartPane', pane: id })}>
            <Play /> {pane.kind === 'claude' ? 'Resume' : 'Start'}
          </button>
        ) : (
          <StateDot state={pane.agent} exited={pane.status === 'exited'} />
        )}
        <span className="pane-actions">
          <button className="icon-btn" title="Split right (Ctrl+Shift+O)" onClick={() => split('right')}>
            <SplitRight />
          </button>
          <button className="icon-btn" title="Split down (Ctrl+Shift+E)" onClick={() => split('down')}>
            <SplitDown />
          </button>
          {canZoom && (
            <button className={`icon-btn${zoomed ? ' on' : ''}`} title="Zoom (Ctrl+B, z)" onClick={zoom}>
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
      <div className="empty-bar" data-tauri-drag-region>
        <WindowControls />
      </div>
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
      <div className="work">
        <div className="tab-body" key={tab.id}>
          {tab.zoomed ? (
            <PaneView id={tab.focused} focused solo />
          ) : (
            <Tree node={tab.layout} path={[]} tab={tab} />
          )}
        </div>
        <SidePanel />
      </div>
      <StatusBar ws={ws} />
    </main>
  )
}
