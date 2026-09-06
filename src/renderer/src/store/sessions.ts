/**
 * Renderer-side session state. The main process is the source of truth;
 * this store mirrors it and adds UI-only state: the grid of panes, which
 * session each pane shows, the dialog, and collapsed folders.
 */
import { create } from 'zustand'
import type { CreateSessionRequest, SessionMeta } from '@shared/session'
import { api } from '../api'
import { folderKey, visualOrder } from '../lib/folders'

const COLLAPSED_KEY = 'nmux.collapsedFolders'
const LAYOUT_KEY = 'nmux.layout'

/** Largest grid on either axis (4×4 = 16 panes). */
export const MAX_GRID = 4

export interface GridLayout {
  rows: number
  cols: number
}

/** Values the new-session dialog is opened with (e.g. from a folder's + button). */
export interface DialogPreset {
  cwd?: string
}

interface SessionsState {
  sessions: SessionMeta[]
  /** Session shown in the focused pane: what the status bar and shortcuts act on. */
  activeId: string | null
  layout: GridLayout
  /** Session id per pane, row-major, always `rows * cols` long. A session shows in at most one pane. */
  cells: (string | null)[]
  focusedCell: number
  dialogOpen: boolean
  dialogPreset: DialogPreset | null
  /** Folder keys (see lib/folders) whose group is collapsed in the sidebar. */
  collapsedFolders: string[]

  init(): () => void
  /** Show a session: focus the pane it is in, or put it in the focused pane. */
  select(id: string): void
  /** Cycle the focused pane through the sessions not shown elsewhere. */
  selectRelative(delta: number): void
  setLayout(rows: number, cols: number): void
  focusCell(index: number): void
  focusCellRelative(delta: number): void
  /** Put a session (or nothing) in a pane, removing it from any other pane. */
  assignCell(index: number, id: string | null): void
  openDialog(preset?: DialogPreset): void
  closeDialog(): void
  toggleFolder(key: string): void
  create(req: CreateSessionRequest): Promise<void>
  restart(id: string): Promise<void>
  kill(id: string): Promise<void>
  remove(id: string): Promise<void>
  rename(id: string, name: string): Promise<void>
}

// ---- persistence (best effort: all of this is UI convenience) ---------------

function readJson(key: string): unknown {
  try {
    return JSON.parse(localStorage.getItem(key) ?? 'null')
  } catch {
    return null
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* storage unavailable */
  }
}

function loadCollapsed(): string[] {
  const parsed = readJson(COLLAPSED_KEY)
  return Array.isArray(parsed) ? parsed.filter((k): k is string => typeof k === 'string') : []
}

const clampAxis = (n: unknown): number =>
  typeof n === 'number' && Number.isInteger(n) ? Math.min(MAX_GRID, Math.max(1, n)) : 1

function loadLayout(): Pick<SessionsState, 'layout' | 'cells' | 'focusedCell'> {
  const parsed = readJson(LAYOUT_KEY) as Partial<{ rows: unknown; cols: unknown; cells: unknown; focusedCell: unknown }> | null
  const layout = { rows: clampAxis(parsed?.rows), cols: clampAxis(parsed?.cols) }
  const n = layout.rows * layout.cols
  const raw = Array.isArray(parsed?.cells) ? parsed.cells : []
  const cells: (string | null)[] = Array.from({ length: n }, (_, i) => (typeof raw[i] === 'string' ? (raw[i] as string) : null))
  const focusedCell = Math.min(n - 1, Math.max(0, clampAxis(parsed?.focusedCell) - 1))
  return { layout, cells, focusedCell }
}

function saveLayout(state: Pick<SessionsState, 'layout' | 'cells' | 'focusedCell'>): void {
  writeJson(LAYOUT_KEY, { ...state.layout, cells: state.cells, focusedCell: state.focusedCell + 1 })
}

// ---- store ---------------------------------------------------------------------

