/**
 * Stops the machine from going to sleep by itself while a session is busy,
 * so a locked laptop keeps working on a long Claude task. Uses Electron's
 * `powerSaveBlocker` in `prevent-app-suspension` mode: the display may turn
 * off and the screen may lock, but the idle timer never puts the system to
 * sleep. A manual sleep, hibernate or lid close is not overridden; those
 * pause every process and the sessions resume after wake.
 *
 * "Busy" means some Claude session is in the middle of a turn (the daemon
 * reads that from Claude Code's title spinner), or was a short while ago.
 * Shells never keep the machine awake.
 */
import { powerMonitor, powerSaveBlocker } from 'electron'
import type { KeepAwakeMode } from './settings'

/** Stay awake this long after the last turn ended, so back-to-back turns don't flap. */
const BUSY_WINDOW_MS = 3 * 60_000
const TICK_MS = 15_000

export interface KeepAwakeOptions {
  mode(): KeepAwakeMode
  log(message: string): void
  /** Called whenever `active` changes, e.g. to update the tray tooltip. */
  onChange?(active: boolean): void
}

export class KeepAwake {
  private working = false
  private lastWorkingAt = 0
  private blockerId: number | null = null
  private timer: NodeJS.Timeout | null = null

  constructor(private readonly opts: KeepAwakeOptions) {
    this.timer = setInterval(() => this.evaluate(), TICK_MS)
    powerMonitor.on('on-ac', () => this.evaluate())
    powerMonitor.on('on-battery', () => this.evaluate())
    powerMonitor.on('suspend', () => this.opts.log('system is going to sleep; sessions pause until wake'))
    powerMonitor.on('resume', () => this.opts.log('system woke up'))
  }

  get active(): boolean {
    return this.blockerId !== null && powerSaveBlocker.isStarted(this.blockerId)
  }

  /** Whether any session is working right now. Call on every session list change. */
  setWorking(working: boolean): void {
    if (working === this.working) return
    this.working = working
    this.lastWorkingAt = Date.now()
    this.evaluate()
  }

  /** Nothing is running any more: release immediately. */
  noteIdle(): void {
    this.working = false
    this.lastWorkingAt = 0
    this.evaluate()
  }

  /** Settings changed. */
  refresh(): void {
    this.evaluate()
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
    this.stop()
  }

  private evaluate(): void {
    const busy = this.working || Date.now() - this.lastWorkingAt < BUSY_WINDOW_MS
    const mode = this.opts.mode()
    const allowed = mode === 'always' || (mode === 'plugged-in' && !powerMonitor.isOnBatteryPower())
    if (busy && allowed) this.start()
    else this.stop()
  }

  private start(): void {
    if (this.active) return
    this.blockerId = powerSaveBlocker.start('prevent-app-suspension')
    this.opts.log('keeping the system awake while sessions are busy')
    this.opts.onChange?.(true)
  }

  private stop(): void {
    if (this.blockerId === null) return
    if (powerSaveBlocker.isStarted(this.blockerId)) powerSaveBlocker.stop(this.blockerId)
    this.blockerId = null
    this.opts.log('sessions idle; the system may sleep again')
    this.opts.onChange?.(false)
  }
}
