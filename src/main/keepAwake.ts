/**
 * Stops the machine from going to sleep by itself while a session is busy,
 * so a locked laptop keeps working on a long Claude task. Uses Electron's
 * `powerSaveBlocker` in `prevent-app-suspension` mode: the display may turn
 * off and the screen may lock, but the idle timer never puts the system to
 * sleep. A manual sleep, hibernate or lid close is not overridden; those
 * pause every process and the sessions resume after wake.
 *
 * "Busy" means some running session produced output recently. Claude Code
 * redraws its spinner while it works, so output is a reliable signal; a
 * shell sitting at a prompt does not keep the machine awake.
 */
import { powerMonitor, powerSaveBlocker } from 'electron'
import type { KeepAwakeMode } from './settings'

/** No output for this long means the sessions are idle. */
const BUSY_WINDOW_MS = 3 * 60_000
const TICK_MS = 15_000

export interface KeepAwakeOptions {
  mode(): KeepAwakeMode
  log(message: string): void
  /** Called whenever `active` changes, e.g. to update the tray tooltip. */
  onChange?(active: boolean): void
}

export class KeepAwake {
  private lastOutputAt = 0
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

  /** A running session produced output. */
  noteActivity(): void {
    this.lastOutputAt = Date.now()
    if (!this.active) this.evaluate()
  }

  /** Nothing is running any more: release immediately. */
  noteIdle(): void {
    this.lastOutputAt = 0
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
    const busy = Date.now() - this.lastOutputAt < BUSY_WINDOW_MS
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
