/**
 * The single source of truth for the main <-> renderer contract.
 *
 * - `IPC` lists every channel name.
 * - `NmuxApi` is the typed surface the preload exposes as `window.nmux`.
 * - `NmuxEvents` are push events from main to renderer.
 */
import type { AppInfo, AttachResult, CreateSessionRequest, SessionMeta } from './session'

export const IPC = {
  // request/response (ipcRenderer.invoke)
  sessionsList: 'sessions:list',
  sessionsCreate: 'sessions:create',
  sessionsRestart: 'sessions:restart',
  sessionsKill: 'sessions:kill',
  sessionsRemove: 'sessions:remove',
  sessionsRename: 'sessions:rename',
  sessionsAttach: 'sessions:attach',
  dialogPickFolder: 'dialog:pickFolder',
  appInfo: 'app:info',

  // fire-and-forget (ipcRenderer.send) - hot path, no round trip
  sessionsWrite: 'sessions:write',
  sessionsResize: 'sessions:resize',

  // push events (webContents.send)
  evData: 'ev:data',
  evSessionsChanged: 'ev:sessionsChanged'
} as const

export interface NmuxEvents {
  /** Raw PTY output. `seq` lets the UI drop chunks already covered by a snapshot. */
  data: (sessionId: string, data: string, seq: number) => void
  /** Full session list, sent after any metadata change. */
  sessionsChanged: (sessions: SessionMeta[]) => void
}

export interface NmuxApi {
  sessions: {
    list(): Promise<SessionMeta[]>
    create(req: CreateSessionRequest): Promise<SessionMeta>
    /** Resume a Claude session (`--resume`) or respawn a shell in the same cwd. */
    restart(id: string): Promise<SessionMeta>
    kill(id: string): Promise<void>
    remove(id: string): Promise<void>
    rename(id: string, name: string): Promise<void>
    attach(id: string): Promise<AttachResult>
    write(id: string, data: string): void
    resize(id: string, cols: number, rows: number): void
  }
  dialog: {
    pickFolder(): Promise<string | null>
  }
  app: {
    info(): Promise<AppInfo>
  }
  /** Subscribe to a push event. Returns an unsubscribe function. */
  on<E extends keyof NmuxEvents>(event: E, listener: NmuxEvents[E]): () => void
}
