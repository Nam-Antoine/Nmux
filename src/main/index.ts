/**
 * Main-process entry: single instance, session manager, window, tray, quit.
 */
import { join } from 'node:path'
import { app, dialog } from 'electron'
import { registerIpc } from './ipc'
import { registerAppScheme, serveRenderer } from './protocol'
import { hardenDefaultSession, hardenWebContents } from './security'
import { SessionManager } from './sessions/SessionManager'
import { SessionStore } from './store/SessionStore'
import { createTray, updateTrayTooltip } from './tray'
import { createMainWindow, getMainWindow, markQuitting, showMainWindow } from './window'

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.setAppUserModelId('dev.namtran.nmux')
  // Every renderer runs sandboxed, regardless of per-window settings.
  app.enableSandbox()
  registerAppScheme()

  let manager: SessionManager | null = null

  app.on('second-instance', () => showMainWindow())
  app.on('web-contents-created', (_event, contents) => hardenWebContents(contents))

  void app.whenReady().then(() => {
    hardenDefaultSession()
    serveRenderer()

    manager = new SessionManager(
      new SessionStore(join(app.getPath('userData'), 'sessions.json'))
    )
    manager.on('changed', () => updateTrayTooltip(manager?.runningCount() ?? 0))

    registerIpc(manager)
    createMainWindow()
    createTray({
      onShow: showMainWindow,
      onQuit: () => void requestQuit(),
      runningCount: () => manager?.runningCount() ?? 0
    })

    app.on('activate', showMainWindow)
  })

  // Closing the last window must NOT quit: sessions live in this process.
  app.on('window-all-closed', () => {
    /* keep running in the tray */
  })

  app.on('before-quit', () => markQuitting())
  app.on('will-quit', () => manager?.dispose())

  async function requestQuit(): Promise<void> {
    const running = manager?.runningCount() ?? 0
    if (running > 0) {
      const options = {
        type: 'warning' as const,
        buttons: ['Quit and stop sessions', 'Cancel'],
        defaultId: 1,
        cancelId: 1,
        title: 'Quit Nmux',
        message: running === 1 ? '1 session is still running.' : `${running} sessions are still running.`,
        detail:
          'Quitting stops every process. Claude sessions can be resumed later from the session list.'
      }
      const owner = getMainWindow()
      const { response } = owner
        ? await dialog.showMessageBox(owner, options)
        : await dialog.showMessageBox(options)
      if (response !== 0) return
    }
    app.quit()
  }
}
