/**
 * Builds the command lines Nmux spawns. Pure functions, no Electron.
 */
import { existsSync } from 'node:fs'
import { delimiter, join } from 'node:path'

const isWin = process.platform === 'win32'

export interface LaunchSpec {
  file: string
  args: string[]
}

/** Resolve an executable on PATH, honouring PATHEXT on Windows. */
export function findOnPath(name: string, env: NodeJS.ProcessEnv = process.env): string | null {
  const pathVar = env.PATH ?? env.Path ?? ''
  const dirs = pathVar.split(delimiter).filter(Boolean)
  const exts = isWin
    ? (env.PATHEXT ?? '.EXE;.CMD;.BAT;.COM').split(';').map((e) => e.toLowerCase())
    : ['']
  for (const dir of dirs) {
    for (const ext of exts) {
      const candidate = join(dir, name + ext)
      if (existsSync(candidate)) return candidate
    }
  }
  return null
}

export interface ClaudeLaunchOptions {
  /** UUID Nmux owns for this conversation. */
  claudeSessionId: string
  /** true -> `--resume <id>`, false -> `--session-id <id>` */
  resume: boolean
  extraArgs?: string[]
}

export function claudeLaunch(opts: ClaudeLaunchOptions): LaunchSpec {
  const bin = findOnPath('claude') ?? (isWin ? 'claude.exe' : 'claude')
  const sessionArgs = opts.resume
    ? ['--resume', opts.claudeSessionId]
    : ['--session-id', opts.claudeSessionId]
  const args = [...sessionArgs, ...(opts.extraArgs ?? [])]

  // npm-installed claude is a .cmd shim on Windows; ConPTY needs cmd.exe to run it.
  if (isWin && /\.(cmd|bat)$/i.test(bin)) {
    return { file: process.env.COMSPEC ?? 'cmd.exe', args: ['/d', '/c', bin, ...args] }
  }
  return { file: bin, args }
}

export function shellLaunch(): LaunchSpec {
  if (isWin) {
    const pwsh = findOnPath('pwsh') ?? findOnPath('powershell') ?? 'powershell.exe'
    return { file: pwsh, args: ['-NoLogo'] }
  }
  return { file: process.env.SHELL ?? '/bin/bash', args: ['-l'] }
}

/**
 * Variables a running Claude Code instance sets for its children. If Nmux is
 * launched from inside a Claude Code session (e.g. `pnpm dev` during
 * development), inheriting these makes the nested `claude` think it is a
 * child session and refuse to start. User configuration such as
 * CLAUDE_CODE_USE_BEDROCK or ANTHROPIC_* is deliberately left alone.
 */
const NESTED_CLAUDE_MARKERS = new Set([
  'CLAUDECODE',
  'CLAUDE_PID',
  'CLAUDE_EFFORT',
  'CLAUDE_CODE_ENTRYPOINT',
  'CLAUDE_CODE_EXECPATH',
  'CLAUDE_CODE_SESSION_ID',
  'CLAUDE_CODE_CHILD_SESSION',
  'CLAUDE_CODE_BRIDGE_SESSION_ID',
  'CLAUDE_CODE_MESSAGING_SOCKET',
  'CLAUDE_CODE_MESSAGING_TOKEN'
])

/** Environment for a spawned session. Strips Electron-only and nested-Claude variables. */
export function sessionEnv(nmuxSessionId: string): Record<string, string> {
  const env: Record<string, string> = {}
  for (const [k, v] of Object.entries(process.env)) {
    if (v === undefined) continue
    if (k.startsWith('ELECTRON_')) continue
    if (NESTED_CLAUDE_MARKERS.has(k)) continue
    env[k] = v
  }
  env.TERM = 'xterm-256color'
  env.COLORTERM = 'truecolor'
  env.TERM_PROGRAM = 'nmux'
  env.NMUX_SESSION_ID = nmuxSessionId
  return env
}
