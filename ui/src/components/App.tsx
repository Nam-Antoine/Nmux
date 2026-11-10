import { useStore } from '../store'
import { Dialogs } from './Dialogs'
import { Main } from './Main'
import { Sidebar } from './Sidebar'
import { SidePanel } from './SidePanel'

export function App(): React.JSX.Element {
  const sidebarOpen = useStore((s) => s.sidebarOpen)
  const connected = useStore((s) => s.connected)
  const problem = useStore((s) => s.problem)
  const prefix = useStore((s) => s.prefix)
  const toast = useStore((s) => s.toast)
  return (
    <div className="app">
      {sidebarOpen && <Sidebar />}
      <Main />
      <SidePanel />
      <Dialogs />
      {prefix && <div className="prefix-hint">Ctrl+B … press a key, ? for help</div>}
      {toast && <div className="toast">{toast}</div>}
      {!connected && (
        <div className="offline">
          <strong>Connecting to the Nmux server…</strong>
          {problem && <span className="muted"> {problem}</span>}
        </div>
      )}
    </div>
  )
}
