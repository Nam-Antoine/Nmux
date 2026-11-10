import { useCallback, useEffect, useState } from 'react'
import { call, openLocalPort, openPath, revealPath } from '../api'
import { activeTab, activeWorkspace, getStore, run, toast, update, useStore, workspacePanes } from '../store'
import { terminals } from '../terminals'
import type { DirEntry, FilePreview, Forward, ListeningPort, Machine } from '../types'
import { Chevron, Close, FolderIcon, Plus, Refresh } from './icons'

function join(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return dir.endsWith(sep) ? dir + name : dir + sep + name
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function insertPath(path: string): void {
  const pane = activeTab(getStore())?.focused
  if (pane === undefined) return toast('No pane to insert into.')
  terminals.paste(pane, /\s/.test(path) ? `"${path}" ` : `${path} `)
  terminals.focus(pane)
}

interface TreeProps {
  path: string
  depth: number
  filter: string
  version: number
  selected: string | null
  onSelect: (path: string) => void
}

function TreeDir({ path, depth, filter, version, selected, onSelect }: TreeProps): React.JSX.Element {
  const [entries, setEntries] = useState<DirEntry[] | null>(null)
  const [open, setOpen] = useState<Record<string, boolean>>({})
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let gone = false
    call({ op: 'listDir', path })
      .then((reply) => {
        if (!gone && reply.kind === 'dir') {
          setEntries(reply.entries)
          setError(null)
        }
      })
      .catch((err) => !gone && setError(String(err)))
    return () => {
      gone = true
    }
  }, [path, version])

  if (error) return <div className="tree-note" style={{ paddingLeft: depth * 14 + 26 }}>{error}</div>
  if (!entries) return <div className="tree-note" style={{ paddingLeft: depth * 14 + 26 }}>Loading…</div>
  const shown = filter ? entries.filter((e) => e.dir || e.name.toLowerCase().includes(filter)) : entries
  if (shown.length === 0 && depth === 0) return <div className="tree-note">Nothing here.</div>

  return (
    <>
      {shown.map((entry) => {
        const full = join(path, entry.name)
        const expanded = open[entry.name] ?? false
        return (
          <div key={entry.name}>
            <div
              className={`tree-row${full === selected ? ' active' : ''}`}
              style={{ paddingLeft: depth * 14 + 8 }}
              title={full}
              onClick={() => (entry.dir ? setOpen({ ...open, [entry.name]: !expanded }) : onSelect(full))}
            >
              {entry.dir ? <Chevron open={expanded} /> : <span className="tree-spacer" />}
              {entry.dir && <FolderIcon />}
              <span className={`tree-name${entry.dir ? ' dir' : ''}`}>{entry.name}</span>
              <button
                className="icon-btn tree-insert"
                title="Type this path into the focused pane"
                onClick={(e) => {
                  e.stopPropagation()
                  insertPath(full)
                }}
              >
                @
              </button>
            </div>
            {entry.dir && expanded && (
              <TreeDir path={full} depth={depth + 1} filter={filter} version={version} selected={selected} onSelect={onSelect} />
            )}
          </div>
        )
      })}
    </>
  )
}

function Preview({ path, onClose }: { path: string; onClose: () => void }): React.JSX.Element {
  const [file, setFile] = useState<FilePreview | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let gone = false
    setFile(null)
    setError(null)
    call({ op: 'readFile', path })
      .then((reply) => !gone && reply.kind === 'file' && setFile(reply.file))
      .catch((err) => !gone && setError(String(err)))
    return () => {
      gone = true
    }
  }, [path])

  const name = path.split(/[\\/]/).pop() ?? path
  const content = file?.content
  return (
    <div className="preview">
      <div className="preview-head">
        <span className="preview-name" title={path}>{name}</span>
        {file && <span className="muted small">{size(file.size)}</span>}
        <span className="grow" />
        <button className="btn small-btn" title="Type this path into the focused pane" onClick={() => insertPath(path)}>
          Insert path
        </button>
        <button className="btn small-btn" title="Open with the default program" onClick={() => openPath(path).catch((e) => toast(String(e)))}>
          Open
        </button>
        <button className="btn small-btn" title="Show in the file manager" onClick={() => revealPath(path).catch((e) => toast(String(e)))}>
          Reveal
        </button>
        <button className="icon-btn" title="Close preview" onClick={onClose}>
          <Close />
        </button>
      </div>
      <div className="preview-body">
        {error && <div className="tree-note">{error}</div>}
        {!file && !error && <div className="tree-note">Loading…</div>}
        {content?.type === 'text' && (
          <pre>
            {content.text}
            {content.truncated && '\n\n… (only the start of this file is shown)'}
          </pre>
        )}
        {content?.type === 'image' && <img src={`data:${content.mime};base64,${content.data}`} alt={name} />}
        {content?.type === 'binary' && <div className="tree-note">This is not a text file. Use Open.</div>}
        {content?.type === 'tooLarge' && <div className="tree-note">Too large to preview. Use Open.</div>}
      </div>
    </div>
  )
}

function Files(): React.JSX.Element {
  const remote = useStore((s) => activeWorkspace(s)?.machine !== undefined)
  const root = useStore((s) => activeWorkspace(s)?.cwd)
  const [filter, setFilter] = useState('')
  const [version, setVersion] = useState(0)
  const [selected, setSelected] = useState<string | null>(null)

  useEffect(() => setSelected(null), [root])

  if (!root) return <div className="tree-note">Open a workspace to see its files.</div>
  if (remote) return <div className="tree-note">This workspace is on another machine. Files are shown for workspaces on this computer.</div>
  return (
    <>
      <div className="panel-tools">
        <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter files" spellCheck={false} />
        <button className="icon-btn" title="Refresh" onClick={() => setVersion(version + 1)}>
          <Refresh />
        </button>
      </div>
      <div className="tree" key={root}>
        <TreeDir path={root} depth={0} filter={filter.trim().toLowerCase()} version={version} selected={selected} onSelect={setSelected} />
      </div>
      {selected && <Preview path={selected} onClose={() => setSelected(null)} />}
    </>
  )
}

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

export function SidePanel(): React.JSX.Element | null {
  const panel = useStore((s) => s.panel)
  const machine = useStore((s) => s.state.machines.find((m) => m.id === activeWorkspace(s)?.machine))
  if (!panel) return null
  return (
    <aside className="side-panel">
      <div className="panel-head">
        <div className="segmented small-seg">
          <button className={panel === 'files' ? 'on' : ''} onClick={() => update({ panel: 'files' })}>
            Files
          </button>
          <button className={panel === 'ports' ? 'on' : ''} onClick={() => update({ panel: 'ports' })}>
            Ports
          </button>
        </div>
        <span className="grow" />
        <button className="icon-btn" title="Close panel" onClick={() => update({ panel: null })}>
          <Close />
        </button>
      </div>
      {panel === 'files' ? <Files /> : machine ? <MachinePorts key={machine.id} machine={machine} /> : <Ports />}
    </aside>
  )
}
