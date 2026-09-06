import { join } from 'node:path'
import { app } from 'electron'

/** Directory holding runtime assets (icons). Differs between dev and packaged. */
export function resourcesDir(): string {
  return app.isPackaged ? process.resourcesPath : join(app.getAppPath(), 'resources')
}

export function iconPath(): string {
  return join(resourcesDir(), 'icon.png')
}

export function trayIconPath(): string {
  return join(resourcesDir(), 'tray.png')
}
