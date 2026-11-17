import { Component } from 'react'
import { note } from '../api'

const RELOAD_AGAIN_AFTER_MS = 60_000

function reloadedLately(): boolean {
  try {
    const last = Number(sessionStorage.getItem('nmux-reloaded') ?? 0)
    sessionStorage.setItem('nmux-reloaded', String(Date.now()))
    return Date.now() - last < RELOAD_AGAIN_AFTER_MS
  } catch {
    return true
  }
}

export class Guard extends Component<{ children: React.ReactNode }, { problem: string | null }> {
  state = { problem: null }

  static getDerivedStateFromError(err: unknown): { problem: string } {
    return { problem: String(err) }
  }

  componentDidCatch(err: unknown, info: React.ErrorInfo): void {
    note(`render failed: ${err instanceof Error ? (err.stack ?? err.message) : String(err)}${info.componentStack ?? ''}`)
    if (!reloadedLately()) location.reload()
  }

  render(): React.ReactNode {
    if (this.state.problem === null) return this.props.children
    return (
      <div className="backdrop">
        <div className="dialog">
          <h2>The window hit a problem</h2>
          <p className="muted">Your panes are still running. Reloading brings the window back.</p>
          <code>{this.state.problem}</code>
          <div className="row end">
            <button className="btn primary" onClick={() => location.reload()}>
              Reload
            </button>
          </div>
        </div>
      </div>
    )
  }
}
