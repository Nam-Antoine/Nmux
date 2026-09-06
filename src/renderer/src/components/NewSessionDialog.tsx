import { useEffect, useMemo, useState, type FormEvent } from 'react'
import type { SessionKind } from '@shared/session'
import { api } from '../api'
import { folderKey, folderName } from '../lib/folders'
import { useSessions } from '../store/sessions'

const LAST_CWD_KEY = 'nmux.lastCwd'
const SKIP_PERMISSIONS_KEY = 'nmux.skipPermissions'
const FOLDERS_DATALIST_ID = 'nmux-known-folders'

function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeLocal(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* storage unavailable: remembering defaults is a convenience only */
  }
}

export function NewSessionDialog(): React.JSX.Element {
  const create = useSessions((s) => s.create)
  const close = useSessions((s) => s.closeDialog)
  const preset = useSessions((s) => s.dialogPreset)
  const sessions = useSessions((s) => s.sessions)

  const [kind, setKind] = useState<SessionKind>('claude')
  const [cwd, setCwd] = useState(() => preset?.cwd ?? readLocal(LAST_CWD_KEY) ?? '')
  const [name, setName] = useState('')
  const [args, setArgs] = useState('')
  const [skipPermissions, setSkipPermissions] = useState(() => readLocal(SKIP_PERMISSIONS_KEY) === '1')
  const [resumeId, setResumeId] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  /** Folders that already have sessions, for the folder field's suggestions. */
  const knownFolders = useMemo(() => {
    const seen = new Map<string, string>()
    for (const s of sessions) if (!seen.has(folderKey(s.cwd))) seen.set(folderKey(s.cwd), s.cwd)
    return [...seen.values()]
  }, [sessions])

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
      writeLocal(LAST_CWD_KEY, cwd.trim())
      writeLocal(SKIP_PERMISSIONS_KEY, skipPermissions ? '1' : '0')
      await create({
        kind,
        cwd: cwd.trim(),
        name: name.trim() || undefined,
        claudeArgs: kind === 'claude' ? splitArgs(args) : [],
        skipPermissions: kind === 'claude' && skipPermissions ? true : undefined,
        resumeClaudeSessionId: kind === 'claude' && resumeId.trim() ? resumeId.trim() : undefined
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setBusy(false)
    }
  }

  const presetName = preset?.cwd ? folderName(preset.cwd) : null

  return (
    <div className="modal-backdrop" onMouseDown={close}>
      <form className="modal" onMouseDown={(e) => e.stopPropagation()} onSubmit={(e) => void submit(e)}>
        <h2>{presetName ? `New session in ${presetName}` : 'New session'}</h2>

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
            <input
              value={cwd}
              onChange={(e) => setCwd(e.target.value)}
              placeholder="C:\path\to\project"
              list={FOLDERS_DATALIST_ID}
              autoFocus={!presetName}
            />
            <button type="button" className="btn" onClick={() => void pickFolder()}>
              Browse…
            </button>
          </div>
          <datalist id={FOLDERS_DATALIST_ID}>
            {knownFolders.map((folder) => (
              <option key={folder} value={folder} />
            ))}
          </datalist>
        </label>

        <label className="field">
          <span className="label">Name (optional)</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Defaults to the folder name"
            autoFocus={Boolean(presetName)}
          />
        </label>

        {kind === 'claude' && (
          <>
            <label className="check">
              <input
                type="checkbox"
                checked={skipPermissions}
                onChange={(e) => setSkipPermissions(e.target.checked)}
              />
              <span className="check-text">
                <span>Skip permission prompts</span>
                <span className="muted small">
                  Runs <code>claude --dangerously-skip-permissions</code>: every tool call is auto-approved.
                  Use only in folders you trust.
                </span>
              </span>
            </label>
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
