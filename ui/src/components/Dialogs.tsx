import { useEffect, useRef, useState } from 'react'
import { call, pickFolder } from '../api'
import { BINDINGS } from '../keys'
import { join, tidy, up } from '../paths'
import { run, update, useStore } from '../store'
import type { DirEntry, Machine, PaneKind, PaneSpec, SshHost } from '../types'
import { Check, FolderIcon, KIND_LABEL, KindIcon, ServerIcon, Up } from './icons'
import { PathInput } from './PathInput'

const KINDS: PaneKind[] = ['claude', 'shell', 'cloud']

function MachineFolder({ machine, cwd, setCwd }: { machine: Machine; cwd: string; setCwd: (cwd: string) => void }): React.JSX.Element {
  const [place, setPlace] = useState('~')
  const [shown, setShown] = useState('')
  const [folders, setFolders] = useState<DirEntry[] | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const latest = useRef(cwd)
  latest.current = cwd

  useEffect(() => {
    let gone = false
    const held = latest.current
    setFolders(null)
    setProblem(null)
    call({ op: 'listDir', path: place, machine: machine.id })
      .then((reply) => {
        if (gone || reply.kind !== 'dir') return
        setFolders(reply.entries.filter((e) => e.dir))
        setShown(reply.path)
        if (latest.current === held) setCwd(reply.path)
      })
      .catch((err) => !gone && setProblem(String(err)))
    return () => {
      gone = true
    }
  }, [place, machine.id, setCwd])

  const typed = tidy(cwd) !== shown
  const go = (path: string): void => {
    if (tidy(path) !== shown) setPlace(tidy(path) || '~')
  }
  return (
    <div className="field">
      <span>Folder on {machine.label}</span>
      <div
        className="row"
        onBlur={() => go(cwd)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && typed && e.target instanceof HTMLInputElement) e.preventDefault()
        }}
      >
        <button
          type="button"
          className="btn"
          title="Up one folder"
          disabled={shown === '' || shown === '/'}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setPlace(up(shown))}
        >
          <Up />
        </button>
        <PathInput machine={machine.id} value={cwd} onChange={setCwd} onGo={go} placeholder="~/project or /srv/app" />
      </div>
      <div className="folder-list">
        {problem && <div className="tree-note">Could not look on {machine.label}: {problem}</div>}
        {!problem && folders === null && <div className="tree-note">Looking on {machine.label}…</div>}
        {folders?.length === 0 && <div className="tree-note">No folders in here.</div>}
        {folders?.map((f) => (
          <div
            key={f.name}
            className="tree-row"
            title={`Open ${f.name}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setPlace(join(shown, f.name))}
          >
            <FolderIcon />
            <span className="tree-name dir">{f.name}</span>
          </div>
        ))}
      </div>
      <span className="muted small">
        The workspace opens in the folder shown above.{' '}
        {machine.status === 'online' && !machine.tmux
          ? 'tmux is not installed there, so panes stop when the connection drops. Install tmux to keep them running.'
          : 'Panes run there inside tmux and keep going when the connection drops or Nmux closes.'}
        {machine.status === 'online' && !machine.claude && ' Claude Code was not found on that machine.'}
      </span>
    </div>
  )
}

function NewPane({ workspace, initialKind, machine }: { workspace?: number; initialKind: PaneKind; machine?: Machine }): React.JSX.Element {
  const [cwd, setCwd] = useState('')
  const [kind, setKind] = useState<PaneKind>(initialKind)
  const [skip, setSkip] = useState(false)
  const [task, setTask] = useState('')
  const [args, setArgs] = useState('')
  const [busy, setBusy] = useState(false)
  const needsFolder = workspace === undefined

  const ready = (!needsFolder || cwd.trim() !== '') && (kind !== 'cloud' || task.trim() !== '') && !busy

  const submit = async (): Promise<void> => {
    if (!ready) return
    setBusy(true)
    const pane: PaneSpec = { kind }
    if (kind === 'claude') pane.skipPermissions = skip
    if (kind === 'cloud') pane.cloudTask = task.trim()
    const extra = args.trim().split(/\s+/).filter(Boolean)
    if (kind !== 'shell' && extra.length > 0) pane.extraArgs = extra
    const reply = needsFolder
      ? await run({ op: 'createWorkspace', cwd: cwd.trim(), pane, machine: machine?.id })
      : await run({ op: 'createTab', workspace, pane })
    setBusy(false)
    if (reply) update({ dialog: null })
  }

  return (
    <form
      className="dialog"
      onSubmit={(e) => {
        e.preventDefault()
        void submit()
      }}
    >
      <h2>{machine ? `New workspace on ${machine.label}` : needsFolder ? 'New workspace' : 'New tab'}</h2>

      {machine && <MachineFolder machine={machine} cwd={cwd} setCwd={setCwd} />}

      {needsFolder && !machine && (
        <label>
          <span>Project folder</span>
          <div className="row">
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="C:\path\to\project" autoFocus spellCheck={false} />
            <button
              type="button"
              className="btn"
              onClick={async () => {
                const picked = await pickFolder()
                if (picked) setCwd(picked)
              }}
            >
              <FolderIcon /> Browse
            </button>
          </div>
        </label>
      )}

      <div className="field">
        <span>Start with</span>
        <div className="segmented">
          {KINDS.filter((k) => !machine || k !== 'cloud').map((k) => (
            <button type="button" key={k} className={k === kind ? 'on' : ''} onClick={() => setKind(k)}>
              <KindIcon kind={k} /> {KIND_LABEL[k]}
            </button>
          ))}
        </div>
      </div>

      {kind === 'cloud' && (
        <label>
          <span>Task</span>
          <textarea
            value={task}
            onChange={(e) => setTask(e.target.value)}
            rows={4}
            maxLength={4000}
            autoFocus={!needsFolder}
            placeholder="What should the cloud session do?"
          />
          <span className="muted small">
            Runs <code>claude --cloud</code> on this folder's GitHub repo and branch. A folder that is not a git repo
            starts an empty session.
          </span>
        </label>
      )}

      {kind === 'claude' && (
        <label className="check">
          <input type="checkbox" checked={skip} onChange={(e) => setSkip(e.target.checked)} />
          <span>
            Skip permission <span className="muted">(--dangerously-skip-permissions)</span>
          </span>
        </label>
      )}

      {kind !== 'shell' && (
        <label>
          <span>
            Extra arguments <span className="muted">(optional)</span>
          </span>
          <input value={args} onChange={(e) => setArgs(e.target.value)} placeholder="--model opus" spellCheck={false} />
        </label>
      )}

      <div className="row end">
        <button type="button" className="btn" onClick={() => update({ dialog: null })}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={!ready}>
          {busy ? 'Starting…' : 'Create'}
        </button>
      </div>
    </form>
  )
}

interface Note {
  text: string
  bad: boolean
}

const AUTH_NOTE = 'Added. It asks for a password or passphrase inside each pane. Ports and Changes need a key that works without one.'

function AddMachine(): React.JSX.Element {
  const machines = useStore((s) => s.state.machines)
  const [hosts, setHosts] = useState<SshHost[] | null>(null)
  const [typed, setTyped] = useState('')
  const [adding, setAdding] = useState<string[]>([])
  const [notes, setNotes] = useState<Record<string, Note>>({})

  useEffect(() => {
    call({ op: 'listSshHosts' })
      .then((reply) => setHosts(reply.kind === 'sshHosts' ? reply.hosts : []))
      .catch(() => setHosts([]))
  }, [])

  const added = (target: string): boolean => machines.some((m) => m.target === target)

  const add = async (raw: string): Promise<void> => {
    const target = raw.trim()
    if (!target || adding.includes(target) || added(target)) return
    const saved = async (): Promise<Machine | undefined> => {
      const reply = await call({ op: 'getState' })
      return reply.kind === 'state' ? reply.state.machines.find((m) => m.target === target) : undefined
    }
    setAdding((list) => [...list, target])
    let note: Note | undefined
    try {
      await call({ op: 'addMachine', target })
      const machine = await saved()
      if (machine) {
        await call({ op: 'checkMachine', machine: machine.id })
        const checked = await saved()
        if (checked?.status === 'auth') note = { text: AUTH_NOTE, bad: false }
        if (checked?.status === 'error') {
          await call({ op: 'removeMachine', machine: machine.id })
          note = {
            text: `Couldn't add ${target}: ${checked.detail ?? 'ssh gave no reason'}. Check that "ssh ${target}" connects in a terminal, then add it again.`,
            bad: true
          }
        }
      }
    } catch (err) {
      note = { text: String(err), bad: true }
    }
    setAdding((list) => list.filter((t) => t !== target))
    setNotes(({ [target]: _, ...rest }) => (note ? { ...rest, [target]: note } : rest))
    if (!note?.bad) setTyped((now) => (now.trim() === target ? '' : now))
  }

  const listed = new Set(hosts?.map((h) => h.alias))
  const message = (target: string): React.JSX.Element | null => {
    const note = notes[target]
    return note ? <div className={`host-note${note.bad ? ' bad' : ''}`}>{note.text}</div> : null
  }

  return (
    <div className="dialog">
      <div className="row">
        <h2 className="grow">Add a machine</h2>
        <button className="esc" onClick={() => update({ dialog: null })}>
          Esc
        </button>
      </div>
      <p className="muted">
        A server, a VPS, a box at home: Nmux reaches it with your own <code>ssh</code>, and nothing is installed there.
        With tmux on it, panes keep running while this computer sleeps.
      </p>
      <div className="field">
        <span>{hosts === null ? 'Reading ~/.ssh/config…' : hosts.length > 0 ? 'From your ssh config' : 'Your ssh config lists no hosts'}</span>
        {hosts !== null && hosts.length > 0 && (
          <div className="host-list">
            {hosts.map((h) => {
              const done = added(h.alias)
              const busy = adding.includes(h.alias)
              return (
                <div key={h.alias}>
                  <div className={`host-row${done || busy ? '' : ' idle'}`} onClick={() => void add(h.alias)}>
                    <ServerIcon />
                    <span className="host-name">{h.alias}</span>
                    <span className="host-sub">{[h.user, h.hostname].filter(Boolean).join('@')}</span>
                    {busy ? (
                      <span className="host-state">
                        <span className="spinner" /> Connecting
                      </span>
                    ) : done ? (
                      <span className="host-state ok">
                        <Check /> Added
                      </span>
                    ) : (
                      <span className="btn">Add</span>
                    )}
                  </div>
                  {message(h.alias)}
                </div>
              )
            })}
          </div>
        )}
      </div>
      <form
        className="field"
        onSubmit={(e) => {
          e.preventDefault()
          void add(typed)
        }}
      >
        <span>Or type how ssh reaches it</span>
        <div className="row">
          <input
            value={typed}
            onChange={(e) => {
              setTyped(e.target.value)
              setNotes((all) => Object.fromEntries(Object.entries(all).filter(([target, note]) => listed.has(target) || !note.bad)))
            }}
            placeholder="user@host"
            autoFocus
            spellCheck={false}
          />
          <button type="submit" className="btn primary" disabled={typed.trim() === '' || adding.includes(typed.trim())}>
            {adding.includes(typed.trim()) ? 'Connecting…' : 'Add'}
          </button>
        </div>
        {Object.keys(notes)
          .filter((target) => !listed.has(target))
          .map((target) => (
            <div key={target}>{message(target)}</div>
          ))}
      </form>
    </div>
  )
}

