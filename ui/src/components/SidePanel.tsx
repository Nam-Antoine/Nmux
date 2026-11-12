import { useCallback, useEffect, useState } from 'react'
import { call, openLocalPort } from '../api'
import { activeWorkspace, run, useStore, workspacePanes } from '../store'
import type { ChangedFile, Forward, GitChanges, ListeningPort, Machine } from '../types'
import { Files, insertPath } from './Files'
import { BranchIcon, Chevron, Close, Plus, Refresh } from './icons'

const validPort = (n: number | undefined): boolean => n !== undefined && Number.isInteger(n) && n > 0 && n < 65536

function ForwardRow({ f, label }: { f: Forward; label?: string }): React.JSX.Element {
  const up = f.state === 'active' || f.state === 'starting'
  return (
    <div className="forward">
      <div className="port-row">
        <span className={`dot ${f.state === 'active' ? 'working still' : f.state === 'starting' ? 'working' : f.state === 'failed' ? 'blocked' : 'exited'}`} title={f.state} />
        <span className="port-what" title={`localhost:${f.localPort} → ${f.host}:${f.remotePort}`}>
          <span className="port-num">{f.localPort}</span>
          <span className="muted"> → </span>
          {label ?? f.host}:{f.remotePort}
        </span>
        {f.state === 'active' && (
          <button className="btn small-btn" onClick={() => openLocalPort(f.localPort)}>
            Open
          </button>
        )}
        {up ? (
          <button className="btn small-btn" onClick={() => void run({ op: 'stopForward', id: f.id })}>
            Stop
          </button>
        ) : (
          <button className="btn small-btn" onClick={() => void run({ op: 'startForward', host: f.host, remotePort: f.remotePort, localPort: f.localPort })}>
            Start
          </button>
        )}
        <button className="icon-btn" title="Remove" onClick={() => void run({ op: 'removeForward', id: f.id })}>
          <Close />
        </button>
      </div>
      {f.state === 'failed' && f.error && <div className="forward-error">{f.error}</div>}
    </div>
  )
}

function MachinePorts({ machine }: { machine: Machine }): React.JSX.Element {
  const all = useStore((s) => s.forwards)
  const [ports, setPorts] = useState<ListeningPort[] | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [remote, setRemote] = useState('')
  const [local, setLocal] = useState('')
  const forwards = all.filter((f) => f.host === machine.target)

  const refresh = useCallback(() => {
    call({ op: 'listMachinePorts', machine: machine.id })
      .then((reply) => {
        if (reply.kind !== 'ports') return
        setPorts(reply.ports)
        setProblem(null)
      })
      .catch((err) => setProblem(String(err)))
  }, [machine.id])
  useEffect(() => {
    refresh()
    const timer = window.setInterval(() => !document.hidden && refresh(), 15_000)
    return () => window.clearInterval(timer)
  }, [refresh])

  const remotePort = Number(remote)
  const localPort = local.trim() === '' ? undefined : Number(local)
  const ready = validPort(remotePort) && (localPort === undefined || validPort(localPort))
  const forwardOf = (port: number): Forward | undefined => forwards.find((f) => f.remotePort === port)
  const unlisted = forwards.filter((f) => !ports?.some((p) => p.port === f.remotePort))

  return (
    <div className="ports">
      <div className="panel-section">
        Listening on {machine.label}
        <span className="grow" />
        <button className="icon-btn" title="Look again" onClick={refresh}>
          <Refresh />
        </button>
      </div>
      {ports === null && !problem && <div className="tree-note">Connecting to {machine.label}…</div>}
      {problem && <div className="tree-note">Could not look on {machine.label}: {problem}</div>}
      {ports !== null && ports.length === 0 && <div className="tree-note">Nothing is listening there. Start a dev server in a pane and it shows up here.</div>}
      {ports?.map((p) => {
        const f = forwardOf(p.port)
        return f ? (
          <ForwardRow key={p.port} f={f} label={p.process || machine.label} />
        ) : (
          <div className="port-row" key={p.port}>
            <span className="dot exited" title="Not forwarded" />
            <span className="port-what" title={`${p.process || 'A program'} on ${p.address}:${p.port}`}>
              <span className="port-num">{p.port}</span>
              <span className="muted"> {p.process}</span>
            </span>
            <button
              className="btn small-btn"
              title={`Make ${machine.label}:${p.port} reachable as localhost:${p.port}`}
              onClick={() => void run({ op: 'startForward', host: machine.target, remotePort: p.port })}
            >
              Forward
            </button>
          </div>
        )
      })}

      {unlisted.length > 0 && <div className="panel-section">Other forwards to {machine.label}</div>}
      {unlisted.map((f) => (
        <ForwardRow key={f.id} f={f} label={machine.label} />
      ))}

      <form
        className="forward-form"
        onSubmit={(e) => {
          e.preventDefault()
          if (!ready) return
          void run({ op: 'startForward', host: machine.target, remotePort, localPort }).then((reply) => {
            if (reply) {
              setRemote('')
              setLocal('')
            }
          })
        }}
      >
        <div className="row">
          <input value={remote} onChange={(e) => setRemote(e.target.value)} placeholder={`port on ${machine.label}`} inputMode="numeric" />
          <input value={local} onChange={(e) => setLocal(e.target.value)} placeholder="local port (same)" inputMode="numeric" />
          <button className="btn" type="submit" disabled={!ready} title="Start forwarding">
            <Plus /> Forward
          </button>
        </div>
        <span className="muted small">
          A forwarded port opens as localhost on this computer. Needs a key or agent for {machine.label}: there is nowhere to type a password here.
        </span>
      </form>
    </div>
  )
}

