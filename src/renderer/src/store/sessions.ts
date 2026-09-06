/**
 * Renderer-side session state. The main process is the source of truth;
 * this store mirrors it and adds UI-only state (selection, dialog, mounts).
 */
import { create } from 'zustand'
import type { CreateSessionRequest, SessionMeta } from '@shared/session'
import { api } from '../api'

interface SessionsState {
  sessions: SessionMeta[]
  activeId: string | null
  /** Sessions whose terminal has been opened at least once. They stay mounted
   *  (hidden) when not active so switching back is instant. */
  mountedIds: string[]
  dialogOpen: boolean

  init(): () => void
  select(id: string): void
  selectRelative(delta: number): void
  openDialog(): void
  closeDialog(): void
  create(req: CreateSessionRequest): Promise<void>
  restart(id: string): Promise<void>
  kill(id: string): Promise<void>
  remove(id: string): Promise<void>
  rename(id: string, name: string): Promise<void>
}

export const useSessions = create<SessionsState>((set, get) => ({
  sessions: [],
  activeId: null,
  mountedIds: [],
  dialogOpen: false,

  init() {
    const apply = (sessions: SessionMeta[]): void => {
      const ids = new Set(sessions.map((s) => s.id))
      set((state) => ({
        sessions,
        activeId: state.activeId && ids.has(state.activeId) ? state.activeId : null,
        mountedIds: state.mountedIds.filter((id) => ids.has(id))
      }))
    }
    const unsubscribe = api.on('sessionsChanged', apply)
    void api.sessions.list().then(apply)
    return unsubscribe
  },

  select(id) {
    set((state) => ({
      activeId: id,
      mountedIds: state.mountedIds.includes(id) ? state.mountedIds : [...state.mountedIds, id]
    }))
  },

  selectRelative(delta) {
    const { sessions, activeId } = get()
    if (sessions.length === 0) return
    const index = sessions.findIndex((s) => s.id === activeId)
    const next = (index + delta + sessions.length) % sessions.length
    get().select(sessions[next]!.id)
  },

  openDialog: () => set({ dialogOpen: true }),
  closeDialog: () => set({ dialogOpen: false }),

  async create(req) {
    const meta = await api.sessions.create(req)
    get().select(meta.id)
    set({ dialogOpen: false })
  },

  async restart(id) {
    await api.sessions.restart(id)
    get().select(id)
  },

  kill: (id) => api.sessions.kill(id),

  async remove(id) {
    await api.sessions.remove(id)
    set((state) => ({
      activeId: state.activeId === id ? null : state.activeId,
      mountedIds: state.mountedIds.filter((m) => m !== id)
    }))
  },

  rename: (id, name) => api.sessions.rename(id, name)
}))
