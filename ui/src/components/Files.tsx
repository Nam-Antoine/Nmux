import { useEffect, useRef, useState } from 'react'
import { call, onFileDrag, onFileDrop, openPath, pickUploads, revealPath, sure } from '../api'
import { join, tail, tidy, up } from '../paths'
import { activeTab, activeWorkspace, getStore, run, toast, useStore } from '../store'
import { terminals } from '../terminals'
import type { DirEntry, FilePreview } from '../types'
import { ContextMenu } from './ContextMenu'
import { InlineEdit } from './InlineEdit'
import { PathInput } from './PathInput'
import { Chevron, Close, Copy, Download, External, FolderIcon, Insert, NewFile, NewFolder, Pencil, Refresh, Trash, Up, Upload } from './icons'

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

export function insertPath(path: string): void {
  const pane = activeTab(getStore())?.focused
  if (pane === undefined) return toast('No pane to insert into.')
  terminals.paste(pane, /\s/.test(path) ? `"${path}" ` : `${path} `)
  terminals.focus(pane)
}

interface Target {
  path: string
  name: string
  dir: boolean
  parent: string
}

type Edit = { dir: string; folder: boolean } | { rename: Target }

interface Tree {
  machine?: number
  filter: string
  version: number
  touched: Record<string, number>
  open: Record<string, boolean>
  selected: string | null
  dropAt: string | null
  edit: Edit | null
  toggle: (path: string) => void
  select: (path: string) => void
  menu: (e: React.MouseEvent, target: Target) => void
  rename: (target: Target) => void
  remove: (target: Target) => void
  finish: (name: string | null) => void
}

function TreeDir({ path, depth, tree }: { path: string; depth: number; tree: Tree }): React.JSX.Element {
  const [entries, setEntries] = useState<DirEntry[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const { machine, edit } = tree
  const stamp = tree.version + (tree.touched[path] ?? 0)

  useEffect(() => {
    let gone = false
    call({ op: 'listDir', path, machine })
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
  }, [path, machine, stamp])

  if (error) return <div className="tree-note" style={{ paddingLeft: depth * 14 + 26 }}>{error}</div>
  if (!entries) return <div className="tree-note" style={{ paddingLeft: depth * 14 + 26 }}>Loading…</div>
  const shown = tree.filter ? entries.filter((e) => e.dir || e.name.toLowerCase().includes(tree.filter)) : entries
  const adding = edit && 'dir' in edit && edit.dir === path ? edit : null

  return (
    <>
      {adding && (
        <div className="tree-row" style={{ paddingLeft: depth * 14 + 8 }}>
          <span className="tree-spacer" />
          {adding.folder && <FolderIcon />}
          <InlineEdit value="" maxLength={255} onDone={tree.finish} />
        </div>
      )}
      {shown.length === 0 && depth === 0 && !adding && <div className="tree-note">Nothing here.</div>}
      {shown.map((entry) => {
        const full = join(path, entry.name)
        const target: Target = { path: full, name: entry.name, dir: entry.dir, parent: path }
        const expanded = tree.open[full] ?? false
        const act = (): void => (entry.dir ? tree.toggle(full) : tree.select(full))
        return (
          <div key={entry.name}>
            <div
              className={`tree-row${full === tree.selected ? ' active' : ''}${entry.dir && full === tree.dropAt ? ' drop' : ''}`}
              style={{ paddingLeft: depth * 14 + 8 }}
              title={full}
              tabIndex={0}
              data-drop={entry.dir ? full : path}
              onClick={act}
              onContextMenu={(e) => tree.menu(e, target)}
              onKeyDown={(e) => {
                if (e.target !== e.currentTarget) return
                if (e.key === 'Enter') act()
                else if (e.key === 'F2') tree.rename(target)
                else if (e.key === 'Delete') tree.remove(target)
              }}
            >
              {entry.dir ? <Chevron open={expanded} /> : <span className="tree-spacer" />}
              {entry.dir && <FolderIcon />}
              {edit && 'rename' in edit && edit.rename.path === full ? (
                <InlineEdit value={entry.name} maxLength={255} onDone={tree.finish} />
              ) : (
                <span className={`tree-name${entry.dir ? ' dir' : ''}`}>{entry.name}</span>
              )}
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
            {entry.dir && expanded && <TreeDir path={full} depth={depth + 1} tree={tree} />}
          </div>
        )
      })}
    </>
  )
}

function Preview({ path, machine, onClose }: { path: string; machine?: number; onClose: () => void }): React.JSX.Element {
  const [file, setFile] = useState<FilePreview | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let gone = false
    setFile(null)
    setError(null)
    call({ op: 'readFile', path, machine })
      .then((reply) => !gone && reply.kind === 'file' && setFile(reply.file))
      .catch((err) => !gone && setError(String(err)))
    return () => {
      gone = true
    }
  }, [path, machine])

  const name = tail(path)
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
        {machine === undefined && (
          <>
            <button className="btn small-btn" title="Open with the default program" onClick={() => openPath(path).catch((e) => toast(String(e)))}>
              Open
            </button>
            <button className="btn small-btn" title="Show in the file manager" onClick={() => revealPath(path).catch((e) => toast(String(e)))}>
              Reveal
            </button>
          </>
        )}
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
        {content?.type === 'binary' && <div className="tree-note">This is not a text file.{machine === undefined && ' Use Open.'}</div>}
        {content?.type === 'tooLarge' && <div className="tree-note">Too large to preview.{machine === undefined && ' Use Open.'}</div>}
      </div>
    </div>
  )
}

