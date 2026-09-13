# Nmux Architecture

Nmux is a desktop home for Claude Code. It runs `claude` (and plain shells) in
real pseudo-terminals that belong to Nmux, not to an editor. The terminals
live in a small daemon process, like a tmux server: closing the window hides
the app to the tray, quitting the app detaches from the daemon, and the next
launch reattaches to whatever is still running. Because Nmux owns the Claude
session ID, a conversation can also be resumed after the daemon itself stops.

## 1. Process model

```
┌─ Session daemon (plain Node: Nmux.exe with ELECTRON_RUN_AS_NODE=1) ──────────┐
│  daemon/index.ts   entry · daemon.json (pid, pipe, token) · idle exit        │
│  ├─ server.ts      named-pipe server, token check + validation ◄────────────┼─┐
│  ├─ sessions/      SessionManager ─► PtySession ─► node-pty ─► claude.exe   │ │
│  │                 (no Electron imports)     └─► @xterm/headless (screen)   │ │
│  └─ store/         SessionStore  → %APPDATA%/nmux/sessions.json             │ │
└─────────────────────────────────────────────────────────────────────────────┘ │
                                                     newline-delimited JSON over │
                                                     \\.\pipe\nmux-<hash>        │
┌─ Electron main process ──────────────────────────────────────────────────────┐ │
│  index.ts        bootstrap · single instance · quit flow                     │ │
│  ├─ daemon/client.ts  finds or spawns the daemon, reconnects, proxies calls ─┼─┘
│  ├─ window.ts    BrowserWindow, hide-on-close (never quits on close)         │
│  ├─ tray.ts      "Show" / "Quit" (detach) / "Quit and stop all sessions"     │
│  ├─ protocol.ts  serves the UI from app://nmux/ (real origin, strict CSP)    │
│  ├─ security.ts  navigation / permission / external-link lockdown            │
│  └─ ipc.ts       ipcMain adapter, sender check + validation ◄─ window.nmux ◄┐ │
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

Four processes, one direction of trust: the renderer can only call what the
preload exposes; the preload only forwards to channels listed in
`src/shared/ipc.ts`; the main process validates every argument in `ipc.ts`
and forwards it to the daemon, which checks the connection token and
validates again in `server.ts`.

**Daemon lifecycle.** On startup the app reads `<userData>/daemon.json`,
connects to the pipe it names and sends `hello` with the token. If there is
no daemon (no file, or the pipe is dead) it spawns `out/main/daemon.js` as a
detached child of its own binary and polls until the daemon rewrites
`daemon.json` with its pid. The daemon exits when told to (`shutdown`, from
"Quit and stop all sessions"), or on its own once no client is attached and
no session is running, so nothing lingers after a clean quit. A daemon from
an older Nmux is replaced on connect only if it has nothing running;
otherwise it is kept so an upgrade never kills live sessions. If the
connection drops the client reconnects (or respawns) with backoff and pushes
the daemon's session list to the UI.

## 2. Layers and their rules

| Layer                | Path                       | May import                     | Rule |
| -------------------- | -------------------------- | ------------------------------ | ---- |
| Shared contract      | `src/shared/`              | nothing                        | Types, channel names and the daemon wire protocol. Single source of truth. |
| Session engine       | `src/daemon/sessions/`     | node, node-pty, xterm headless | **No Electron.** |
| Persistence          | `src/daemon/store/`        | node                           | Atomic JSON writes. |
| Daemon               | `src/daemon/{index,server,validate,endpoint,framing}.ts` | node, sessions | **No Electron.** Runs as a plain Node process. Owns the token check and input validation. |
| Daemon client        | `src/main/daemon/client.ts` | node, shared, daemon/{endpoint,framing} | Spawn, connect, reconnect, typed `call` / `notify`. |
| Electron adapter     | `src/main/ipc.ts`          | electron, daemon client, validate | Only file that knows both `ipcMain` and the daemon client. |
| Shell                | `src/main/{index,window,tray,paths}.ts` | electron          | App lifecycle. No session logic. |
| Bridge               | `src/preload/`             | electron, shared               | Exposes `NmuxApi`, nothing else. |
| UI                   | `src/renderer/`            | react, xterm, shared           | Talks to main only through `api.ts`. |

## 3. Data flow

**Output (PTY → screen)**

```
node-pty onData
  → PtySession: seq++, headless term.write(chunk, cb: parsedSeq = seq), emit('data', chunk, seq)
  → SessionManager emit('data', id, chunk, seq)
  → daemon server.ts writes {"event":"data",...} to every attached client
  → main daemon/client.ts emit('data') → ipc.ts broadcast 'ev:data' to every window
  → preload → renderer lib/ptyStream.ts routes by session id
  → TerminalPane xterm.write(chunk)
