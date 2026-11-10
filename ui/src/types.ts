// Mirrors of the Rust types in `nmux-core` and `nmux-proto`, as they appear on the wire.

export type PaneId = number
export type TabId = number
export type WorkspaceId = number
export type MachineId = number

export type PaneKind = 'claude' | 'shell' | 'cloud'
export type PaneStatus = 'running' | 'exited'
export type AgentState = 'blocked' | 'working' | 'done' | 'idle' | 'unknown'
export type Dir = 'right' | 'down'
export type Side = 'left' | 'right' | 'up' | 'down'

export type LayoutNode =
  | { type: 'leaf'; pane: PaneId }
  | { type: 'split'; dir: Dir; ratio: number; a: LayoutNode; b: LayoutNode }

export type MachineStatus = 'unknown' | 'checking' | 'online' | 'auth' | 'error'

/** Another computer, reached with the user's own ssh. */
export interface Machine {
  id: MachineId
  label: string
  target: string
  status: MachineStatus
  detail?: string
  tmux: boolean
  claude: boolean
}

export interface Pane {
  id: PaneId
  kind: PaneKind
  name: string
  cwd: string
  status: PaneStatus
  exitCode?: number
  agent: AgentState
  claudeSessionId?: string
  skipPermissions: boolean
  extraArgs?: string[]
  cloudTask?: string
  cloudSessionId?: string
  cloudUrl?: string
  teleportedFrom?: string
  machine?: MachineId
  createdAt: number
}

export interface Tab {
  id: TabId
  name?: string
  layout: LayoutNode
  focused: PaneId
  zoomed: boolean
}

export interface Workspace {
  id: WorkspaceId
  name: string
  cwd: string
  tabs: Tab[]
  activeTab: TabId
  machine?: MachineId
}

export interface State {
  workspaces: Workspace[]
  panes: Pane[]
  activeWorkspace?: WorkspaceId
  machines: Machine[]
}

export interface PaneSpec {
  kind: PaneKind
  name?: string
  cwd?: string
  skipPermissions?: boolean
  extraArgs?: string[]
  cloudTask?: string
}

export type Op =
  | { op: 'getState' }
  | { op: 'createWorkspace'; name?: string; cwd: string; pane: PaneSpec; machine?: MachineId }
  | { op: 'addMachine'; label?: string; target: string }
  | { op: 'renameMachine'; machine: MachineId; label: string }
  | { op: 'removeMachine'; machine: MachineId }
  | { op: 'checkMachine'; machine: MachineId }
  | { op: 'createTab'; workspace: WorkspaceId; name?: string; pane: PaneSpec }
  | { op: 'splitPane'; target: PaneId; dir: Dir; pane: PaneSpec }
  | { op: 'closePane'; pane: PaneId }
  | { op: 'closeTab'; tab: TabId }
  | { op: 'closeWorkspace'; workspace: WorkspaceId }
  | { op: 'focusPane'; pane: PaneId }
  | { op: 'selectTab'; tab: TabId }
  | { op: 'selectWorkspace'; workspace: WorkspaceId }
  | { op: 'focusSide'; pane: PaneId; side: Side }
  | { op: 'swapPanes'; a: PaneId; b: PaneId }
  | { op: 'setRatio'; tab: TabId; path: number[]; ratio: number }
  | { op: 'resizeSplit'; pane: PaneId; side: Side; delta: number }
  | { op: 'toggleZoom'; pane: PaneId }
  | { op: 'renamePane'; pane: PaneId; name: string }
  | { op: 'renameTab'; tab: TabId; name?: string }
  | { op: 'renameWorkspace'; workspace: WorkspaceId; name: string }
  | { op: 'restartPane'; pane: PaneId }
  | { op: 'teleportPane'; pane: PaneId }
  | { op: 'attach'; pane: PaneId }
  | { op: 'write'; pane: PaneId; data: string }
  | { op: 'resize'; pane: PaneId; cols: number; rows: number }
  | { op: 'setWindowVisible'; visible: boolean }
  | { op: 'listDir'; path: string }
  | { op: 'readFile'; path: string }
  | { op: 'pasteImage'; pane: PaneId; extension: string; data: string }
  | { op: 'listPorts' }
  | { op: 'listForwards' }
  | { op: 'startForward'; host: string; remotePort: number; localPort?: number }
  | { op: 'stopForward'; id: number }
  | { op: 'removeForward'; id: number }

export interface DirEntry {
  name: string
  dir: boolean
  size: number
}

export type FileContent =
  | { type: 'text'; text: string; truncated: boolean }
  | { type: 'image'; mime: string; data: string }
  | { type: 'binary' }
  | { type: 'tooLarge' }

export interface FilePreview {
  path: string
  size: number
  content: FileContent
}

export interface ListeningPort {
  port: number
  address: string
  pid: number
  process: string
  pane?: PaneId
}

export interface Forward {
  id: number
  host: string
  remotePort: number
  localPort: number
  state: 'starting' | 'active' | 'stopped' | 'failed'
  error?: string
}

export interface Snapshot {
  data: string
  seq: number
  cols: number
  rows: number
}

export type Reply =
  | { kind: 'done' }
  | { kind: 'state'; state: State }
  | { kind: 'created'; created: { workspace: WorkspaceId; tab: TabId; pane: PaneId } }
  | { kind: 'closed'; closed: { panes: PaneId[]; tab?: TabId; workspace?: WorkspaceId } }
  | { kind: 'attached'; snapshot: Snapshot }
  | { kind: 'flag'; value: boolean }
  | { kind: 'pane'; pane?: PaneId }
  | { kind: 'dir'; path: string; entries: DirEntry[] }
  | { kind: 'file'; file: FilePreview }
  | { kind: 'path'; path: string }
  | { kind: 'ports'; ports: ListeningPort[] }
  | { kind: 'forwards'; forwards: Forward[] }

export type ServerEvent =
  | { event: 'state'; state: State }
  | { event: 'output'; pane: PaneId; seq: number; data: string }
  | { event: 'attention'; pane: PaneId; blocked: boolean }
  | { event: 'forwards'; forwards: Forward[] }
