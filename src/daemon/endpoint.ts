/**
 * Where the daemon and its clients find each other. Everything is derived
 * from the app's userData directory, so a dev profile and an installed Nmux
 * never share a daemon. Pure Node, no Electron.
 */
import { createHash } from 'node:crypto'
import { join, resolve } from 'node:path'

const isWin = process.platform === 'win32'

/** `daemon.json`: pid, pipe and token of the running daemon. */
export function daemonFilePath(userDataDir: string): string {
  return join(userDataDir, 'daemon.json')
}

/** Where the daemon's stdout/stderr go. */
export function daemonLogPath(userDataDir: string): string {
  return join(userDataDir, 'daemon.log')
}

/** Named pipe (Windows) or Unix socket the daemon listens on. */
export function pipePath(userDataDir: string): string {
  const key = resolve(userDataDir)
  const hash = createHash('sha256')
    .update(isWin ? key.toLowerCase() : key)
    .digest('hex')
    .slice(0, 16)
  return isWin ? `\\\\.\\pipe\\nmux-${hash}` : join(userDataDir, `daemon-${hash}.sock`)
}
