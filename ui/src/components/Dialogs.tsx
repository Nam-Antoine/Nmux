import { useEffect, useState } from 'react'
import { pickFolder } from '../api'
import { BINDINGS } from '../keys'
import { getStore, run, update, useStore } from '../store'
import type { Machine, PaneKind, PaneSpec } from '../types'
import { FolderIcon, KIND_LABEL, KindIcon } from './icons'

const KINDS: PaneKind[] = ['claude', 'shell', 'cloud']

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

      {machine && (
        <label>
          <span>Folder on {machine.label}</span>
          <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="~/project or /srv/app" autoFocus spellCheck={false} />
          <span className="muted small">
            {machine.status === 'online' && !machine.tmux
              ? 'tmux is not installed there, so panes stop when the connection drops. Install tmux to keep them running.'
              : 'Panes run there inside tmux and keep going when the connection drops or Nmux closes.'}
            {machine.status === 'online' && !machine.claude && ' Claude Code was not found on that machine.'}
          </span>
        </label>
      )}

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
            Skip permission prompts <span className="muted">(--dangerously-skip-permissions)</span>
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

function AddMachine(): React.JSX.Element {
  const [target, setTarget] = useState('')
  const [label, setLabel] = useState('')
  const [busy, setBusy] = useState(false)
  const ready = target.trim() !== '' && !busy

  const submit = async (): Promise<void> => {
    if (!ready) return
    setBusy(true)
    const before = new Set(getStore().state.machines.map((m) => m.id))
    const reply = await run({ op: 'addMachine', target: target.trim(), label: label.trim() || undefined })
    setBusy(false)
    if (!reply) return
    update({ dialog: null })
    // Try it at once, so the sidebar says whether it can be reached.
    window.setTimeout(() => {
      const added = getStore().state.machines.find((m) => !before.has(m.id))
      if (added) void run({ op: 'checkMachine', machine: added.id })
    }, 150)
  }

  return (
    <form
      className="dialog"
      onSubmit={(e) => {
        e.preventDefault()
        void submit()
      }}
    >
      <h2>Add machine</h2>
      <p className="muted">
        Work on another computer over SSH, next to your local workspaces. Nmux runs your own <code>ssh</code>, with your
        config and keys.
      </p>
      <label>
        <span>SSH target</span>
        <input value={target} onChange={(e) => setTarget(e.target.value)} placeholder="workbox, you@server or ssh://you@server:2222" autoFocus spellCheck={false} />
        <span className="muted small">
          Whatever you type after <code>ssh</code> in a terminal. Check that it connects there first.
        </span>
      </label>
      <label>
        <span>
          Name <span className="muted">(optional)</span>
        </span>
        <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="The host name" maxLength={80} />
      </label>
      <span className="muted small">
        Nothing is installed on the machine. With tmux there, panes keep running when the connection drops. A password
        or key passphrase is asked for inside the pane.
      </span>
      <div className="row end">
        <button type="button" className="btn" onClick={() => update({ dialog: null })}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={!ready}>
          {busy ? 'Adding…' : 'Add'}
        </button>
      </div>
    </form>
  )
}

function Help(): React.JSX.Element {
  return (
    <div className="dialog">
      <h2>Keyboard</h2>
      <p className="muted">Press Ctrl+B, release, then one of these. Everything is also on the mouse.</p>
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