export function Files(): React.JSX.Element {
  const machine = useStore((s) => activeWorkspace(s)?.machine)
  const label = useStore((s) => s.state.machines.find((m) => m.id === activeWorkspace(s)?.machine)?.label)
  const root = useStore((s) => activeWorkspace(s)?.cwd)
  const [place, setPlace] = useState(root)
  const [typed, setTyped] = useState(root ?? '')
  const [filter, setFilter] = useState('')
  const [version, setVersion] = useState(0)
  const [touched, setTouched] = useState<Record<string, number>>({})
  const [open, setOpen] = useState<Record<string, boolean>>({})
  const [selected, setSelected] = useState<string | null>(null)
  const [edit, setEdit] = useState<Edit | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; target: Target | null } | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<string | null>(null)
  const editing = useRef<Edit | null>(null)

  const startEdit = (next: Edit | null): void => {
    editing.current = next
    setEdit(next)
  }

  const go = (path: string): void => {
    setPlace(path)
    setTyped(path)
    setSelected(null)
    startEdit(null)
  }

  const refresh = (dir: string): void => setTouched((t) => ({ ...t, [dir]: (t[dir] ?? 0) + 1 }))
  const expand = (dir: string): void => setOpen((o) => ({ ...o, [dir]: true }))
  const forget = (path: string): void => setSelected((s) => (s !== null && (s === path || s.startsWith(join(path, ''))) ? null : s))

  const during = <T,>(note: string, work: Promise<T>): Promise<T> => {
    setBusy(note)
    return work.finally(() => setBusy((b) => (b === note ? null : b)))
  }

  const upload = (sources: string[], to: string): void => {
    if (sources.length === 0) return
    const what = sources.length === 1 ? tail(sources[0]!) : `${sources.length} items`
    const send = (overwrite: boolean): Promise<unknown> => call({ op: 'uploadPaths', sources, to, overwrite, machine })
    const work = send(false).catch(async (err) => {
      if (!String(err).includes('already exists')) throw err
      const ask = `In ${to}, ${String(err)}\n\nUpload anyway? Folders are merged, and files with the same name are replaced.`
      return (await sure(ask)) ? send(true) : null
    })
    during(`Uploading ${what} to ${to}…`, work)
      .then((done) => {
        if (!done) return
        toast(`Uploaded ${what} to ${to}.`)
        expand(to)
        refresh(to)
      })
      .catch((err) => toast(String(err)))
  }

  useEffect(() => {
    if (!root) return
    let gone = false
    go(root)
    if (machine !== undefined && !root.startsWith('/')) {
      call({ op: 'listDir', path: root, machine })
        .then((reply) => !gone && reply.kind === 'dir' && go(reply.path))
        .catch(() => undefined)
    }
    return () => {
      gone = true
    }
  }, [root, machine])

  useEffect(() => {
    const under = (x: number, y: number): string | undefined =>
      (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>('[data-drop]')?.dataset.drop
    const stops = [
      onFileDrag((at) => setDropAt((at && under(at.x, at.y)) ?? null)),
      onFileDrop((paths, x, y) => {
        const to = under(x, y)
        if (to) upload(paths, to)
      })
    ]
    return () => stops.forEach((stop) => void stop.then((off) => off()))
  }, [machine])

  if (!root || !place) return <div className="tree-note">Open a workspace to see its files.</div>

  const pick = (to: string, folders: boolean): void =>
    void pickUploads(folders)
      .then((paths) => upload(paths, to))
      .catch((err) => toast(String(err)))

  const create = (dir: string, folder: boolean): void => {
    expand(dir)
    startEdit({ dir, folder })
  }

  const finish = (name: string | null): void => {
    const was = editing.current
    startEdit(null)
    if (!was || !name) return
    if (/[\\/]/.test(name)) return toast('A name cannot contain / or \\.')
    if ('rename' in was) {
      const { path, parent } = was.rename
      if (name === was.rename.name) return
      void run({ op: 'renamePath', from: path, to: join(parent, name), machine }).then((ok) => {
        if (!ok) return
        forget(path)
        refresh(parent)
      })
    } else {
      void run({ op: was.folder ? 'makeDir' : 'makeFile', path: join(was.dir, name), machine }).then((ok) => ok && refresh(was.dir))
    }
  }

  const rename = (target: Target): void => startEdit({ rename: target })

  const remove = (target: Target): void => {
    const what = `"${target.name}"${target.dir ? ' and everything inside it' : ''}`
    const ask = machine === undefined ? `Move ${what} to the Recycle Bin?` : `Delete ${what} from ${label ?? 'the machine'}? This cannot be undone.`
    void sure(ask)
      .then((yes) => (yes ? run({ op: 'removePath', path: target.path, machine }) : null))
      .then((ok) => {
        if (!ok) return
        forget(target.path)
        refresh(target.parent)
      })
  }

  const download = (path: string): void => {
    if (machine === undefined) return
    void during(`Downloading ${tail(path)}…`, run({ op: 'downloadPath', path, machine })).then((reply) => reply?.kind === 'path' && toast(`Saved to ${reply.path}`))
  }

  const tree: Tree = {
    machine,
    filter: filter.trim().toLowerCase(),
    version,
    touched,
    open,
    selected,
    dropAt,
    edit,
    toggle: (path) => setOpen({ ...open, [path]: !open[path] }),
    select: setSelected,
    menu: (e, target) => {
      e.preventDefault()
      e.stopPropagation()
      setMenu({ x: e.clientX, y: Math.min(e.clientY, window.innerHeight - 290), target })
    },
    rename,
    remove,
    finish
  }

  const target = menu?.target
  const into = target ? (target.dir ? target.path : target.parent) : place
  return (
    <div className="files" data-drop={place}>
      {machine !== undefined && (
        <div className="panel-tools">
          <button
            className="icon-btn"
            title="Up one folder"
            disabled={!place.startsWith('/') || place === '/'}
            onClick={() => go(up(place))}
          >
            <Up />
          </button>
          <PathInput machine={machine} value={typed} onChange={setTyped} onGo={(path) => tidy(path) && go(tidy(path))} placeholder="Any folder on this machine" />
        </div>
      )}
      <div className="panel-tools">
        <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter files" spellCheck={false} />
        <button className="icon-btn" title="New file here" onClick={() => create(place, false)}>
          <NewFile />
        </button>
        <button className="icon-btn" title="New folder here" onClick={() => create(place, true)}>
          <NewFolder />
        </button>
        <button className="icon-btn" title="Upload files into this folder. Right-click the list to upload a folder, or drop files onto it." onClick={() => pick(place, false)}>
          <Upload />
        </button>
        <button className="icon-btn" title="Refresh" onClick={() => setVersion(version + 1)}>
          <Refresh />
        </button>
      </div>
      {busy && (
        <div className="tree-note busy">
          <span className="dot working" />
          {busy}
        </div>
      )}
      <div
        className={`tree${dropAt === place ? ' drop' : ''}`}
        key={place}
        onContextMenu={(e) => {
          e.preventDefault()
          setMenu({ x: e.clientX, y: e.clientY, target: null })
        }}
      >
        <TreeDir path={place} depth={0} tree={tree} />
      </div>
      {selected && <Preview path={selected} machine={machine} onClose={() => setSelected(null)} />}
      {menu && (
        <ContextMenu at={menu} onClose={() => setMenu(null)}>
          <button onClick={() => create(into, false)}>
            <NewFile /> New file
          </button>
          <button onClick={() => create(into, true)}>
            <NewFolder /> New folder
          </button>
          {(!target || target.dir) && (
            <>
              <button onClick={() => pick(into, false)}>
                <Upload /> Upload files
              </button>
              <button onClick={() => pick(into, true)}>
                <Upload /> Upload folder
              </button>
            </>
          )}
          {!target && (
            <button onClick={() => setVersion(version + 1)}>
              <Refresh /> Refresh
            </button>
          )}
          {target && (
            <>
              <button onClick={() => rename(target)}>
                <Pencil /> Rename
              </button>
              <button onClick={() => void navigator.clipboard.writeText(target.path).then(() => toast('Path copied.'))}>
                <Copy /> Copy path
              </button>
              <button onClick={() => insertPath(target.path)}>
                <Insert /> Insert path into pane
              </button>
              {machine === undefined ? (
                <>
                  <button onClick={() => openPath(target.path).catch((e) => toast(String(e)))}>
                    <External /> Open
                  </button>
                  <button onClick={() => revealPath(target.path).catch((e) => toast(String(e)))}>
                    <FolderIcon /> Reveal in file manager
                  </button>
                </>
              ) : (
                <button onClick={() => download(target.path)}>
                  <Download /> Download
                </button>
              )}
              <button className="danger" onClick={() => remove(target)}>
                <Trash /> Delete
              </button>
            </>
          )}
        </ContextMenu>
      )}
    </div>
  )
}
