/**
 * Main-process entry: single instance, daemon client, window, tray, quit.
 *
 * Sessions do not live here. They live in the daemon (src/daemon), which
 * this process starts on demand and attaches to; quitting the app leaves it
 * running unless the user asks to stop everything.
 */
import { join } from 'node:path'
import { app, dialog, powerMonitor } from 'electron'
import type { SessionMeta } from '@shared/session'
import { DaemonClient } from './daemon/client'
import { registerIpc } from './ipc'
import { KeepAwake } from './keepAwake'
import { registerAppScheme, serveRenderer } from './protocol'
import { hardenDefaultSession, hardenWebContents } from './security'
import { loadSettings, saveSettings } from './settings'
import { createTray, setTrayKeepingAwake, updateTrayTooltip } from './tray'
import { createMainWindow, getMainWindow, markQuitting, showMainWindow } from './window'

// Development aid: give the dev build its own profile so it never fights an
// installed Nmux for the single-instance lock, sessions.json or the daemon
// (all resolve to %APPDATA%/nmux on Windows). Ignored in packaged builds so
// the environment cannot redirect a real install.
if (!app.isPackaged && process.env.NMUX_USER_DATA_DIR) {
  app.setPath('userData', process.env.NMUX_USER_DATA_DIR)
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.setAppUserModelId('dev.namtran.nmux')
  // Every renderer runs sandboxed, regardless of per-window settings.
  app.enableSandbox()
  registerAppScheme()

  let client: DaemonClient | null = null
  let keepAwake: KeepAwake | null = null
  let runningCount = 0

  app.on('second-instance', () => showMainWindow())
  app.on('web-contents-created', (_event, contents) => hardenWebContents(contents))

  void app.whenReady().then(async () => {
    hardenDefaultSession()
    serveRenderer()

    client = new DaemonClient({
      userDataDir: app.getPath('userData'),
      script: join(__dirname, 'daemon.js'),
      execPath: process.execPath,
      appVersion: app.getVersion(),
      log: (message) => console.log(`[daemon] ${message}`)
    })
    const settings = loadSettings()
    keepAwake = new KeepAwake({
      mode: () => settings.keepAwake,
      log: (message) => console.log(`[power] ${message}`),
      onChange: setTrayKeepingAwake
    })

    const setRunning = (sessions: SessionMeta[]): void => {
      runningCount = sessions.filter((s) => s.status === 'running').length
      updateTrayTooltip(runningCount)
      if (runningCount === 0) keepAwake?.noteIdle()
      else keepAwake?.setWorking(sessions.some((s) => s.status === 'running' && s.working))
    }
    client.on('changed', setRunning)
    client.on('connected', (info) => {
      runningCount = info.runningCount
      updateTrayTooltip(runningCount)
    })
    client.on('disconnected', () => {
      updateTrayTooltip(0)
      keepAwake?.noteIdle()
    })
    registerIpc(client)

    // After sleep or hibernate the pipe is normally intact; a quick round
    // trip finds out immediately if the daemon is gone, so the reconnect
    // starts now rather than on the next keystroke.
    powerMonitor.on('resume', () => {
      client?.call('info').catch(() => {
        /* the close handler already schedules a reconnect */
      })
    })

    try {
      await client.start()
    } catch (err) {
      dialog.showErrorBox(
        'Nmux could not start its session daemon',
        `${err instanceof Error ? err.message : String(err)}\n\nSee daemon.log in ${app.getPath('userData')}.`
      )
      app.exit(1)
      return
    }

    createMainWindow()
    createTray({
      onShow: showMainWindow,
      onQuit: (stopSessions) => void requestQuit(stopSessions),
      runningCount: () => runningCount,
      keepAwakeMode: () => settings.keepAwake,
      setKeepAwakeMode: (mode) => {
        settings.keepAwake = mode
        saveSettings(settings)
        keepAwake?.refresh()
      }
    })

    app.on('activate', showMainWindow)
  })

  // Closing the last window must NOT quit: the app lives in the tray.
  app.on('window-all-closed', () => {
    /* keep running in the tray */
  })

  app.on('before-quit', () => markQuitting())
  // Detach only. Sessions keep running in the daemon.
  app.on('will-quit', () => {
    keepAwake?.dispose()
    client?.close()
  })

  /**
   * Quit the app. With `stopSessions` the daemon is told to end every
   * session and exit first; without it the daemon (and everything in it)
   * keeps running and the next launch reattaches, like `tmux detach`.
   */
  async function requestQuit(stopSessions: boolean): Promise<void> {
    if (stopSessions) {
      if (runningCount > 0) {
        const options = {
          type: 'warning' as const,
          buttons: ['Quit and stop sessions', 'Cancel'],
          defaultId: 1,
          cancelId: 1,
          title: 'Quit Nmux',
          message:
            runningCount === 1 ? '1 session is still running.' : `${runningCount} sessions are still running.`,
          detail:
            'This stops every process. Claude sessions can be resumed later from the session list.\n\n' +
            'To keep them running in the background, use "Quit Nmux" instead.'
        }
        const owner = getMainWindow()
        const { response } = owner ? await dialog.showMessageBox(owner, options) : await dialog.showMessageBox(options)
        if (response !== 0) return
      }
      await client?.stopDaemon()
    }
    app.quit()
  }
}
