/**
 * Main window factory. Closing the window hides it; the app (and every
 * session) keeps running in the tray until the user explicitly quits.
 */
import { join } from 'node:path'
import { BrowserWindow, Menu, app } from 'electron'
import { iconPath } from './paths'
import { rendererBaseUrl } from './protocol'

let win: BrowserWindow | null = null
let quitting = false

/** Called from `before-quit` so the close handler lets the window go. */
export function markQuitting(): void {
  quitting = true
}

export function createMainWindow(): BrowserWindow {
  win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 820,
    minHeight: 520,
    show: false,
    autoHideMenuBar: true,
    backgroundColor: '#0f1115',
    title: 'Nmux',
    icon: iconPath(),
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webviewTag: false,
      // No DevTools, reload or menu accelerators in the packaged app.
      devTools: !app.isPackaged,
      // Spellcheck would download dictionaries; the UI has no prose input.
      spellcheck: false,
      // Keep the renderer's timers running while hidden so xterm stays in sync.
      backgroundThrottling: false
    }
  })

  if (app.isPackaged) Menu.setApplicationMenu(null)

  win.on('ready-to-show', () => win?.show())

  win.on('close', (event) => {
    if (quitting) return
    event.preventDefault()
    win?.hide()
  })

  win.on('closed', () => {
    win = null
  })

  // Dev: electron-vite's HTTP server. Prod: app://nmux/ served by protocol.ts.
  void win.loadURL(rendererBaseUrl())

  return win
}

export function showMainWindow(): void {
  if (!win) {
    createMainWindow()
    return
  }
  if (win.isMinimized()) win.restore()
  win.show()
  win.focus()
}

export function getMainWindow(): BrowserWindow | null {
  return win
}
