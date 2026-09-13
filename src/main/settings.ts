/**
 * Small persisted app settings (`<userData>/settings.json`). Main process only.
 */
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { app } from 'electron'

/**
 * When to hold a power request that stops the machine from sleeping on its
 * own while a session is producing output. The display may still turn off
 * and the screen may be locked; only automatic sleep is prevented.
 */
export type KeepAwakeMode = 'always' | 'plugged-in' | 'never'

export interface Settings {
  keepAwake: KeepAwakeMode
}

const DEFAULTS: Settings = { keepAwake: 'plugged-in' }

function file(): string {
  return join(app.getPath('userData'), 'settings.json')
}

export function loadSettings(): Settings {
  const path = file()
  if (!existsSync(path)) return { ...DEFAULTS }
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8')) as Partial<Settings>
    const keepAwake =
      parsed.keepAwake === 'always' || parsed.keepAwake === 'plugged-in' || parsed.keepAwake === 'never'
        ? parsed.keepAwake
        : DEFAULTS.keepAwake
    return { keepAwake }
  } catch (err) {
    console.warn('[settings] unreadable, using defaults:', err instanceof Error ? err.message : err)
    return { ...DEFAULTS }
  }
}

export function saveSettings(settings: Settings): void {
  const path = file()
  try {
    mkdirSync(dirname(path), { recursive: true })
    const tmp = `${path}.tmp`
    writeFileSync(tmp, JSON.stringify(settings, null, 2), 'utf8')
    renameSync(tmp, path)
  } catch (err) {
    console.error('[settings] save failed:', err)
  }
}
