/**
 * The grid of panes. Each pane shows one session (or nothing); the focused
 * pane is the one the sidebar, status bar and shortcuts act on.
 */
import { useMemo } from 'react'
import { groupByFolder } from '../lib/folders'
import { useSessions } from '../store/sessions'
import { EmptyState } from './EmptyState'
import { TerminalPane } from './TerminalPane'

export function Workspace(): React.JSX.Element {
  const layout = useSessions((s) => s.layout)
  const cells = useSessions((s) => s.cells)
  const focusedCell = useSessions((s) => s.focusedCell)
  const multi = cells.length > 1

  return (
    <main
      className={`workspace${multi ? ' multi' : ''}`}
      style={{
        gridTemplateRows: `repeat(${layout.rows}, minmax(0, 1fr))`,
        gridTemplateColumns: `repeat(${layout.cols}, minmax(0, 1fr))`
      }}
    >
      {cells.map((id, i) => (
        <GridCell key={i} index={i} sessionId={id} focused={i === focusedCell} multi={multi} />
      ))}
    </main>
  )
}

function GridCell({
  index,
  sessionId,
  focused,
  multi
}: {
  index: number
  sessionId: string | null
  focused: boolean
  multi: boolean
}): React.JSX.Element {
  const focusCell = useSessions((s) => s.focusCell)
  const assignCell = useSessions((s) => s.assignCell)
  const sessions = useSessions((s) => s.sessions)
  const session = sessionId ? sessions.find((x) => x.id === sessionId) : undefined
  const groups = useMemo(() => groupByFolder(sessions), [sessions])

  return (
    <section
      className={`cell${focused ? ' focused' : ''}`}
      data-cell={index}
      onMouseDownCapture={() => {
        if (!focused) focusCell(index)
      }}
    >
      {multi && (
        <header className="cell-header">
          {session && <span className={`status-dot ${session.status}`} title={session.status} />}
          <select
            className="cell-select"
            value={sessionId ?? ''}
            onChange={(e) => assignCell(index, e.target.value || null)}
            title="Choose which session this pane shows"
          >
            <option value="">— empty pane —</option>
            {groups.map((g) => (
              <optgroup key={g.key} label={g.name}>
                {g.sessions.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                    {s.kind === 'shell' ? ' (shell)' : ''}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          {sessionId && (
            <button className="icon-btn" title="Clear this pane" onClick={() => assignCell(index, null)}>
              ×
            </button>
          )}
        </header>
      )}
      <div className="cell-body">
        {sessionId ? (
          <TerminalPane key={sessionId} sessionId={sessionId} active={focused} />
        ) : multi ? (
          <CellPlaceholder index={index} />
        ) : (
          <EmptyState />
        )}
      </div>
    </section>
  )
}

function CellPlaceholder({ index }: { index: number }): React.JSX.Element {
  const focusCell = useSessions((s) => s.focusCell)
  const openDialog = useSessions((s) => s.openDialog)
  return (
    <div className="cell-empty muted">
      <span>Empty pane</span>
      <span className="small">Pick a session above or click one in the sidebar.</span>
      <button
        className="btn small"
        onClick={() => {
          focusCell(index)
          openDialog()
        }}
      >
        + New session here
      </button>
    </div>
  )
}
