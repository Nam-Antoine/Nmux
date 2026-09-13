/**
 * Persists session metadata as JSON in the app's userData directory.
 * Writes are atomic (temp file + rename) so a crash never leaves a torn file.
 */
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import type { SessionMeta } from '@shared/session'

interface StoreFile {
  version: 1
  sessions: SessionMeta[]
}

export class SessionStore {
  constructor(private readonly file: string) {}

  load(): SessionMeta[] {
    if (!existsSync(this.file)) return []
    try {
      const parsed = JSON.parse(readFileSync(this.file, 'utf8')) as Partial<StoreFile>
      if (parsed.version !== 1 || !Array.isArray(parsed.sessions)) return []
      return parsed.sessions
    } catch (err) {
      console.error(`[SessionStore] failed to read ${this.file}:`, err)
      return []
    }
  }

  save(sessions: SessionMeta[]): void {
    const data: StoreFile = { version: 1, sessions }
    mkdirSync(dirname(this.file), { recursive: true })
    const tmp = `${this.file}.tmp`
    writeFileSync(tmp, JSON.stringify(data, null, 2), 'utf8')
    renameSync(tmp, this.file)
  }
}
