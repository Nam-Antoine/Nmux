import { useEffect } from 'react'
import { NewSessionDialog } from './components/NewSessionDialog'
import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { Workspace } from './components/Workspace'
import { useSessions } from './store/sessions'

export function App(): React.JSX.Element {
  const init = useSessions((s) => s.init)
  const dialogOpen = useSessions((s) => s.dialogOpen)
  const openDialog = useSessions((s) => s.openDialog)
  const selectRelative = useSessions((s) => s.selectRelative)
  const focusCellRelative = useSessions((s) => s.focusCellRelative)

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
      } else if (ev.ctrlKey && ev.shiftKey && ev.key === 'ArrowRight') {
        ev.preventDefault()
        focusCellRelative(1)
      } else if (ev.ctrlKey && ev.shiftKey && ev.key === 'ArrowLeft') {
        ev.preventDefault()
        focusCellRelative(-1)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [openDialog, selectRelative, focusCellRelative])

  return (
    <div className="app">
      <Sidebar />
      <Workspace />
      <StatusBar />
      {dialogOpen && <NewSessionDialog />}
    </div>
  )
}
