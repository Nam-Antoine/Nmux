# Nmux Architecture

Nmux is a desktop home for Claude Code. It runs `claude` (and plain shells) in
real pseudo-terminals that belong to Nmux, not to an editor. Closing the window
hides it to the system tray; every process keeps running. Because Nmux owns the
Claude session ID, a conversation can be resumed after Nmux itself restarts.

## 1. Process model

```
┌─ Electron main process ──────────────────────────────────────────────────────┐
│                                                                              │
│  index.ts        bootstrap · single instance · quit flow                     │
│  ├─ window.ts    BrowserWindow, hide-on-close (never quits on close)         │
│  ├─ tray.ts      tray icon, "Show" / "Quit"                                  │
│  ├─ protocol.ts  serves the UI from app://nmux/ (real origin, strict CSP)    │
│  ├─ security.ts  navigation / permission / external-link lockdown            │
│  ├─ ipc.ts       ipcMain adapter, sender check + validation ◄─ window.nmux ◄┐ │
│  └─ sessions/    SessionManager ─► PtySession ─► node-pty ─► claude.exe   │  │
│                  (no Electron imports)     └─► @xterm/headless (screen)   │  │
│     store/       SessionStore  → %APPDATA%/nmux/sessions.json             │  │
└───────────────────────────────────────────────────────────────────────────┼──┘
                                                                            │
┌─ Preload (sandboxed) ──────────────────────────────────────────────────────┼──┐
│  contextBridge exposes exactly `NmuxApi` as window.nmux                    ┘  │
└──────────────────────────────────────────────────────────────────────────────┘
                                                                            ▲
┌─ Renderer (sandboxed, no Node) ────────────────────────────────────────────┼──┐
│  api.ts ─► store/sessions.ts (zustand mirror) ─► components/               │  │
│                                    TerminalPane = one xterm.js per session ┘  │
└──────────────────────────────────────────────────────────────────────────────┘
```

Three processes, one direction of trust: the renderer can only call what the
preload exposes; the preload only forwards to channels listed in
`src/shared/ipc.ts`; the main process validates every argument in `ipc.ts`.

## 2. Layers and their rules

| Layer                | Path                       | May import                     | Rule |
| -------------------- | -------------------------- | ------------------------------ | ---- |
| Shared contract      | `src/shared/`              | nothing                        | Types and channel names only. Single source of truth for main ↔ renderer. |
| Session engine       | `src/main/sessions/`       | node, node-pty, xterm headless | **No Electron.** This is the future daemon. |
| Persistence          | `src/main/store/`          | node                           | Atomic JSON writes. |
| Electron adapter     | `src/main/ipc.ts`          | electron, sessions             | Only file that knows both `ipcMain` and `SessionManager`. |
| Shell                | `src/main/{index,window,tray,paths}.ts` | electron          | App lifecycle. No session logic. |
| Bridge               | `src/preload/`             | electron, shared               | Exposes `NmuxApi`, nothing else. |
| UI                   | `src/renderer/`            | react, xterm, shared           | Talks to main only through `api.ts`. |

## 3. Data flow

**Output (PTY → screen)**

```
node-pty onData
  → PtySession: seq++, headless term.write(chunk, cb: parsedSeq = seq), emit('data', chunk, seq)
  → SessionManager emit('data', id, chunk, seq)
  → ipc.ts broadcast 'ev:data' to every window
  → preload → renderer lib/ptyStream.ts routes by session id
  → TerminalPane xterm.write(chunk)
```

**Input (keyboard → PTY)**

```
xterm onData → api.sessions.write(id, data) → ipcRenderer.send('sessions:write')
  → ipcMain.on → SessionManager.write → PtySession.write → node-pty
```

Input and resize use fire-and-forget `send`, not `invoke`: no round trip on
the hot path.

**Attach (UI (re)binds to a live process)**

```
renderer invoke 'sessions:attach'
  → PtySession.snapshot(): serialize headless screen + scrollback, return parsedSeq
  → renderer writes the snapshot, then replays any chunks it buffered
    while waiting whose seq > parsedSeq, then streams live
```

`parsedSeq` is updated inside xterm's write callback, so it is exactly the set
of chunks the serialized screen contains. No output is lost or duplicated at
the attach boundary, regardless of IPC timing.

**Resize**

```
ResizeObserver → FitAddon.fit() → api.sessions.resize(id, cols, rows)
  → PtySession.resize → headless term + node-pty (SIGWINCH equivalent)
```

Resizes are skipped while the pane is hidden (0×0), so background sessions
never get told they are two columns wide.

## 4. Session lifecycle

