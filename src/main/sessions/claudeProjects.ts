/**
 * Read-only knowledge of where Claude Code keeps its conversation
 * transcripts, so Nmux can tell whether `claude --resume <id>` has anything
 * to resume. Pure Node, no Electron.
 *
 * Layout (Claude Code 2.x): <config dir>/projects/<encoded cwd>/<session id>.jsonl
 * where the config dir is $CLAUDE_CONFIG_DIR or ~/.claude, and the cwd is
 * encoded by replacing every non-alphanumeric character with "-".
 */
import { closeSync, existsSync, openSync, readSync, statSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

export type TranscriptState =
  /** A transcript with at least one message: `--resume` will work. */
  | 'resumable'
  /** A transcript file exists but holds no messages (e.g. only a Remote
   *  Control bridge record). `--resume` fails and the id cannot be reused. */
  | 'empty'
  /** No transcript at all: the conversation never got a message. */
  | 'missing'
  /** Claude's project folder for this cwd is not where we expect it; make no assumptions. */
  | 'unknown'

export function claudeConfigDir(env: NodeJS.ProcessEnv = process.env): string {
  return env.CLAUDE_CONFIG_DIR || join(homedir(), '.claude')
}

export function encodeProjectDir(cwd: string): string {
  return cwd.replace(/[\\/]+$/, '').replace(/[^A-Za-z0-9]/g, '-')
}

export function transcriptPath(cwd: string, claudeSessionId: string, env?: NodeJS.ProcessEnv): string {
  return join(claudeConfigDir(env), 'projects', encodeProjectDir(cwd), `${claudeSessionId}.jsonl`)
}

/** Lines that mean a real conversation exists. Cheap substring test per chunk. */
const MESSAGE_MARKERS = ['"type":"user"', '"type":"assistant"']
const CHUNK = 64 * 1024
/** Stop scanning after this much; a message shows up early if there is one. */
const MAX_SCAN = 8 * 1024 * 1024

export function transcriptState(cwd: string, claudeSessionId: string, env?: NodeJS.ProcessEnv): TranscriptState {
  const file = transcriptPath(cwd, claudeSessionId, env)
  const projectDir = join(file, '..')
  try {
    if (!existsSync(projectDir)) return 'unknown'
    if (!existsSync(file)) return 'missing'
    if (statSync(file).size === 0) return 'empty'
    return hasMessages(file) ? 'resumable' : 'empty'
  } catch {
    return 'unknown'
  }
}

function hasMessages(file: string): boolean {
  const fd = openSync(file, 'r')
  try {
    const buf = Buffer.alloc(CHUNK)
    let carry = ''
    let position = 0
    while (position < MAX_SCAN) {
      const n = readSync(fd, buf, 0, CHUNK, position)
      if (n === 0) break
      position += n
      const text = carry + buf.toString('utf8', 0, n)
      if (MESSAGE_MARKERS.some((m) => text.includes(m))) return true
      // Keep a tail so a marker split across chunks is still found.
      carry = text.slice(-32)
    }
    return false
  } finally {
    closeSync(fd)
  }
}