function Ports(): React.JSX.Element {
  const s = useStore((x) => x)
  const [ports, setPorts] = useState<ListeningPort[] | null>(null)
  const [showOther, setShowOther] = useState(false)
  const [host, setHost] = useState('')
  const [remote, setRemote] = useState('')
  const [local, setLocal] = useState('')

  const refresh = useCallback(() => {
    call({ op: 'listPorts' })
      .then((reply) => reply.kind === 'ports' && setPorts(reply.ports))
      .catch(() => {})
  }, [])
  useEffect(() => {
    refresh()
    const timer = window.setInterval(refresh, 3000)
    return () => window.clearInterval(timer)
  }, [refresh])

  const mine = ports?.filter((p) => p.pane !== undefined && here.has(p.pane)) ?? []
  const other = ports?.filter((p) => p.pane === undefined || !here.has(p.pane)) ?? []
  const remotePort = Number(remote)
  const localPort = local.trim() === '' ? undefined : Number(local)
  const ready = host.trim() !== '' && validPort(remotePort) && (localPort === undefined || validPort(localPort))
  const ws = activeWorkspace(s)
  const here = new Set(ws ? workspacePanes(ws) : [])
  const label = (host: string): string | undefined => s.state.machines.find((m) => m.target === host)?.label

  const row = (p: ListeningPort): React.JSX.Element => (
    <div className="port-row" key={`${p.port}-${p.pid}`}>
      <span className="port-num">{p.port}</span>
      <span className="port-what" title={`${p.process} (pid ${p.pid}) on ${p.address}`}>
        {p.pane !== undefined ? (s.panes.get(p.pane)?.name ?? p.process) : p.process || `pid ${p.pid}`}
        <span className="muted"> {p.pane !== undefined ? p.process : ''}</span>
      </span>
      {p.pane !== undefined && (
        <button className="btn small-btn" title="Go to the pane that opened this port" onClick={() => void run({ op: 'focusPane', pane: p.pane! })}>
          Pane
        </button>
      )}
      <button className="btn small-btn" title={`Open http://localhost:${p.port}`} onClick={() => openLocalPort(p.port)}>
        Open
      </button>
    </div>
  )

  return (
    <div className="ports">
      <div className="panel-section">From {activeWorkspace(s)?.name ?? 'this workspace'}</div>
      {ports === null && <div className="tree-note">Loading…</div>}
      {ports !== null && mine.length === 0 && (
        <div className="tree-note">No pane in this workspace is listening on a port. Start a dev server in a pane and it shows up here.</div>
      )}
      {mine.map(row)}

      {other.length > 0 && (
        <>
          <div className="panel-section clickable" onClick={() => setShowOther(!showOther)}>
            <Chevron open={showOther} /> Other programs on this computer <span className="count">{other.length}</span>
          </div>
          {showOther && other.map(row)}
        </>
      )}

      <div className="panel-section">SSH port forwards</div>
      {s.forwards.length === 0 && (
        <div className="tree-note">Reach a port on another machine as if it were on this one. Uses your own ssh and its config.</div>
      )}
      {s.forwards.map((f) => (
        <ForwardRow key={f.id} f={f} label={label(f.host)} />
      ))}

      <form
        className="forward-form"
        onSubmit={(e) => {
          e.preventDefault()
          if (!ready) return
          void run({ op: 'startForward', host: host.trim(), remotePort, localPort }).then((reply) => {
            if (reply) {
              setRemote('')
              setLocal('')
            }
          })
        }}
      >
        <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="ssh host (alias or user@host)" spellCheck={false} />
        <div className="row">
          <input value={remote} onChange={(e) => setRemote(e.target.value)} placeholder="remote port" inputMode="numeric" />
          <input value={local} onChange={(e) => setLocal(e.target.value)} placeholder="local port (same)" inputMode="numeric" />
          <button className="btn" type="submit" disabled={!ready} title="Start forwarding">
            <Plus /> Forward
          </button>
        </div>
        <span className="muted small">Key-based hosts only: there is nowhere to type a password.</span>
      </form>
    </div>
  )
}

const STATUS_LETTER: Record<ChangedFile['status'], string> = { modified: 'M', added: 'A', deleted: 'D', renamed: 'R', untracked: 'U' }