```
create ─────► running ──(process exit)──► exited ──(restart)──► running
                                            │
                                            └──(remove)──► gone
app start : every persisted session loads as `exited` (no live process yet)
app quit  : every process is killed, statuses persisted as `exited`
```

`restart` means:

- Claude session → `claude --resume <claudeSessionId>` in the same cwd.
- Shell session → a fresh shell in the same cwd.

The same `TerminalPane` stays mounted across restarts; it re-attaches when the
session's `pid` changes.

## 5. Claude session identity

On create, Nmux generates a UUID and launches `claude --session-id <uuid>`.
Claude Code writes its transcript under `~/.claude/projects/...` keyed by that
ID. Nmux stores only metadata (`SessionMeta`), never conversation content.

That single decision gives:

- **Resume after Nmux restart**: `claude --resume <uuid>`.
- **Escape hatch**: the status bar copies `claude --resume <uuid>` so the same
  conversation can be continued in any terminal.
- **Attach to an existing conversation**: paste an ID into the new-session
  dialog.

## 6. Persistence

`%APPDATA%/nmux/sessions.json` (Electron `userData`), schema version 1:

```json
{ "version": 1, "sessions": [ SessionMeta, ... ] }
```

Writes are atomic (temp file + rename) and debounced 500 ms. The file only
holds metadata; losing it loses the session *list*, not any conversation.

## 7. Security posture

Full threat model and control table: [SECURITY.md](./SECURITY.md). In short:

- Renderer is sandboxed, isolated, has no Node, no DevTools or menu when packaged.
- UI is served from `app://nmux/` (`protocol.ts`), never `file://`; strict CSP header.
- Navigation, redirects, webviews, downloads and non-clipboard permissions are
  denied (`security.ts`); terminal links open externally only for http/https/mailto.
- Every IPC message is checked for a trusted sender (main frame of our own
  URL) and validated against `LIMITS` before reaching `SessionManager`.
- Packaged builds set Electron fuses (ASAR integrity, no `NODE_OPTIONS`, no `--inspect`).
- Sessions inherit the user's environment minus `ELECTRON_*` and nested-Claude markers.

## 8. Key decisions

1. **Electron, not Tauri.** node-pty is the most battle-tested PTY binding
   (VS Code, Hyper, Tabby) and handles ConPTY quirks on Windows. One language
   end to end, and the Claude ecosystem (Agent SDK, hooks) is Node-based.
   Cost: ~200 MB install footprint. Rust is installed on this machine, so a
   Tauri port stays possible if footprint ever matters more than time.

2. **Headless xterm in the main process, not a raw byte ring buffer.** A ring
   buffer of raw ANSI replays badly for full-screen TUIs like Claude Code's
   Ink UI. A headless terminal keeps real screen state and serializes it
   cleanly. VS Code does the same for terminal reconnection.

3. **Sessions live in the main process for v1.** Hide-to-tray already gives
   "close the window, keep working". A separate daemon (survive UI crashes and
   restarts) is Milestone 2 in `PLAN.md`; the `sessions/` package has no
   Electron imports so that split is a transport change, not a rewrite.

4. **One xterm per session, kept mounted.** Switching sessions toggles
   `hidden`; nothing is torn down, so switching is instant and selection,
   scroll position and search state survive.

5. **`node-linker=hoisted` for pnpm.** electron-builder walks a flat
   `node_modules`; pnpm's symlinked store breaks native module packaging.

6. **`--session-id` over parsing Claude's output.** Owning the ID up front is
   deterministic and needs no hooks or transcript scraping.

## 9. Known limitations (v1)

- Sessions die when Nmux quits (not when the window closes). See M2.
- Windows only has been exercised. macOS/Linux should work (`shellLaunch`
  branches on platform) but are untested.
- No split panes or tabs-within-a-session yet.
- No notification when Claude is waiting for input (M3, via hooks).
- Extra `claude` args are split with a minimal tokenizer (whitespace + double
  quotes), not a full shell parser.
- Stopping a session on Windows may print `Error: AttachConsole failed` in the
  dev console. It comes from a helper process node-pty forks to enumerate the
  console's processes; the ConPTY has already been closed by then, so the
  helper fails, and node-pty falls back correctly. The process is killed
  (verified: `onExit` fires, PID gone). Harmless.

## 10. Verifying a change

`pnpm smoke` builds, launches the real app with DevTools enabled, creates a
shell and a Claude Code session, checks the attach snapshot contains the
expected output, screenshots each state into `.smoke/`, and removes what it
created. Use `node scripts/smoke.mjs --attach 9333` against
`pnpm dev -- --remote-debugging-port=9333` to test the dev build.
