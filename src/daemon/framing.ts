/**
 * Newline-delimited JSON framing shared by the daemon server and its clients.
 */
import type { RequestMessage, ResponseMessage, EventMessage } from '@shared/daemon'

export type Frame = RequestMessage | ResponseMessage | EventMessage

/** Serialize one frame. JSON never contains a raw newline, so `\n` is a safe delimiter. */
export function encodeFrame(frame: Frame): string {
  return JSON.stringify(frame) + '\n'
}

/**
 * Splits a stream of text into lines and parses each as JSON. A line longer
 * than `maxChars` means a broken or hostile peer: `onOverflow` fires and the
 * splitter stops accepting input.
 */
export class LineSplitter {
  private buffer = ''
  private dead = false

  constructor(
    private readonly maxChars: number,
    private readonly onFrame: (frame: unknown) => void,
    private readonly onOverflow: () => void
  ) {}

  push(chunk: string): void {
    if (this.dead) return
    this.buffer += chunk
    let start = 0
    for (;;) {
      const nl = this.buffer.indexOf('\n', start)
      if (nl === -1) break
      const line = this.buffer.slice(start, nl)
      start = nl + 1
      if (line.length === 0) continue
      let frame: unknown
      try {
        frame = JSON.parse(line)
      } catch {
        continue // Skip garbage; the peer will notice a missing reply.
      }
      this.onFrame(frame)
      if (this.dead) return
    }
    this.buffer = start === 0 ? this.buffer : this.buffer.slice(start)
    if (this.buffer.length > this.maxChars) {
      this.dead = true
      this.buffer = ''
      this.onOverflow()
    }
  }
}
