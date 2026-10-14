/**
 * Tells whether Claude Code is working from the window title it sets.
 *
 * Claude Code writes `OSC 0 ; <glyph> <title>` and animates the glyph while a
 * turn runs (◐ ◓ ◑ ◒, braille dots in older versions); at rest it shows ✳.
 * That is a much better "working" signal than output flow, which also fires
 * for typing, screen redraws and anything a shell prints.
 */

const ESC = '\x1b'
const BEL = '\x07'
/** Leading glyphs that mean "a turn is running". */
const WORKING_RE = /^[◐◓◑◒⠁-⣿]/
/** An unterminated OSC longer than this is not a title; drop it. */
const MAX_PENDING = 1024

export class ClaudeTitleWatcher {
  private pending = ''

  /** Feed raw PTY output. Returns the new working state when a title in it says so, else null. */
  push(data: string): boolean | null {
    let text = this.pending + data
    this.pending = ''
    let result: boolean | null = null
    let i = 0
    while ((i = text.indexOf(`${ESC}]`, i)) >= 0) {
      const bel = text.indexOf(BEL, i + 2)
      const st = text.indexOf(`${ESC}\\`, i + 2)
      const end = bel < 0 ? st : st < 0 ? bel : Math.min(bel, st)
      if (end < 0) {
        // Split across chunks: keep the start for the next call.
        const rest = text.slice(i)
        if (rest.length <= MAX_PENDING) this.pending = rest
        break
      }
      const osc = text.slice(i + 2, end)
      const m = /^[02];(.*)$/s.exec(osc)
      if (m) result = WORKING_RE.test(m[1]!.trimStart())
      i = end + 1
    }
    text = ''
    return result
  }
}
