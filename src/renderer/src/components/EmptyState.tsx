import { useEffect, useState } from 'react'
import type { AppInfo } from '@shared/session'
import { api } from '../api'
import { useSessions } from '../store/sessions'

export function EmptyState(): React.JSX.Element {
  const openDialog = useSessions((s) => s.openDialog)
  const hasSessions = useSessions((s) => s.sessions.length > 0)
  const [info, setInfo] = useState<AppInfo | null>(null)

  useEffect(() => {
    void api.app.info().then(setInfo)
  }, [])

  return (
    <div className="empty">
      <h1>Nmux</h1>
      <p className="muted">
        A persistent home for Claude Code. Sessions keep running when you close this window;
        find it again in the system tray.
      </p>
      <button className="btn primary" onClick={openDialog}>
        New session
      </button>
      {hasSessions && <p className="muted small">…or pick a session from the sidebar.</p>}
      {info && (
        <p className={`small ${info.claudeBinary ? 'muted' : 'error'}`}>
          {info.claudeBinary
            ? `claude: ${info.claudeBinary}`
            : 'claude was not found on PATH. Install Claude Code, then restart Nmux.'}
        </p>
      )}
    </div>
  )
}
