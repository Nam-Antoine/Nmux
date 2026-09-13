import { useEffect } from 'react'
import { NewSessionDialog } from './components/NewSessionDialog'
import { Sidebar } from './components/Sidebar'
import { StatusBar } from './components/StatusBar'
import { Workspace } from './components/Workspace'
import { startPtyStream } from './lib/ptyStream'
import { useSessions } from './store/sessions'

export function App(): React.JSX.Element {
  const init = useSessions((s) => s.init)
  const dialogOpen = useSessions((s) => s.dialogOpen)
  const openDialog = useSessions((s) => s.openDialog)
  const selectRelative = useSessions((s) => s.selectRelative)
  const focusCellRelative = useSessions((s) => s.focusCellRelative)
  const sidebarFolded = useSessions((s) => s.sidebarFolded)
  const toggleSidebar = useSessions((s) => s.toggleSidebar)
  const toggleOverview = useSessions((s) => s.toggleOverview)

  useEffect(() => {
    startPtyStream()
    return init()
  }, [init])

  // Global shortcuts. TerminalPane lets these bubble past xterm.
  useEffect(() => {
    const onKeyDown = (ev: KeyboardEvent): void => {
      if (ev.ctrlKey && ev.key.toLowerCase() === 'b') {
        ev.preventDefault()
        toggleSidebar()
      } else if (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === 'o') {
        ev.preventDefault()
        toggleOverview()
      } else if (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === 'n') {
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
  }, [openDialog, selectRelative, focusCellRelative, toggleSidebar, toggleOverview])

  return (
    <div className={`app${sidebarFolded ? ' sidebar-folded' : ''}`}>
      <Sidebar />
      <Workspace />
      <StatusBar />
      {dialogOpen && <NewSessionDialog />}
    </div>
  )
}
