import type { AgentState, Pane, PaneKind } from '../types'

const base = { width: 16, height: 16, viewBox: '0 0 16 16', fill: 'none', stroke: 'currentColor', strokeWidth: 1.4, strokeLinecap: 'round', strokeLinejoin: 'round' } as const

export const Plus = () => (
  <svg {...base}><path d="M8 3.5v9M3.5 8h9" /></svg>
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
  <svg {...base} style={{ transform: open ? 'rotate(90deg)' : undefined, transition: 'transform .12s' }}><path d="M6 4l4 4-4 4" /></svg>
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
export const FolderIcon = () => (
  <svg {...base}><path d="M2.5 5a1.5 1.5 0 011.5-1.5h2.2l1.3 1.5H12A1.5 1.5 0 0113.5 6.5v4.5A1.5 1.5 0 0112 12.5H4A1.5 1.5 0 012.5 11z" /></svg>
)

/** What kind of thing runs in a pane. */
export function KindIcon({ kind }: { kind: PaneKind }): React.JSX.Element {
  if (kind === 'shell') {
    return (
      <svg {...base} className="kind-icon shell"><path d="M3.5 5l3 3-3 3M8.5 11h4" /></svg>
    )
  }
  if (kind === 'cloud') {
    return (
      <svg {...base} className="kind-icon cloud"><path d="M4.8 12a2.6 2.6 0 01-.3-5.2 3.6 3.6 0 017 .8A2.2 2.2 0 0111.4 12z" /></svg>
    )
  }
  return (
    <svg {...base} className="kind-icon claude" strokeWidth={1.7}>
      <path d="M8 2.5v11M2.5 8h11M4.1 4.1l7.8 7.8M11.9 4.1l-7.8 7.8" />
    </svg>
  )
}

export const KIND_LABEL: Record<PaneKind, string> = { claude: 'Claude Code', shell: 'Shell', cloud: 'Cloud' }

const STATE_TITLE: Record<AgentState, string> = {
  blocked: 'Needs your answer',
  working: 'Working',
  done: 'Finished, not seen yet',
  idle: 'Idle',
  unknown: ''
}

/** The dot that says what an agent is doing. */
export function StateDot({ state, exited }: { state: AgentState; exited?: boolean }): React.JSX.Element {
  const cls = exited ? 'exited' : state
  return <span className={`dot ${cls}`} title={exited ? 'Not running' : STATE_TITLE[state]} />
}

export function paneState(pane: Pane | undefined): { state: AgentState; exited: boolean } {
  return { state: pane?.agent ?? 'unknown', exited: pane?.status === 'exited' }
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
