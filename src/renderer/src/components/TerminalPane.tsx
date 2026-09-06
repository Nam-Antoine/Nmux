/**
 * One xterm instance bound to one session, living inside a grid pane. It is
 * mounted only while a pane shows the session; re-mounting replays the
 * process's serialized screen, so nothing is lost. `active` means the pane
 * is the focused one (keyboard focus, refit).
 */
import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { SearchAddon } from '@xterm/addon-search'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import '@xterm/xterm/css/xterm.css'
import { SCROLLBACK_LINES } from '@shared/session'
import { api } from '../api'
import { subscribePty } from '../lib/ptyStream'
import { useSessions } from '../store/sessions'
import { terminalTheme } from '../styles/terminalTheme'

interface Props {
  sessionId: string
  active: boolean
}

interface TermHandle {
  term: Terminal
  fit: FitAddon
  /** Last size sent to the PTY, to avoid redundant resize round trips. */
  sent: { cols: number; rows: number }
  /** Whether attach() has run at least once for this xterm. */
  attached: boolean
}

export function TerminalPane({ sessionId, active }: Props): React.JSX.Element {
  const session = useSessions((s) => s.sessions.find((x) => x.id === sessionId))
  const restart = useSessions((s) => s.restart)
  const hostRef = useRef<HTMLDivElement>(null)
  const handleRef = useRef<TermHandle | null>(null)

  // 1. xterm lifecycle: one instance per session.
  useEffect(() => {
    const host = hostRef.current
    if (!host) return

    const term = new Terminal({
      allowProposedApi: true,
      cursorBlink: true,
      cursorStyle: 'bar',
      fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, "Courier New", monospace',
      fontSize: 13,
      lineHeight: 1.2,
      scrollback: SCROLLBACK_LINES,
      theme: terminalTheme,
      windowsPty: { backend: 'conpty' }
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new SearchAddon())
    term.loadAddon(new WebLinksAddon())
    term.loadAddon(new Unicode11Addon())
    term.unicode.activeVersion = '11'
    term.open(host)

    try {
      const webgl = new WebglAddon()
      webgl.onContextLoss(() => webgl.dispose())
      term.loadAddon(webgl)
    } catch (err) {
      console.warn('[TerminalPane] WebGL renderer unavailable, using DOM renderer', err)
    }

    const handle: TermHandle = { term, fit, sent: { cols: 0, rows: 0 }, attached: false }
    handleRef.current = handle

    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== 'keydown') return true
      const key = ev.key.toLowerCase()
      // App shortcuts bubble up to App.tsx.
      if (ev.ctrlKey && ((ev.shiftKey && key === 'n') || ev.key === 'PageUp' || ev.key === 'PageDown')) {
        return false
      }
      if (ev.ctrlKey && ev.shiftKey && (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight')) return false
      // Copy: Ctrl+Shift+C always, Ctrl+C only when there is a selection
      // (otherwise Ctrl+C must reach the process as SIGINT).
      if (ev.ctrlKey && key === 'c' && (ev.shiftKey || term.hasSelection())) {
        void navigator.clipboard.writeText(term.getSelection())
        term.clearSelection()
        return false
      }
      // Paste: let the browser fire a paste event on xterm's textarea.
      if (ev.ctrlKey && key === 'v') return false
      return true
    })

    const inputDisposable = term.onData((data) => api.sessions.write(sessionId, data))

    const observer = new ResizeObserver(() => syncSize(sessionId, handle))
    observer.observe(host)

    return () => {
      observer.disconnect()
      inputDisposable.dispose()
      term.dispose()
      handleRef.current = null
    }
  }, [sessionId])

  // 2. Attach to the process. Runs on first mount (an exited session still
  //    has its last screen) and again whenever a new process starts (pid
  //    changes), which is how "Resume" re-binds the same pane. When the
  //    process ends the pane keeps what is on screen.
  const pid = session?.pid
  useEffect(() => {
    const handle = handleRef.current
    if (!handle) return
    if (pid === undefined && handle.attached) return
    handle.attached = true
    const { term } = handle

    let disposed = false
    let attached = false
    const pending: Array<[string, number]> = []

    term.reset()
    const unsubscribe = subscribePty(sessionId, (data, seq) => {
      if (attached) term.write(data)
      else pending.push([data, seq])
    })

    void api.sessions.attach(sessionId).then((res) => {
      if (disposed) return
      if (res.snapshot) term.write(res.snapshot)
      for (const [data, seq] of pending) {
        if (seq > res.seq) term.write(data)
      }
      pending.length = 0
      attached = true
      // The PTY was started at a default size; adopt the pane's real size now.
      handle.sent = { cols: res.cols, rows: res.rows }
      syncSize(sessionId, handle)
    })

    return () => {
      disposed = true
      unsubscribe()
    }
  }, [sessionId, pid])

  // 3. Becoming visible: refit and focus.
  useEffect(() => {
    if (!active) return
    const handle = handleRef.current
    if (!handle) return
    const frame = requestAnimationFrame(() => {
      syncSize(sessionId, handle)
      handle.term.focus()
    })
    return () => cancelAnimationFrame(frame)
  }, [active, sessionId])

  const exited = session?.status === 'exited'
  const notice = session?.notice
  const [dismissedNotice, setDismissedNotice] = useState<string | null>(null)

  return (
    <div className="terminal-pane">
      <div ref={hostRef} className="terminal-host" />
      {notice && notice !== dismissedNotice && (
        <div className="terminal-notice">
          <span>{notice}</span>
          <button className="icon-btn" title="Dismiss" onClick={() => setDismissedNotice(notice)}>
            ×
          </button>
        </div>
      )}
      {exited && session && (
        <div className="terminal-overlay">
          <div className="terminal-overlay-card">
            <strong>Process exited</strong>
            {session.exitCode !== undefined && <span className="muted"> (code {session.exitCode})</span>}
            <div className="row" style={{ marginTop: 10 }}>
              <button className="btn primary" onClick={() => void restart(session.id)}>
                {session.kind === 'claude' ? 'Resume conversation' : 'Restart shell'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

/** Fit xterm to its host and tell the PTY, but only when the host is laid out. */
function syncSize(sessionId: string, handle: TermHandle): void {
  const el = handle.term.element?.parentElement
  if (!el || el.offsetWidth === 0 || el.offsetHeight === 0) return
  handle.fit.fit()
  const { cols, rows } = handle.term
  if (cols === handle.sent.cols && rows === handle.sent.rows) return
  handle.sent = { cols, rows }
  api.sessions.resize(sessionId, cols, rows)
}
