import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import '@xterm/xterm/css/xterm.css'
import { call, send } from './api'
import { handleKey } from './keys'
import { toast } from './store'
import type { Pane, PaneId } from './types'

const terminalTheme = {
  background: '#17191d',
  foreground: '#d7dae0',
  cursor: '#d7dae0',
  cursorAccent: '#17191d',
  selectionBackground: '#2f3644',
  scrollbarSliderBackground: 'rgba(215, 218, 224, 0.3)',
  scrollbarSliderHoverBackground: 'rgba(215, 218, 224, 0.5)',
  scrollbarSliderActiveBackground: 'rgba(150, 178, 215, 0.8)',
  black: '#1b1f2a',
  red: '#f07178',
  green: '#98c379',
  yellow: '#e5c07b',
  blue: '#7aa2f7',
  magenta: '#c678dd',
  cyan: '#56b6c2',
  white: '#c8ccd4',
  brightBlack: '#5c6370',
  brightRed: '#ff8a8f',
  brightGreen: '#b5e890',
  brightYellow: '#f5d68a',
  brightBlue: '#9ab8ff',
  brightMagenta: '#d99cf0',
  brightCyan: '#7fd8e0',
  brightWhite: '#ffffff'
}

interface Entry {
  term: Terminal
  fit: FitAddon
  host: HTMLDivElement
  sent: { cols: number; rows: number }
  ready: boolean
  pending: Array<[number, string]>
  seq: number
  attachId: number
  observer: ResizeObserver
}

const entries = new Map<PaneId, Entry>()

function create(pane: PaneId): Entry {
  const host = document.createElement('div')
  host.className = 'terminal-host'
  const term = new Terminal({
    allowProposedApi: true,
    cursorBlink: true,
    cursorStyle: 'block',
    fontFamily: '"Geist Mono Variable", "Cascadia Mono", Consolas, "Courier New", monospace',
    fontSize: 14,
    lineHeight: 1.2,
    scrollback: 5000,
    theme: terminalTheme,
    windowsPty: { backend: 'conpty' }
  })
  const fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  term.loadAddon(new Unicode11Addon())
  term.unicode.activeVersion = '11'

  const entry: Entry = {
    term,
    fit,
    host,
    sent: { cols: 0, rows: 0 },
    ready: false,
    pending: [],
    seq: 0,
    attachId: 0,
    observer: new ResizeObserver(() => syncSize(pane, entry))
  }

  term.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== 'keydown') return true
    if (handleKey(ev)) return false
    const key = ev.key.toLowerCase()
    if (ev.ctrlKey && key === 'c' && (ev.shiftKey || term.hasSelection())) {
      void navigator.clipboard.writeText(term.getSelection())
      term.clearSelection()
      return false
    }
    if (ev.ctrlKey && key === 'v') return false
    return true
  })
  term.onData((data) => send({ op: 'write', pane, data }))
  const markScrolls = (): void => void host.classList.toggle('scrolls', term.buffer.active.baseY > 0)
  term.onWriteParsed(markScrolls)
  term.buffer.onBufferChange(markScrolls)
  host.addEventListener(
    'paste',
    (ev) => {
      const item = Array.from(ev.clipboardData?.items ?? []).find((i) => i.kind === 'file' && i.type.startsWith('image/'))
      const file = item?.getAsFile()
      if (!file) return
      ev.preventDefault()
      ev.stopPropagation()
      void pasteImage(pane, file)
    },
    true
  )
  entries.set(pane, entry)
  return entry
}

function syncSize(pane: PaneId, entry: Entry): void {
  if (!entry.ready || entry.host.offsetWidth === 0 || entry.host.offsetHeight === 0) return
  entry.fit.fit()
  const { cols, rows } = entry.term
  if (cols === entry.sent.cols && rows === entry.sent.rows) return
  entry.sent = { cols, rows }
  send({ op: 'resize', pane, cols, rows })
}

async function pasteImage(pane: PaneId, file: File): Promise<void> {
  try {
    const bytes = new Uint8Array(await file.arrayBuffer())
    let binary = ''
    for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    await call({ op: 'pasteImage', pane, extension: file.type.split('/')[1] ?? 'png', data: btoa(binary) })
  } catch (err) {
    toast(String(err))
  }
}

async function attach(pane: PaneId, entry: Entry): Promise<void> {
  const id = ++entry.attachId
  entry.ready = false
  entry.pending = []
  try {
    const reply = await call({ op: 'attach', pane })
    if (id !== entry.attachId || reply.kind !== 'attached') return
    const { snapshot } = reply
    entry.term.reset()
    entry.term.resize(snapshot.cols, snapshot.rows)
    entry.term.write(snapshot.data)
    entry.seq = snapshot.seq
    for (const [seq, data] of entry.pending) {
      if (seq > entry.seq) entry.term.write(data)
    }
    entry.pending = []
    entry.sent = { cols: snapshot.cols, rows: snapshot.rows }
    entry.ready = true
    syncSize(pane, entry)
  } catch (err) {
    if (id === entry.attachId) console.warn('[nmux] attach', pane, err)
  }
}

export const terminals = {
  mount(pane: PaneId, container: HTMLElement): void {
    let entry = entries.get(pane)
    const fresh = !entry
    entry ??= create(pane)
    if (entry.host.parentElement !== container) container.appendChild(entry.host)
    if (fresh) {
      entry.term.open(entry.host)
      try {
        const webgl = new WebglAddon()
        webgl.onContextLoss(() => webgl.dispose())
        entry.term.loadAddon(webgl)
      } catch {
      }
      entry.observer.observe(entry.host)
      void attach(pane, entry)
    } else {
      syncSize(pane, entry)
    }
  },

  unmount(pane: PaneId, container: HTMLElement): void {
    const entry = entries.get(pane)
    if (entry && entry.host.parentElement === container) container.removeChild(entry.host)
  },

  paste(pane: PaneId, text: string): void {
    const entry = entries.get(pane)
    if (entry) entry.term.paste(text)
    else send({ op: 'write', pane, data: text })
  },

  resync(pane: PaneId): void {
    const entry = entries.get(pane)
    if (!entry) return
    entry.sent = { cols: 0, rows: 0 }
    syncSize(pane, entry)
  },

  focus(pane: PaneId): void {
    entries.get(pane)?.term.focus()
  },

  output(pane: PaneId, seq: number, data: string): void {
    const entry = entries.get(pane)
    if (!entry) return
    if (!entry.ready) entry.pending.push([seq, data])
    else if (seq > entry.seq) entry.term.write(data)
  },

  retain(panes: Map<PaneId, Pane>): void {
    for (const [id, entry] of entries) {
      if (panes.has(id)) continue
      entry.observer.disconnect()
      entry.term.dispose()
      entry.host.remove()
      entries.delete(id)
    }
  },

  reattachAll(): void {
    for (const [id, entry] of entries) void attach(id, entry)
  }
}
