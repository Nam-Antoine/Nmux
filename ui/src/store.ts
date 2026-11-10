import { useSyncExternalStore } from 'react'
import { call, notify, onConnected, onDisconnected, onFileDrop, onServerEvent } from './api'
import { terminals } from './terminals'
import type { AgentState, Forward, Pane, PaneId, PaneKind, State, Tab, Workspace } from './types'

export type Dialog =
  | { type: 'workspace'; machine?: number }
  | { type: 'machine' }
  | { type: 'tab'; workspace: number; kind: PaneKind }
  | { type: 'help' }
  | null

export type Panel = 'files' | 'ports' | 'changes'

export function togglePanel(which: Panel): void {
  update({ panel: store.panel === which ? null : which })
}

export interface Store {
  state: State
  panes: Map<PaneId, Pane>
  connected: boolean
  problem: string | null
  sidebarOpen: boolean
  dialog: Dialog
  panel: Panel | null
  forwards: Forward[]
  prefix: boolean
  toast: string | null
}

let store: Store = {
  state: { workspaces: [], panes: [], machines: [] },
  panes: new Map(),
  connected: false,
  problem: null,
  sidebarOpen: true,
  dialog: null,
  panel: null,
  forwards: [],
  prefix: false,
  toast: null
}

const listeners = new Set<() => void>()

export function getStore(): Store {
  return store
}

export function update(patch: Partial<Store>): void {
  store = { ...store, ...patch }
  for (const l of listeners) l()
}

export function useStore<T>(select: (s: Store) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => select(store)
  )
}

function setState(state: State): void {
  state.machines ??= []
  const panes = new Map(state.panes.map((p) => [p.id, p]))
  terminals.retain(panes)
  update({ state, panes })
}

let toastTimer: number | undefined
export function toast(message: string): void {
  window.clearTimeout(toastTimer)
  update({ toast: message })
  toastTimer = window.setTimeout(() => update({ toast: null }), 5000)
}

export async function run(op: Parameters<typeof call>[0]): Promise<Awaited<ReturnType<typeof call>> | null> {
  try {
    return await call(op)
  } catch (err) {
    toast(String(err))
    return null
  }
}

function checkMachines(): void {
  for (const machine of store.state.machines) {
    if (machine.status !== 'checking') void call({ op: 'checkMachine', machine: machine.id }).catch(() => {})
  }
}

async function refresh(): Promise<void> {
  try {
    const reply = await call({ op: 'getState' })
    if (reply.kind === 'state') setState(reply.state)
    update({ connected: true, problem: null })
    terminals.reattachAll()
    const forwards = await call({ op: 'listForwards' })
    if (forwards.kind === 'forwards') update({ forwards: forwards.forwards })
    void call({ op: 'setWindowVisible', visible: !document.hidden })
    checkMachines()
  } catch (err) {
    update({ connected: false, problem: String(err) })
  }
}

export async function start(): Promise<void> {
  await onServerEvent((event) => {
    if (event.event === 'state') setState(event.state)
    else if (event.event === 'output') terminals.output(event.pane, event.seq, event.data)
    else if (event.event === 'forwards') update({ forwards: event.forwards })
    else if (event.event === 'attention') {
      const pane = store.panes.get(event.pane)
      if (pane && (document.hidden || !document.hasFocus())) {
        void notify(event.blocked ? 'Needs your answer' : 'Finished', pane.name)
      }
    }
  })
  await onConnected(() => void refresh())
  await onFileDrop((paths, x, y) => {
    const under = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>('[data-pane]')
    const pane = under ? Number(under.dataset.pane) : activeTab(store)?.focused
    if (pane === undefined || paths.length === 0) return
    terminals.paste(pane, paths.map((p) => (/\s/.test(p) ? `"${p}"` : p)).join(' ') + ' ')
    terminals.focus(pane)
  })
  await onDisconnected((why) => update({ connected: false, problem: why }))
  document.addEventListener('visibilitychange', () => {
    if (store.connected) void call({ op: 'setWindowVisible', visible: !document.hidden }).catch(() => {})
  })
  window.setInterval(() => {
    if (store.connected && !document.hidden) checkMachines()
  }, 60_000)
  await refresh()
}

export function activeWorkspace(s: Store): Workspace | undefined {
  return s.state.workspaces.find((w) => w.id === s.state.activeWorkspace) ?? s.state.workspaces[0]
}

export function activeTab(s: Store): Tab | undefined {
  const ws = activeWorkspace(s)
  return ws?.tabs.find((t) => t.id === ws.activeTab) ?? ws?.tabs[0]
}

export function leaves(node: Tab['layout']): PaneId[] {
  return node.type === 'leaf' ? [node.pane] : [...leaves(node.a), ...leaves(node.b)]
}

const RANK: Record<AgentState, number> = { blocked: 4, working: 3, done: 2, idle: 1, unknown: 0 }

export function rollup(s: Store, ids: PaneId[]): AgentState {
  let best: AgentState = 'unknown'
  for (const id of ids) {
    const agent = s.panes.get(id)?.agent ?? 'unknown'
    if (RANK[agent] > RANK[best]) best = agent
  }
  return best
}

export function workspacePanes(ws: Workspace): PaneId[] {
  return ws.tabs.flatMap((t) => leaves(t.layout))
}

export function paneAddress(s: Store, pane: PaneId): string | undefined {
  for (const ws of s.state.workspaces) {
    const panes = workspacePanes(ws)
    const index = panes.indexOf(pane)
    if (index >= 0) return panes.length === 1 ? `n${ws.number}` : `n${ws.number}.${index + 1}`
  }
  return undefined
}

export function tabLabel(s: Store, tab: Tab, index: number): string {
  return tab.name ?? s.panes.get(tab.focused)?.name ?? String(index + 1)
}
