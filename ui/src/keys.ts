import { send } from './api'
import { activeTab, activeWorkspace, getStore, run, update } from './store'
import type { PaneSpec, Side } from './types'

export const BINDINGS: Array<[string, string]> = [
  ['c', 'New Claude tab'],
  ['s', 'New shell tab'],
  ['v', 'Split right (shell)'],
  ['-', 'Split down (shell)'],
  ['V', 'Split right (Claude)'],
  ['_', 'Split down (Claude)'],
  ['h j k l / arrows', 'Move between panes'],
  ['H J K L', 'Resize the focused pane'],
  ['z', 'Zoom the focused pane'],
  ['x', 'Close pane'],
  ['X', 'Close tab'],
  ['n / p', 'Next / previous tab'],
  ['1–9', 'Jump to tab'],
  ['w', 'Next workspace'],
  ['N', 'New workspace'],
  ['b', 'Toggle sidebar'],
  ['f', 'Files panel'],
  ['o', 'Ports panel'],
  ['Ctrl+B', 'Send Ctrl+B to the pane'],
  ['?', 'This help']
]

const SIDES: Record<string, Side> = {
  h: 'left',
  j: 'down',
  k: 'up',
  l: 'right',
  arrowleft: 'left',
  arrowdown: 'down',
  arrowup: 'up',
  arrowright: 'right'
}

let prefixTimer: number | undefined

function arm(on: boolean): void {
  window.clearTimeout(prefixTimer)
  update({ prefix: on })
  if (on) prefixTimer = window.setTimeout(() => update({ prefix: false }), 3000)
}

const claude: PaneSpec = { kind: 'claude' }
const shell: PaneSpec = { kind: 'shell' }

export function handleKey(ev: KeyboardEvent): boolean {
  const s = getStore()
  if (s.dialog) return false
  const key = ev.key

  if (!s.prefix) {
    if (ev.ctrlKey && !ev.shiftKey && !ev.altKey && key.toLowerCase() === 'b') {
      arm(true)
      return true
    }
    return false
  }

  if (key === 'Control' || key === 'Shift' || key === 'Alt' || key === 'Meta') return true
  arm(false)

  const ws = activeWorkspace(s)
  const tab = activeTab(s)
  const pane = tab?.focused
  const lower = key.toLowerCase()

  if (ev.ctrlKey && lower === 'b') {
    if (pane !== undefined) send({ op: 'write', pane, data: '\x02' })
    return true
  }
  if (key === 'Escape') return true
  if (key === '?') return update({ dialog: { type: 'help' } }), true
  if (key === 'N') return update({ dialog: { type: 'workspace' } }), true
  if (key === 'b') return update({ sidebarOpen: !s.sidebarOpen }), true
  if (key === 'f') return update({ panel: s.panel === 'files' ? null : 'files' }), true
  if (key === 'o') return update({ panel: s.panel === 'ports' ? null : 'ports' }), true
  if (!ws || !tab || pane === undefined) return true

  if (key === 'c') void run({ op: 'createTab', workspace: ws.id, pane: claude })
  else if (key === 's') void run({ op: 'createTab', workspace: ws.id, pane: shell })
  else if (key === 'v') void run({ op: 'splitPane', target: pane, dir: 'right', pane: shell })
  else if (key === '-') void run({ op: 'splitPane', target: pane, dir: 'down', pane: shell })
  else if (key === 'V') void run({ op: 'splitPane', target: pane, dir: 'right', pane: claude })
  else if (key === '_') void run({ op: 'splitPane', target: pane, dir: 'down', pane: claude })
  else if (key === 'z') void run({ op: 'toggleZoom', pane })
  else if (key === 'x') void run({ op: 'closePane', pane })
  else if (key === 'X') void run({ op: 'closeTab', tab: tab.id })
  else if (key === 'n' || key === 'p') {
    const i = ws.tabs.findIndex((t) => t.id === tab.id)
    const next = ws.tabs[(i + (key === 'n' ? 1 : ws.tabs.length - 1)) % ws.tabs.length]
    if (next) void run({ op: 'selectTab', tab: next.id })
  } else if (/^[1-9]$/.test(key)) {
    const target = ws.tabs[Number(key) - 1]
    if (target) void run({ op: 'selectTab', tab: target.id })
  } else if (key === 'w') {
    const all = s.state.workspaces
    const next = all[(all.findIndex((w) => w.id === ws.id) + 1) % all.length]
    if (next) void run({ op: 'selectWorkspace', workspace: next.id })
  } else if (SIDES[lower]) {
    const side = SIDES[lower]
    if (ev.shiftKey) void run({ op: 'resizeSplit', pane, side, delta: 0.05 })
    else void run({ op: 'focusSide', pane, side })
  }
  return true
}

export function installGlobalKeys(): void {
  window.addEventListener('keydown', (ev) => {
    const target = ev.target as HTMLElement | null
    if (target?.closest('.xterm')) return
    if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return
    if (handleKey(ev)) ev.preventDefault()
  })
}
