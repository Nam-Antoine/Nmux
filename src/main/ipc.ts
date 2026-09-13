/**
 * Adapts the daemon client to Electron IPC. This is the only file that knows
 * both about `ipcMain` and about sessions, and it is the renderer-facing
 * trust boundary: every message is checked for a trusted sender and
 * validated before it is forwarded to the daemon (which validates again).
 */
import { BrowserWindow, app, dialog, ipcMain, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import { IPC } from '@shared/ipc'
import type { AppInfo } from '@shared/session'
import { cols, createRequest, label, rows, uuid, writeChunk } from '../daemon/validate'
import type { DaemonClient } from './daemon/client'
import { isTrustedRendererUrl } from './protocol'

// ---- sender trust -------------------------------------------------------------

/** Only the main frame of our own renderer may talk to the main process. */
function isTrustedSender(event: IpcMainInvokeEvent | IpcMainEvent): boolean {
  const frame = event.senderFrame
  if (!frame || frame !== event.sender.mainFrame) return false
  return isTrustedRendererUrl(frame.url)
}

function handle(channel: string, fn: (event: IpcMainInvokeEvent, ...args: unknown[]) => unknown): void {
  ipcMain.handle(channel, (event, ...args) => {
    if (!isTrustedSender(event)) throw new Error('Rejected IPC call from untrusted sender')
    return fn(event, ...args)
  })
}

function on(channel: string, fn: (...args: unknown[]) => void): void {
  ipcMain.on(channel, (event, ...args) => {
    if (!isTrustedSender(event)) return
    try {
      fn(...args)
    } catch (err) {
      // Hot path: log and drop, never throw back.
      console.warn(`[ipc] ${channel} dropped:`, err instanceof Error ? err.message : err)
    }
  })
}

// ---- registration --------------------------------------------------------------

export function registerIpc(client: DaemonClient): void {
  handle(IPC.sessionsList, () => client.call('list'))
  handle(IPC.sessionsCreate, (_e, req) => client.call('create', createRequest(req)))
  handle(IPC.sessionsRestart, (_e, id) => client.call('restart', uuid(id, 'id')))
  handle(IPC.sessionsKill, (_e, id) => client.call('kill', uuid(id, 'id')))
  handle(IPC.sessionsRemove, (_e, id) => client.call('remove', uuid(id, 'id')))
  handle(IPC.sessionsRename, (_e, id, name) => client.call('rename', uuid(id, 'id'), label(name, 'name')))
  handle(IPC.sessionsAttach, (_e, id) => client.call('attach', uuid(id, 'id')))

  on(IPC.sessionsWrite, (id, data) => client.notify('write', uuid(id, 'id'), writeChunk(data)))
  on(IPC.sessionsResize, (id, c, r) => client.notify('resize', uuid(id, 'id'), cols(c), rows(r)))

  handle(IPC.dialogPickFolder, async (event) => {
    const owner = BrowserWindow.fromWebContents(event.sender)
    const options = {
      title: 'Choose a project folder',
      properties: ['openDirectory', 'createDirectory'] as Array<'openDirectory' | 'createDirectory'>
    }
    const result = owner ? await dialog.showOpenDialog(owner, options) : await dialog.showOpenDialog(options)
    return result.canceled ? null : (result.filePaths[0] ?? null)
  })

  handle(
    IPC.appInfo,
    (): AppInfo => ({
      version: app.getVersion(),
      platform: process.platform,
      userDataDir: app.getPath('userData'),
      homeDir: app.getPath('home'),
      claudeBinary: client.info?.claudeBinary ?? null,
      daemonPid: client.info?.pid ?? null
    })
  )

  const broadcast = (channel: string, ...payload: unknown[]): void => {
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) win.webContents.send(channel, ...payload)
    }
  }
  client.on('data', (id, data, seq) => broadcast(IPC.evData, id, data, seq))
  client.on('changed', (sessions) => broadcast(IPC.evSessionsChanged, sessions))
  // After a (re)connect the UI's list may be stale: push the daemon's truth.
  client.on('connected', () => {
    client
      .call('list')
      .then((sessions) => broadcast(IPC.evSessionsChanged, sessions))
      .catch(() => {
        /* disconnected again; the next connect will retry */
      })
  })
}
