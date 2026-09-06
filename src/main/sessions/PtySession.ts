/**
 * One live process: a node-pty handle plus a headless xterm that mirrors the
 * screen. The headless terminal is what lets the UI detach and reattach
 * without losing what is on screen (same technique VS Code uses for
 * terminal reconnection).
 *
 * No Electron imports here so this can move into a standalone daemon later.
 */
import { EventEmitter } from 'node:events'
import * as pty from 'node-pty'
import { Terminal } from '@xterm/headless'
import { SerializeAddon } from '@xterm/addon-serialize'
import type { AttachResult } from '@shared/session'
import type { LaunchSpec } from './launch'

export interface PtySessionOptions {
  launch: LaunchSpec
  cwd: string
  env: Record<string, string>
  cols: number
  rows: number
  scrollback: number
}

export interface PtySessionEvents {
  data: (data: string, seq: number) => void
  exit: (exitCode: number) => void
}

export class PtySession extends EventEmitter {
  readonly pid: number
  private readonly proc: pty.IPty
  private readonly term: Terminal
  private readonly serializer: SerializeAddon
  private readonly scrollback: number
  /** Chunks received from the PTY so far. */
  private seq = 0
  /** Highest seq the headless terminal has finished parsing. */
  private parsedSeq = 0
  private exited = false
  private disposed = false

  constructor(opts: PtySessionOptions) {
    super()
    this.scrollback = opts.scrollback
    this.term = new Terminal({
      cols: opts.cols,
      rows: opts.rows,
      scrollback: opts.scrollback,
      allowProposedApi: true
    })
    this.serializer = new SerializeAddon()
    this.term.loadAddon(this.serializer)

    this.proc = pty.spawn(opts.launch.file, opts.launch.args, {
      name: 'xterm-256color',
      cols: opts.cols,
      rows: opts.rows,
      cwd: opts.cwd,
      env: opts.env,
      useConpty: true
    })
    this.pid = this.proc.pid

    this.proc.onData((data) => {
      const seq = ++this.seq
      // The callback fires once xterm has parsed this chunk, so `parsedSeq`
      // always matches what `snapshot()` will serialize.
      this.term.write(data, () => {
        this.parsedSeq = seq
      })
      this.emit('data', data, seq)
    })

    this.proc.onExit(({ exitCode }) => {
      this.exited = true
      this.emit('exit', exitCode)
    })
  }

  override on<E extends keyof PtySessionEvents>(event: E, listener: PtySessionEvents[E]): this {
    return super.on(event, listener)
  }

  get isRunning(): boolean {
    return !this.exited
  }

  get cols(): number {
    return this.term.cols
  }

  get rows(): number {
    return this.term.rows
  }

  write(data: string): void {
    if (!this.exited) this.proc.write(data)
  }

  /** Print a dim line from Nmux itself (not from the process), e.g. a resume notice. */
  note(text: string): void {
    const data = `\x1b[2m[nmux] ${text}\x1b[0m\r\n`
    const seq = ++this.seq
    this.term.write(data, () => {
      this.parsedSeq = seq
    })
    this.emit('data', data, seq)
  }

  resize(cols: number, rows: number): void {
    if (cols < 2 || rows < 1) return
    if (cols === this.term.cols && rows === this.term.rows) return
    this.term.resize(cols, rows)
    if (!this.exited) this.proc.resize(cols, rows)
  }

  /** Current screen + scrollback, and the seq it is consistent with. */
  snapshot(): AttachResult {
    return {
      snapshot: this.serializer.serialize({ scrollback: this.scrollback }),
      seq: this.parsedSeq,
      cols: this.term.cols,
      rows: this.term.rows
    }
  }

  kill(): void {
    if (!this.exited) this.proc.kill()
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    this.kill()
    this.term.dispose()
    this.removeAllListeners()
  }
}