function Help(): React.JSX.Element {
  return (
    <div className="dialog">
      <h2>Keyboard</h2>
      <p className="muted">
        Press Ctrl+B, release, then one of these. Everything is also on the mouse. Without Ctrl+B: Ctrl+Shift+O splits
        right, Ctrl+Shift+E splits down.
      </p>
      <div className="keys">
        {BINDINGS.map(([key, what]) => (
          <div key={key}>
            <kbd>{key}</kbd>
            <span>{what}</span>
          </div>
        ))}
      </div>
      <div className="row end">
        <button className="btn primary" onClick={() => update({ dialog: null })}>
          Close
        </button>
      </div>
    </div>
  )
}

export function Dialogs(): React.JSX.Element | null {
  const dialog = useStore((s) => s.dialog)
  const machines = useStore((s) => s.state.machines)
  useEffect(() => {
    if (!dialog) return
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') update({ dialog: null })
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [dialog])
  if (!dialog) return null
  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && update({ dialog: null })}>
      {dialog.type === 'help' ? (
        <Help />
      ) : dialog.type === 'machine' ? (
        <AddMachine />
      ) : dialog.type === 'workspace' ? (
        <NewPane initialKind="claude" machine={machines.find((m) => m.id === dialog.machine)} />
      ) : (
        <NewPane workspace={dialog.workspace} initialKind={dialog.kind} />
      )}
    </div>
  )
}