function Diff({ workspace, file, onClose }: { workspace: number; file: ChangedFile; onClose: () => void }): React.JSX.Element {
  const [diff, setDiff] = useState<{ text: string; truncated: boolean } | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let gone = false
    setDiff(null)
    setError(null)
    call({ op: 'gitDiff', workspace, path: file.path, untracked: file.status === 'untracked' })
      .then((reply) => !gone && reply.kind === 'diff' && setDiff({ text: reply.diff, truncated: reply.truncated }))
      .catch((err) => !gone && setError(String(err)))
    return () => {
      gone = true
    }
  }, [workspace, file.path, file.status, file.added, file.removed])

  const lines = diff?.text.split('\n') ?? []
  const start = Math.max(0, lines.findIndex((l) => l.startsWith('@@')))
  return (
    <div className="preview">
      <div className="preview-head">
        <span className="preview-name" title={file.path}>{file.path.split('/').pop()}</span>
        <span className="grow" />
        <button className="btn small-btn" title="Type this path into the focused pane" onClick={() => insertPath(file.path)}>
          Insert path
        </button>
        <button className="icon-btn" title="Close" onClick={onClose}>
          <Close />
        </button>
      </div>
      <div className="preview-body">
        {error && <div className="tree-note">{error}</div>}
        {!diff && !error && <div className="tree-note">Loading…</div>}
        {diff && lines.length <= 1 && <div className="tree-note">No text changes to show.</div>}
        {diff && lines.length > 1 && (
          <pre className="diff">
            {lines.slice(start).map((line, i) => (
              <span key={i} className={line.startsWith('@@') ? 'hunk' : line.startsWith('+') ? 'plus' : line.startsWith('-') ? 'minus' : undefined}>
                {line + '\n'}
              </span>
            ))}
            {diff.truncated && '\n… (only the start of this diff is shown)'}
          </pre>
        )}
      </div>
    </div>
  )
}

function Changes(): React.JSX.Element {
  const ws = useStore(activeWorkspace)
  const [changes, setChanges] = useState<GitChanges | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const id = ws?.id
  const remote = ws?.machine !== undefined

  const refresh = useCallback(() => {
    if (id === undefined) return
    call({ op: 'gitChanges', workspace: id })
      .then((reply) => {
        if (reply.kind !== 'changes') return
        setChanges(reply.changes)
        setProblem(null)
      })
      .catch((err) => setProblem(String(err)))
  }, [id])
  useEffect(() => {
    setChanges(null)
    setSelected(null)
    refresh()
    const timer = window.setInterval(() => !document.hidden && refresh(), remote ? 15_000 : 4000)
    return () => window.clearInterval(timer)
  }, [refresh, remote])

  if (id === undefined) return <div className="tree-note">Open a workspace to see its changes.</div>
  const file = changes?.files.find((f) => f.path === selected)
  return (
    <>
      <div className="panel-section">
        <BranchIcon />
        <span className="branch">{changes?.branch ?? (problem ? 'No repository' : '…')}</span>
        {changes && changes.ahead > 0 && <span className="muted">↑{changes.ahead}</span>}
        {changes && changes.behind > 0 && <span className="muted">↓{changes.behind}</span>}
        <span className="grow" />
        {changes && <span className="count">{changes.files.length}</span>}
        <button className="icon-btn" title="Look again" onClick={refresh}>
          <Refresh />
        </button>
      </div>
      <div className="tree">
        {problem && <div className="tree-note">{problem}</div>}
        {!problem && changes === null && <div className="tree-note">Loading…</div>}
        {!problem && changes?.files.length === 0 && <div className="tree-note">No changes. The working tree matches the last commit.</div>}
        {!problem &&
          changes?.files.map((f) => {
            const cut = f.path.lastIndexOf('/') + 1
            return (
              <div key={f.path} className={`tree-row change${f.path === selected ? ' active' : ''}`} title={`${f.status}: ${f.path}`} onClick={() => setSelected(f.path === selected ? null : f.path)}>
                <span className={`change-status ${f.status}`}>{STATUS_LETTER[f.status]}</span>
                <span className="change-name">{f.path.slice(cut)}</span>
                <span className="change-dir">{f.path.slice(0, cut)}</span>
                {f.added !== undefined && f.added > 0 && <span className="plus">+{f.added}</span>}
                {f.removed !== undefined && f.removed > 0 && <span className="minus">−{f.removed}</span>}
              </div>
            )
          })}
      </div>
      {file && <Diff workspace={id} file={file} onClose={() => setSelected(null)} />}
    </>
  )
}

export function SidePanel(): React.JSX.Element | null {
  const panel = useStore((s) => s.panel)
  const machine = useStore((s) => s.state.machines.find((m) => m.id === activeWorkspace(s)?.machine))
  if (!panel) return null
  return (
    <aside className="side-panel">
      <div className="panel-view" key={panel}>
        {panel === 'files' ? <Files /> : panel === 'changes' ? <Changes /> : machine ? <MachinePorts key={machine.id} machine={machine} /> : <Ports />}
      </div>
    </aside>
  )
}
