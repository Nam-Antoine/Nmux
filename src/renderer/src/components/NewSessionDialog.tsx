import { useEffect, useState, type FormEvent } from 'react'
import type { SessionKind } from '@shared/session'
import { api } from '../api'
import { useSessions } from '../store/sessions'

const LAST_CWD_KEY = 'nmux.lastCwd'

export function NewSessionDialog(): React.JSX.Element {
  const create = useSessions((s) => s.create)
  const close = useSessions((s) => s.closeDialog)

  const [kind, setKind] = useState<SessionKind>('claude')
  const [cwd, setCwd] = useState(() => localStorage.getItem(LAST_CWD_KEY) ?? '')
  const [name, setName] = useState('')
  const [args, setArgs] = useState('')
  const [resumeId, setResumeId] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [close])

  const pickFolder = async (): Promise<void> => {
    const folder = await api.dialog.pickFolder()
    if (folder) setCwd(folder)
  }

  const submit = async (e: FormEvent): Promise<void> => {
    e.preventDefault()
    if (!cwd.trim()) {
      setError('Choose a project folder.')
      return
    }
    setBusy(true)
    setError(null)
    try {
      localStorage.setItem(LAST_CWD_KEY, cwd.trim())
      await create({
        kind,
        cwd: cwd.trim(),
        name: name.trim() || undefined,
        claudeArgs: kind === 'claude' ? splitArgs(args) : [],
        resumeClaudeSessionId: kind === 'claude' && resumeId.trim() ? resumeId.trim() : undefined
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setBusy(false)
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <form className="modal" onMouseDown={(e) => e.stopPropagation()} onSubmit={(e) => void submit(e)}>
        <h2>New session</h2>

        <div className="field">
          <span className="label">Type</span>
          <div className="segmented">
            <button type="button" className={kind === 'claude' ? 'on' : ''} onClick={() => setKind('claude')}>
              Claude Code
            </button>
            <button type="button" className={kind === 'shell' ? 'on' : ''} onClick={() => setKind('shell')}>
              Shell
            </button>
          </div>
        </div>

        <label className="field">
          <span className="label">Project folder</span>
          <div className="row">
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="C:\path\to\project" autoFocus />
            <button type="button" className="btn" onClick={() => void pickFolder()}>
              Browse…
            </button>
          </div>
        </label>

        <label className="field">
          <span className="label">Name (optional)</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Defaults to the folder name" />
        </label>

        {kind === 'claude' && (
          <>
            <label className="field">
              <span className="label">Extra claude args (optional)</span>
              <input value={args} onChange={(e) => setArgs(e.target.value)} placeholder="--model opus --add-dir ../shared" />
            </label>
            <label className="field">
              <span className="label">Resume existing Claude session ID (optional)</span>
              <input value={resumeId} onChange={(e) => setResumeId(e.target.value)} placeholder="uuid from a previous session" />
            </label>
          </>
        )}

        {error && <div className="error">{error}</div>}

        <div className="row end" style={{ marginTop: 6 }}>
          <button type="button" className="btn" onClick={close} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="btn primary" disabled={busy}>
            {busy ? 'Starting…' : 'Start'}
          </button>
        </div>
      </form>
    </div>
  )
}

/** Minimal shell-style splitting: whitespace separated, double quotes group. */
function splitArgs(input: string): string[] {
  const out: string[] = []
  const re = /"([^"]*)"|(\S+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(input)) !== null) out.push(m[1] ?? m[2] ?? '')
  return out
}
