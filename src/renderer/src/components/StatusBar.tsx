import { useMemo, useState } from 'react'
import { activityOf, ago, lastOutput, useNow } from '../lib/activity'
import { MAX_GRID, useSessions } from '../store/sessions'

export function StatusBar(): React.JSX.Element {
  const session = useSessions((s) => s.sessions.find((x) => x.id === s.activeId))
  const [copied, setCopied] = useState(false)
  const now = useNow(1000)
  const activity = session ? activityOf(session) : null
  const last = session ? lastOutput(session.id) : undefined

  const copyResume = (): void => {
    if (!session?.claudeSessionId) return
    void navigator.clipboard.writeText(`claude --resume ${session.claudeSessionId}`)
    setCopied(true)
    setTimeout(() => setCopied(false), 1200)
  }

  return (
    <footer className="statusbar">
      <Counts now={now} />
      <span className="sep" />
      {session ? (
        <>
          <span className={`status-dot ${activity === 'busy' ? 'busy' : session.status === 'running' ? 'running' : 'exited'}`} />
          <span className={activity === 'busy' ? 'ok-text' : undefined}>
            {activity === 'busy'
              ? `working · pid ${session.pid}`
              : activity === 'idle'
                ? `idle${last ? ` · last output ${ago(now - last)}` : ''} · pid ${session.pid}`
                : 'exited'}
          </span>
          {session.skipPermissions && (
            <span className="tag warn" title="Started with --dangerously-skip-permissions">
              skips permissions
            </span>
          )}
          <span className="sep" />
          <span className="muted ellipsis" title={session.cwd}>
            {session.cwd}
          </span>
          {session.claudeSessionId && (
            <>
              <span className="sep" />
              <button className="link" onClick={copyResume} title="Copy the command to resume this conversation in any terminal">
                {copied ? 'copied!' : `session ${session.claudeSessionId.slice(0, 8)}… (copy resume cmd)`}
              </button>
            </>
          )}
        </>
      ) : (
        <span className="muted">No session in this pane</span>
      )}
      <span className="grow" />
      <LayoutPicker />
    </footer>
  )
}

/** Live "working / idle / exited" totals; opens the overview. */
function Counts({ now }: { now: number }): React.JSX.Element {
  const sessions = useSessions((s) => s.sessions)
  const overviewOpen = useSessions((s) => s.overviewOpen)
  const toggleOverview = useSessions((s) => s.toggleOverview)
  const counts = useMemo(() => {
    const c = { busy: 0, idle: 0, exited: 0 }
    for (const s of sessions) c[activityOf(s)]++
    return c
  }, [sessions, now])

  return (
    <button
      className={`counts${overviewOpen ? ' on' : ''}`}
      onClick={toggleOverview}
      title="Session overview (Ctrl+Shift+O)"
      aria-pressed={overviewOpen}
    >
      <span className={`count busy${counts.busy ? ' on' : ''}`}>
        <span className="status-dot busy" />
        {counts.busy} working
      </span>
      <span className={`count idle${counts.idle ? ' on' : ''}`}>
        <span className="status-dot running" />
        {counts.idle} idle
      </span>
      <span className="count exited">
        <span className="status-dot exited" />
        {counts.exited} exited
      </span>
    </button>
  )
}

const PRESETS: Array<[rows: number, cols: number]> = [
  [1, 1],
  [1, 2],
  [2, 1],
  [2, 2],
  [2, 3],
  [3, 3]
]

/** Grid presets plus rows/cols steppers for anything else up to MAX_GRID². */
function LayoutPicker(): React.JSX.Element {
  const layout = useSessions((s) => s.layout)
  const setLayout = useSessions((s) => s.setLayout)

  return (
    <div className="layout-picker" role="group" aria-label="Grid layout">
      {PRESETS.map(([rows, cols]) => {
        const on = layout.rows === rows && layout.cols === cols
        return (
          <button
            key={`${rows}x${cols}`}
            className={`layout-btn${on ? ' on' : ''}`}
            title={`${rows}×${cols} grid`}
            aria-pressed={on}
            onClick={() => setLayout(rows, cols)}
          >
            <span
              className="layout-icon"
              style={{ gridTemplateRows: `repeat(${rows}, 1fr)`, gridTemplateColumns: `repeat(${cols}, 1fr)` }}
            >
              {Array.from({ length: rows * cols }, (_, i) => (
                <i key={i} />
              ))}
            </span>
          </button>
        )
      })}
      <span className="sep" />
      <Stepper label="rows" value={layout.rows} onChange={(v) => setLayout(v, layout.cols)} />
      <Stepper label="cols" value={layout.cols} onChange={(v) => setLayout(layout.rows, v)} />
    </div>
  )
}

function Stepper({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }): React.JSX.Element {
  return (
    <span className="stepper" title={`Grid ${label}`}>
      <button onClick={() => onChange(value - 1)} disabled={value <= 1} aria-label={`fewer ${label}`}>
        −
      </button>
      <span>
        {value} {label}
      </span>
      <button onClick={() => onChange(value + 1)} disabled={value >= MAX_GRID} aria-label={`more ${label}`}>
        +
      </button>
    </span>
  )
}
