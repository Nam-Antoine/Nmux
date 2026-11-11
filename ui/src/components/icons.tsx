import type { AgentState, Pane, PaneKind } from '../types'

const base = { width: 16, height: 16, viewBox: '0 0 16 16', fill: 'none', stroke: 'currentColor', strokeWidth: 1.4, strokeLinecap: 'round', strokeLinejoin: 'round' } as const

export const Plus = () => (
  <svg {...base}><path d="M8 3.5v9M3.5 8h9" /></svg>
)
export const Check = () => (
  <svg {...base}><path d="M3.5 8.5l3 3 6-7" /></svg>
)
export const Close = () => (
  <svg {...base}><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
)
export const SidebarIcon = () => (
  <svg {...base}><rect x="2.5" y="3" width="11" height="10" rx="2" /><path d="M6.5 3v10" /></svg>
)
export const SplitRight = () => (
  <svg {...base}><rect x="2.5" y="3" width="11" height="10" rx="2" /><path d="M8 3v10" /></svg>
)
export const SplitDown = () => (
  <svg {...base}><rect x="2.5" y="3" width="11" height="10" rx="2" /><path d="M2.5 8h11" /></svg>
)
export const Zoom = () => (
  <svg {...base}><path d="M9.5 3h3.5v3.5M6.5 13H3V9.5M13 3L9 7M3 13l4-4" /></svg>
)
export const Chevron = ({ open }: { open: boolean }) => (
  <svg {...base} style={{ transform: open ? 'rotate(90deg)' : undefined, transition: 'transform var(--t-med) var(--ease-out)' }}><path d="M6 4l4 4-4 4" /></svg>
)
export const Up = () => (
  <svg {...base}><path d="M8 12.5v-9M4.5 7L8 3.5 11.5 7" /></svg>
)
export const Refresh = () => (
  <svg {...base}><path d="M13 8a5 5 0 11-1.6-3.7M13 3v2.6h-2.6" /></svg>
)
export const FilesIcon = () => (
  <svg {...base}><path d="M4 2.5h5l3 3V13a.5.5 0 01-.5.5h-7A.5.5 0 014 13zM9 2.5v3h3" /></svg>
)
export const PortsIcon = () => (
  <svg {...base}><circle cx="8" cy="8" r="5.5" /><path d="M2.5 8h11M8 2.5c1.8 1.6 1.8 9.4 0 11M8 2.5c-1.8 1.6-1.8 9.4 0 11" /></svg>
)
export const BranchIcon = () => (
  <svg {...base}><circle cx="4.5" cy="3.8" r="1.5" /><circle cx="4.5" cy="12.2" r="1.5" /><circle cx="11.5" cy="5.8" r="1.5" /><path d="M4.5 5.3v5.4M11.5 7.3c0 2.4-3.2 2.2-7 3.4" /></svg>
)
export const Pencil = () => (
  <svg {...base}><path d="M10.8 2.8l2.4 2.4L6 12.4l-3 .6.6-3z" /></svg>
)
export const FolderIcon = () => (
  <svg {...base}><path d="M2.5 5a1.5 1.5 0 011.5-1.5h2.2l1.3 1.5H12A1.5 1.5 0 0113.5 6.5v4.5A1.5 1.5 0 0112 12.5H4A1.5 1.5 0 012.5 11z" /></svg>
)

export const Play = () => (
  <svg {...base} fill="currentColor" stroke="none"><path d="M5.2 3.4v9.2a.5.5 0 00.77.42l7-4.6a.5.5 0 000-.84l-7-4.6a.5.5 0 00-.77.42z" /></svg>
)
export const Search = () => (
  <svg {...base}><circle cx="7.2" cy="7.2" r="4.2" /><path d="M10.4 10.4l3 3" /></svg>
)
export const Clock = () => (
  <svg {...base}><circle cx="8" cy="8" r="5.6" /><path d="M8 4.8V8l2.2 1.4" /></svg>
)
export const Monitor = () => (
  <svg {...base}><rect x="2" y="3" width="12" height="8" rx="1.5" /><path d="M6 13.5h4M8 11v2.5" /></svg>
)
export const ServerIcon = () => (
  <svg {...base}><rect x="2.5" y="3" width="11" height="4.2" rx="1.2" /><rect x="2.5" y="8.8" width="11" height="4.2" rx="1.2" /></svg>
)
export const Keyboard = () => (
  <svg {...base}><rect x="1.8" y="4" width="12.4" height="8" rx="1.6" /><path d="M4.5 6.8h.01M7 6.8h.01M9.5 6.8h.01M11.6 6.8h.01M5 9.5h6" /></svg>
)
export const WinMin = () => (
  <svg {...base} strokeWidth={1.1}><path d="M3 8.5h10" /></svg>
)
export const WinMax = () => (
  <svg {...base} strokeWidth={1.1}><rect x="3" y="5" width="8" height="8" rx="1" /><path d="M5.5 5V3.6a.6.6 0 01.6-.6h6.3a.6.6 0 01.6.6v6.3a.6.6 0 01-.6.6H11" /></svg>
)
export const WinClose = () => (
  <svg {...base} strokeWidth={1.1}><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" /></svg>
)

export function KindIcon({ kind }: { kind: PaneKind }): React.JSX.Element {
  if (kind === 'shell') {
    return (
      <svg {...base} className="kind-icon shell"><rect x="2.2" y="2.7" width="11.6" height="10.6" rx="1.8" /><path d="M5 6.2l1.8 1.6L5 9.4M8.4 10h2.4" /></svg>
    )
  }
  if (kind === 'cloud') {
    return (
      <svg {...base} className="kind-icon cloud"><path d="M4.8 12a2.6 2.6 0 01-.3-5.2 3.6 3.6 0 017 .8A2.2 2.2 0 0111.4 12z" /></svg>
    )
  }
  return (
    <svg width="16" height="13" viewBox="0 0 20 16" className="kind-icon claude" fill="currentColor" shapeRendering="crispEdges">
      <path d="M3 3h14v8H3z" />
      <path d="M0 6h3v3H0zM17 6h3v3h-3z" />
      <path d="M4 11h2v3H4zM7 11h2v3H7zM11 11h2v3h-2zM14 11h2v3h-2z" />
      <path d="M6 5h1.5v2.5H6zM12.5 5H14v2.5h-1.5z" fill="var(--side)" />
    </svg>
  )
}

export const shownKind = (pane: Pane): PaneKind => (pane.runsClaude ? 'claude' : pane.kind)

export const KIND_LABEL: Record<PaneKind, string> = { claude: 'Claude Code', shell: 'Shell', cloud: 'Cloud' }

const STATE_TITLE: Record<AgentState, string> = {
  blocked: 'Needs your answer',
  working: 'Working',
  done: 'Finished, not seen yet',
  idle: 'Idle',
  unknown: ''
}

export function StateDot({ state, exited }: { state: AgentState; exited?: boolean }): React.JSX.Element {
  const cls = exited ? 'exited' : state
  return <span className={`dot ${cls}`} title={exited ? 'Not running' : STATE_TITLE[state]} />
}

const AVATAR_COLORS = ['#5b8def', '#8b6cf0', '#3fb37f', '#e0823d', '#d9577a', '#2fa8b5', '#c9a227']

export function Avatar({ name }: { name: string }): React.JSX.Element {
  let hash = 0
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0
  const letter = (name.trim()[0] ?? '?').toUpperCase()
  return (
    <span className="avatar" style={{ background: AVATAR_COLORS[hash % AVATAR_COLORS.length] }}>
      {letter}
    </span>
  )
}
