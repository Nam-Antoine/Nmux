import { useState } from 'react'
import { useSessions } from '../store/sessions'

export function StatusBar(): React.JSX.Element {
  const session = useSessions((s) => s.sessions.find((x) => x.id === s.activeId))
  const [copied, setCopied] = useState(false)

  if (!session) {
    return <footer className="statusbar muted">No session selected</footer>
  }

  const copyResume = (): void => {
    if (!session.claudeSessionId) return
    void navigator.clipboard.writeText(`claude --resume ${session.claudeSessionId}`)
    setCopied(true)
    setTimeout(() => setCopied(false), 1200)
  }

  return (
    <footer className="statusbar">
      <span className={`status-dot ${session.status}`} />
      <span>{session.status === 'running' ? `running · pid ${session.pid}` : 'exited'}</span>
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
    </footer>
  )
}