export const useSessions = create<SessionsState>((set, get) => {
  /** Apply a grid change: keep activeId in sync and persist. */
  const grid = (
    next: Partial<Pick<SessionsState, 'layout' | 'cells' | 'focusedCell'>>
  ): Pick<SessionsState, 'layout' | 'cells' | 'focusedCell' | 'activeId'> => {
    const state = get()
    const merged = {
      layout: next.layout ?? state.layout,
      cells: next.cells ?? state.cells,
      focusedCell: next.focusedCell ?? state.focusedCell
    }
    saveLayout(merged)
    return { ...merged, activeId: merged.cells[merged.focusedCell] ?? null }
  }

  const initial = loadLayout()

  return {
    sessions: [],
    activeId: initial.cells[initial.focusedCell] ?? null,
    layout: initial.layout,
    cells: initial.cells,
    focusedCell: initial.focusedCell,
    dialogOpen: false,
    dialogPreset: null,
    collapsedFolders: loadCollapsed(),

    init() {
      const apply = (sessions: SessionMeta[]): void => {
        const ids = new Set(sessions.map((s) => s.id))
        const cells = get().cells.map((id) => (id && ids.has(id) ? id : null))
        set({ sessions, ...grid({ cells }) })
      }
      const unsubscribe = api.on('sessionsChanged', apply)
      void api.sessions.list().then(apply)
      return unsubscribe
    },

    select(id) {
      const state = get()
      // Selecting a session (keyboard, create, restart) reveals its folder.
      const session = state.sessions.find((s) => s.id === id)
      const key = session ? folderKey(session.cwd) : null
      if (key && state.collapsedFolders.includes(key)) {
        const collapsedFolders = state.collapsedFolders.filter((k) => k !== key)
        writeJson(COLLAPSED_KEY, collapsedFolders)
        set({ collapsedFolders })
      }
      const shownAt = state.cells.indexOf(id)
      if (shownAt !== -1) {
        set(grid({ focusedCell: shownAt }))
      } else {
        const cells = state.cells.slice()
        cells[state.focusedCell] = id
        set(grid({ cells }))
      }
    },

    selectRelative(delta) {
      const { sessions, cells, focusedCell } = get()
      const current = cells[focusedCell]
      const candidates = visualOrder(sessions).filter((s) => s.id === current || !cells.includes(s.id))
      if (candidates.length === 0) return
      const index = candidates.findIndex((s) => s.id === current)
      const next = candidates[(index + delta + candidates.length) % candidates.length]!
      get().select(next.id)
    },

    setLayout(rows, cols) {
      const state = get()
      const layout = { rows: clampAxis(rows), cols: clampAxis(cols) }
      const n = layout.rows * layout.cols
      // Keep the visible sessions, packed from the first pane, so shrinking
      // the grid never silently drops the ones at the end.
      const shown = state.cells.filter((id): id is string => id !== null)
      const cells: (string | null)[] = Array.from({ length: n }, (_, i) => shown[i] ?? null)
      const active = state.cells[state.focusedCell]
      const focusedCell = active ? Math.max(0, cells.indexOf(active)) : Math.min(state.focusedCell, n - 1)
      set(grid({ layout, cells, focusedCell }))
    },

    focusCell(index) {
      const { cells } = get()
      if (index < 0 || index >= cells.length) return
      set(grid({ focusedCell: index }))
    },

    focusCellRelative(delta) {
      const { cells, focusedCell } = get()
      get().focusCell((focusedCell + delta + cells.length) % cells.length)
    },

    assignCell(index, id) {
      const state = get()
      if (index < 0 || index >= state.cells.length) return
      const cells = state.cells.map((c) => (id !== null && c === id ? null : c))
      cells[index] = id
      set(grid({ cells, focusedCell: index }))
    },

    openDialog: (preset) => set({ dialogOpen: true, dialogPreset: preset ?? null }),
    closeDialog: () => set({ dialogOpen: false, dialogPreset: null }),

    toggleFolder(key) {
      const collapsedFolders = get().collapsedFolders.includes(key)
        ? get().collapsedFolders.filter((k) => k !== key)
        : [...get().collapsedFolders, key]
      writeJson(COLLAPSED_KEY, collapsedFolders)
      set({ collapsedFolders })
    },

    async create(req) {
      const meta = await api.sessions.create(req)
      // The changed-event with the new session may still be in flight; put it
      // in the list now so select() can find its folder.
      set((state) => (state.sessions.some((s) => s.id === meta.id) ? {} : { sessions: [meta, ...state.sessions] }))
      get().select(meta.id)
      set({ dialogOpen: false, dialogPreset: null })
    },

    async restart(id) {
      await api.sessions.restart(id)
      get().select(id)
    },

    kill: (id) => api.sessions.kill(id),

    async remove(id) {
      await api.sessions.remove(id)
      set(grid({ cells: get().cells.map((c) => (c === id ? null : c)) }))
    },

    rename: (id, name) => api.sessions.rename(id, name)
  }
})
