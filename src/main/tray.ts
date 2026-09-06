import { Menu, Tray, nativeImage } from 'electron'
import { trayIconPath } from './paths'

export interface TrayHandlers {
  onShow(): void
  onQuit(): void
  runningCount(): number
}

let tray: Tray | null = null

export function createTray(handlers: TrayHandlers): Tray {
  tray = new Tray(nativeImage.createFromPath(trayIconPath()))

  const menu = Menu.buildFromTemplate([
    { label: 'Show Nmux', click: handlers.onShow },
    { type: 'separator' },
    { label: 'Quit Nmux', click: handlers.onQuit }
  ])
  tray.setContextMenu(menu)
  tray.on('click', handlers.onShow)
  tray.on('double-click', handlers.onShow)
  updateTrayTooltip(handlers.runningCount())
  return tray
}

export function updateTrayTooltip(running: number): void {
  if (!tray) return
  const label = running === 1 ? '1 running session' : `${running} running sessions`
  tray.setToolTip(running === 0 ? 'Nmux - no running sessions' : `Nmux - ${label}`)
}