```

**Input (keyboard → PTY)**

```
xterm onData → api.sessions.write(id, data) → ipcRenderer.send('sessions:write')
  → ipcMain.on → client.notify('write') → pipe → server.ts → SessionManager.write
  → PtySession.write → node-pty
```

Input and resize use fire-and-forget messages end to end (`send` on IPC, a
request without `id` on the pipe): no round trip on the hot path.

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
daemon start : every persisted session loads as `exited` (no live process yet)
app start    : attach to the daemon; whatever it has running is still running
app quit     : detach only. Sessions keep running in the daemon
"Quit and stop all sessions" : daemon kills every process, persists `exited`, exits
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

3. **Sessions live in a daemon, not in the main process.** That is what
   makes Nmux behave like tmux: the UI can crash, be updated or be quit and
   the processes carry on. The daemon is the same `Nmux.exe` run with
   `ELECTRON_RUN_AS_NODE=1`, so node-pty's native binary and the ABI are
   shared and nothing extra ships. Transport is a named pipe with a random
   per-start token in `daemon.json` (Unix socket on other platforms), chosen
   over a TCP port so nothing listens on the network at all. The trade-off
   is that the daemon holds `Nmux.exe` open: the installer stops it (and its
   sessions) before upgrading, exactly as before the split.

4. **One xterm per session, kept mounted.** Switching sessions toggles
   `hidden`; nothing is torn down, so switching is instant and selection,
   scroll position and search state survive.

5. **`node-linker=hoisted` for pnpm.** electron-builder walks a flat
   `node_modules`; pnpm's symlinked store breaks native module packaging.

6. **`--session-id` over parsing Claude's output.** Owning the ID up front is
   deterministic and needs no hooks or transcript scraping.

## 9. Known limitations (v1)

- Sessions survive the app, not the machine: a reboot, logout or installer
  run stops the daemon. Claude sessions can be resumed afterwards. Lock
  screen changes nothing; sleep and hibernate pause every process and the
  sessions continue after wake. While a session is producing output the app
  holds a `powerSaveBlocker` (`main/keepAwake.ts`, mode in `settings.json`)
  so the idle timer does not put a locked laptop to sleep mid-task; a manual
  sleep or lid close is not overridden, and the blocker needs the app to be
  running (in the tray), not only the daemon.
- Every attached client receives every session's output; a client that
  wants one session (a future CLI `nmux attach`) filters on its side.
- On Windows a spawned child inherits every inheritable handle of its parent
  (libuv passes `bInheritHandles=TRUE`). The daemon therefore keeps a copy of
  the app's `--remote-debugging-port` socket in dev runs, so that port stays
  bound until the daemon exits; the smoke test relaunches on the next port.
  Packaged builds open no such socket. Chromium's own files, pipes and the
  single-instance mutex are non-inheritable and are not affected.
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
expected output, screenshots each state into `.smoke/`, hard-kills the app
and relaunches it to prove the sessions survived in the daemon, removes what
it created and stops the test profile's daemon. Use `node scripts/smoke.mjs --attach 9333` against
`pnpm dev -- --remote-debugging-port=9333` to test the dev build.
