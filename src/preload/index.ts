/**
 * Preload: the only bridge between the sandboxed renderer and the main
 * process. It exposes exactly `NmuxApi` as `window.nmux` and nothing else.
 */
import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import { IPC, type NmuxApi, type NmuxEvents } from '@shared/ipc'

const eventChannels: Record<keyof NmuxEvents, string> = {
  data: IPC.evData,
  sessionsChanged: IPC.evSessionsChanged
}

const api: NmuxApi = {
  sessions: {
    list: () => ipcRenderer.invoke(IPC.sessionsList),
    create: (req) => ipcRenderer.invoke(IPC.sessionsCreate, req),
    restart: (id) => ipcRenderer.invoke(IPC.sessionsRestart, id),
    kill: (id) => ipcRenderer.invoke(IPC.sessionsKill, id),
    remove: (id) => ipcRenderer.invoke(IPC.sessionsRemove, id),
    rename: (id, name) => ipcRenderer.invoke(IPC.sessionsRename, id, name),
    attach: (id) => ipcRenderer.invoke(IPC.sessionsAttach, id),
    write: (id, data) => ipcRenderer.send(IPC.sessionsWrite, id, data),
    resize: (id, cols, rows) => ipcRenderer.send(IPC.sessionsResize, id, cols, rows)
  },
  dialog: {
    pickFolder: () => ipcRenderer.invoke(IPC.dialogPickFolder)
  },
  app: {
    info: () => ipcRenderer.invoke(IPC.appInfo)
  },
  on(event, listener) {
    const channel = Object.hasOwn(eventChannels, event) ? eventChannels[event] : undefined
    if (!channel || typeof listener !== 'function') throw new TypeError(`Unknown event: ${String(event)}`)
    const wrapped = (_e: IpcRendererEvent, ...args: unknown[]): void => {
      ;(listener as (...a: unknown[]) => void)(...args)
    }
    ipcRenderer.on(channel, wrapped)
    return () => ipcRenderer.removeListener(channel, wrapped)
  }
}

contextBridge.exposeInMainWorld('nmux', api)
