import { useEffect } from 'react'
import { EmptyState } from './components/EmptyState'
import { NewSessionDialog } from './components/NewSessionDialog'
import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { TerminalPane } from './components/TerminalPane'
import { useSessions } from './store/sessions'

export function App(): React.JSX.Element {
  const init = useSessions((s) => s.init)
  const activeId = useSessions((s) => s.activeId)
  const mountedIds = useSessions((s) => s.mountedIds)
  const dialogOpen = useSessions((s) => s.dialogOpen)
  const openDialog = useSessions((s) => s.openDialog)
  const selectRelative = useSessions((s) => s.selectRelative)

  useEffect(() => init(), [init])

  // Global shortcuts. TerminalPane lets these bubble past xterm.
  useEffect(() => {
    const onKeyDown = (ev: KeyboardEvent): void => {
      if (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === 'n') {
        ev.preventDefault()
        openDialog()
      } else if (ev.ctrlKey && ev.key === 'PageDown') {
        ev.preventDefault()
        selectRelative(1)
      } else if (ev.ctrlKey && ev.key === 'PageUp') {
        ev.preventDefault()
        selectRelative(-1)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [openDialog, selectRelative])

  return (
    <div className="app">
      <Sidebar />
      <main className="workspace">
        {mountedIds.map((id) => (
          <TerminalPane key={id} sessionId={id} active={id === activeId} />
        ))}
        {activeId === null && <EmptyState />}
      </main>
      <StatusBar />
      {dialogOpen && <NewSessionDialog />}
    </div>
  )
}
