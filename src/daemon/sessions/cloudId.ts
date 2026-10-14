/**
 * Finds the cloud session id in `claude --cloud` output.
 *
 * When the CLI opens a cloud session it prints a note like
 *   Cloud session: session_01AbC... (https://claude.ai/code/session_01AbC...)
 * Nmux keeps that id so a later restart can re-attach with
 * `claude --cloud <id>` instead of creating another cloud session.
 */

const ID = '(?:session|cse)_[A-Za-z0-9_-]{8,120}'
/** The id must be followed by something that is not part of an id, so a chunk split can't truncate it. */
const NOTE_RE = new RegExp(`Cloud session:\\s*(${ID})(?=[^A-Za-z0-9_-])(?:\\s*\\((https://[^\\s)]+)\\))?`)
const URL_RE = new RegExp(`https://claude\\.ai/code/(${ID})(?=[^A-Za-z0-9_-])`)

/** Characters of recent plain text kept between chunks. */
const KEEP = 4096

export interface CloudIdMatch {
  id: string
  url?: string
}

export class CloudIdSniffer {
  private text = ''

  /** Feed raw PTY output. Returns the id the first time it shows up. */
  push(data: string): CloudIdMatch | null {
    this.text = (this.text + toPlainText(data)).slice(-KEEP)
    const note = NOTE_RE.exec(this.text)
    if (note) return { id: note[1]!, url: safeUrl(note[2]) }
    const link = URL_RE.exec(this.text)
    if (link) return { id: link[1]!, url: safeUrl(link[0]) }
    return null
  }
}

/**
 * Drop terminal control sequences. Cursor-forward (`CSI n C`) becomes spaces:
 * Claude's renderer uses it instead of writing blanks between words.
 */
export function toPlainText(data: string): string {
  return (
    data
      // OSC (titles, hyperlinks): ESC ] ... BEL or ESC \
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\[(\d*)C/g, (_m, n: string) => ' '.repeat(Math.min(Number(n) || 1, 200)))
      // other CSI sequences
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b[@-_]/g, '')
      // eslint-disable-next-line no-control-regex
      .replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '')
  )
}

/** Only claude.ai pages are worth a button; anything else in the output is ignored. */
function safeUrl(raw: string | undefined): string | undefined {
  if (!raw || raw.length > 2048) return undefined
  try {
    const url = new URL(raw)
    return url.protocol === 'https:' && url.hostname === 'claude.ai' ? url.toString() : undefined
  } catch {
    return undefined
  }
}
