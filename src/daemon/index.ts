/**
 * Daemon entry. Runs as a plain Node process (the Electron binary with
 * ELECTRON_RUN_AS_NODE=1) detached from the app, so sessions outlive the
 * window, the main process and app restarts. The app connects through the
 * pipe advertised in `<userData>/daemon.json`.
 *
 *   daemon.js --user-data <dir> --app-version <version>
 */
import { randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { DAEMON_PROTOCOL_VERSION, type DaemonFile, type DaemonInfo } from '@shared/daemon'
import { daemonFilePath, pipePath } from './endpoint'
import { DaemonServer } from './server'
import { findOnPath } from './sessions/launch'
import { SessionManager } from './sessions/SessionManager'
import { SessionStore } from './store/SessionStore'

function argValue(flag: string): string | undefined {
  const i = process.argv.indexOf(flag)
  return i !== -1 ? process.argv[i + 1] : undefined
}

function log(message: string): void {
  console.log(`${new Date().toISOString()} ${message}`)
}

async function main(): Promise<void> {
  const userData = argValue('--user-data')
  if (!userData) {
    console.error('usage: daemon.js --user-data <dir> [--app-version <version>]')
    process.exit(2)
  }
  const appVersion = argValue('--app-version') ?? '0.0.0'
  process.title = 'nmux-daemon'
  mkdirSync(userData, { recursive: true })

  const startedAt = Date.now()
  const token = randomBytes(32).toString('hex')
  const pipe = pipePath(userData)
  const daemonFile = daemonFilePath(userData)

  const manager = new SessionManager(new SessionStore(join(userData, 'sessions.json')))
  const info = (): DaemonInfo => ({
    protocolVersion: DAEMON_PROTOCOL_VERSION,
    version: appVersion,
    pid: process.pid,
    startedAt,
    runningCount: manager.runningCount(),
    claudeBinary: findOnPath('claude')
  })

  let exiting = false
  const exit = (reason: string, stopSessions: boolean): void => {
    if (exiting) return
    exiting = true
    log(`exiting: ${reason}`)
    server.close()
    if (stopSessions) manager.dispose()
    else manager.flushSave()
    removeDaemonFile(daemonFile)
    // Give node-pty a moment to reap its children before the process ends.
    setTimeout(() => process.exit(0), 200)
  }

  const server = new DaemonServer(manager, {
    token,
    info,
    log,
    onShutdown: () => exit('shutdown requested', true),
    onIdle: () => exit('no clients and no running sessions', false)
  })

  try {
    await server.listen(pipe)
  } catch (err) {
    const code = (err as NodeJS.ErrnoException).code
    if (code === 'EADDRINUSE') {
      log('another daemon already owns the pipe, exiting')
      process.exit(0)
    }
    throw err
  }

  const file: DaemonFile = { version: 1, pid: process.pid, pipe, token, appVersion, startedAt }
  writeDaemonFile(daemonFile, file)
  log(`nmux daemon ${appVersion} listening on ${pipe} (pid ${process.pid})`)

  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
    process.on(signal, () => exit(signal, true))
  }
  // A bug in one code path must not take every user session down with it.
  process.on('uncaughtException', (err) => log(`uncaught exception: ${err.stack ?? err.message}`))
  process.on('unhandledRejection', (reason) => log(`unhandled rejection: ${String(reason)}`))
}

function writeDaemonFile(path: string, file: DaemonFile): void {
  const tmp = `${path}.tmp`
  writeFileSync(tmp, JSON.stringify(file, null, 2), { encoding: 'utf8', mode: 0o600 })
  renameSync(tmp, path)
}

/** Delete `daemon.json` only if it still describes this process. */
function removeDaemonFile(path: string): void {
  try {
    if (!existsSync(path)) return
    const current = JSON.parse(readFileSync(path, 'utf8')) as Partial<DaemonFile>
    if (current.pid === process.pid) unlinkSync(path)
  } catch {
    /* ignore */
  }
}

main().catch((err) => {
  console.error('daemon failed to start:', err)
  process.exit(1)
})
