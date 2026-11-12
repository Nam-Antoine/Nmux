import { update, useStore } from '../store'
import { Dialogs } from './Dialogs'
import { Main } from './Main'
import { Sidebar } from './Sidebar'

export function App(): React.JSX.Element {
  const sidebarOpen = useStore((s) => s.sidebarOpen)
  const connected = useStore((s) => s.connected)
  const problem = useStore((s) => s.problem)
  const prefix = useStore((s) => s.prefix)
  const toast = useStore((s) => s.toast)
  const toastAction = useStore((s) => s.toastAction)
  return (
    <div className="app">
      {sidebarOpen && <Sidebar />}
      <Main />
      <Dialogs />
      {prefix && <div className="prefix-hint">Ctrl+B … press a key, ? for help</div>}
      {toast && (
        <div className="toast">
          {toast}
          {toastAction && (
            <button
              className="btn small-btn"
              title="Ctrl+Z"
              onClick={() => {
                update({ toast: null, toastAction: null })
                toastAction.run()
              }}
            >
              {toastAction.label}
            </button>
          )}
        </div>
      )}
      {!connected && (
        <div className="offline">
          <strong>Connecting to the Nmux server…</strong>
          {problem && <span className="muted"> {problem}</span>}
        </div>
      )}
    </div>
  )
}
