import { Menu, Tray, nativeImage } from 'electron'
import { trayIconPath } from './paths'
import type { KeepAwakeMode } from './settings'

export interface TrayHandlers {
  onShow(): void
  /** `stopSessions` false: detach and quit, sessions keep running in the daemon. */
  onQuit(stopSessions: boolean): void
  runningCount(): number
  keepAwakeMode(): KeepAwakeMode
  setKeepAwakeMode(mode: KeepAwakeMode): void
}

let tray: Tray | null = null
let handlers: TrayHandlers | null = null
let lastRunning = 0
let lastAwake = false

export function createTray(h: TrayHandlers): Tray {
  handlers = h
  tray = new Tray(nativeImage.createFromPath(trayIconPath()))
  rebuildMenu()
  tray.on('click', h.onShow)
  tray.on('double-click', h.onShow)
  updateTrayTooltip(h.runningCount(), false)
  return tray
}

function rebuildMenu(): void {
  if (!tray || !handlers) return
  const h = handlers
  const mode = h.keepAwakeMode()
  const choice = (label: string, value: KeepAwakeMode): Electron.MenuItemConstructorOptions => ({
    label,
    type: 'radio',
    checked: mode === value,
    click: () => {
      h.setKeepAwakeMode(value)
      rebuildMenu()
    }
  })
  const menu = Menu.buildFromTemplate([
    { label: 'Show Nmux', click: h.onShow },
    { type: 'separator' },
    {
      label: 'Keep PC awake while sessions are busy',
      sublabel: 'Screen may lock and turn off; automatic sleep is postponed',
      submenu: [
        choice('Only when plugged in', 'plugged-in'),
        choice('Always (also on battery)', 'always'),
        choice('Never', 'never')
      ]
    },
    { type: 'separator' },
    {
      label: 'Quit Nmux',
      sublabel: 'Sessions keep running in the background',
      click: () => h.onQuit(false)
    },
    { label: 'Quit and stop all sessions', click: () => h.onQuit(true) }
  ])
  tray.setContextMenu(menu)
}

export function updateTrayTooltip(running: number, keepingAwake = lastAwake): void {
  if (!tray) return
  lastRunning = running
  lastAwake = keepingAwake
  const label = running === 1 ? '1 running session' : `${running} running sessions`
  const awake = keepingAwake ? ' · keeping PC awake' : ''
  tray.setToolTip(running === 0 ? 'Nmux - no running sessions' : `Nmux - ${label}${awake}`)
}

export function setTrayKeepingAwake(keepingAwake: boolean): void {
  updateTrayTooltip(lastRunning, keepingAwake)
}
