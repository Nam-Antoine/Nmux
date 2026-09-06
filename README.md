# Nmux

A persistent desktop home for Claude Code. Sessions run in real terminals that
belong to Nmux, so closing VS Code (or the Nmux window) never kills them.

- Close the window → Nmux keeps running in the system tray.
- Reopen → the same screen, same process.
- Quit and relaunch → "Resume conversation" (Nmux owns the Claude session ID).
- Sessions are grouped by project folder in the sidebar; a folder can hold as
  many Claude and shell sessions as you like.
- Optional "Skip permission prompts" per session (`--dangerously-skip-permissions`).
- Grid view: split the workspace 1×2, 2×2, up to 4×4 from the status bar and
  choose which session each pane shows.
- "Resume conversation" falls back to a new conversation when the old one has
  nothing to resume, and an exited session keeps its last screen.

## Quick start

```bash
pnpm install     # node-pty uses a prebuilt binary, no compiler needed
pnpm dev         # run with hot reload
```

| Script           | What it does                                   |
| ---------------- | ---------------------------------------------- |
| `pnpm dev`       | Dev mode with HMR for the renderer             |
| `pnpm typecheck` | Type-check main/preload and renderer           |
| `pnpm build`     | Production bundle into `out/`                  |
| `pnpm start`     | Run the production bundle                      |
| `pnpm dist`      | Windows installer + portable exe in `release/` |
| `pnpm icons`     | Regenerate placeholder icons                   |
| `pnpm smoke`     | End-to-end check: real sessions, screenshots in `.smoke/` |

Requires Node ≥ 22.12, pnpm 10, and `claude` on PATH.

## Shortcuts

| Keys             | Action                          |
| ---------------- | ------------------------------- |
| Ctrl+Shift+N     | New session                     |
| Ctrl+PgUp / PgDn | Previous / next session in the focused pane |
| Ctrl+Shift+← / → | Focus previous / next pane      |
| Ctrl+Shift+C     | Copy selection                  |
| Ctrl+C           | Copy if text is selected, else SIGINT |
| Ctrl+V           | Paste                           |
| Double-click     | Rename session (sidebar)        |
| Folder row       | Click to collapse / expand; **+** starts another session in that folder |

## Project structure

```
src/
├─ shared/          contract shared by all processes (types + IPC channel names)
│  ├─ session.ts    SessionMeta, CreateSessionRequest, AttachResult …
│  └─ ipc.ts        IPC channel names, NmuxApi, NmuxEvents
├─ main/            Electron main process
│  ├─ index.ts      bootstrap, single instance, quit flow
│  ├─ window.ts     BrowserWindow + hide-on-close
│  ├─ tray.ts       tray icon and menu
│  ├─ ipc.ts        ipcMain ↔ SessionManager adapter (sender check + validation)
│  ├─ protocol.ts   serves the UI from app://nmux/ with a strict CSP
│  ├─ security.ts   navigation, permission and external-link lockdown
│  ├─ paths.ts      resource paths (dev vs packaged)
│  ├─ sessions/     PTY engine — NO Electron imports (future daemon)
│  │  ├─ SessionManager.ts  create/restart/kill/attach/write/resize, events
│  │  ├─ PtySession.ts      node-pty + headless xterm mirror + seq tracking
│  │  ├─ launch.ts          builds `claude --session-id …` / shell commands
│  │  └─ claudeProjects.ts  finds Claude's transcript to know if a resume can work
│  └─ store/SessionStore.ts atomic JSON persistence
├─ preload/         contextBridge → window.nmux (exactly NmuxApi)
└─ renderer/        React UI (sandboxed, no Node)
   └─ src/
      ├─ api.ts             the one seam to window.nmux
      ├─ store/sessions.ts  zustand mirror of main's session list + UI state
      ├─ lib/ptyStream.ts   routes output chunks to the owning terminal
      ├─ lib/folders.ts     groups sessions by project folder for the sidebar
      └─ components/        Sidebar, Workspace (grid), TerminalPane, NewSessionDialog, StatusBar, EmptyState

docs/ARCHITECTURE.md   process model, data flow, decisions
docs/SECURITY.md       threat model, controls, residual risks
docs/PLAN.md           milestones and risks
```

Runtime data lives in `%APPDATA%/nmux/sessions.json` (metadata only; Claude
keeps transcripts in `~/.claude/projects`).

## Status

Early. Milestone M0 (foundation) is done and verified on Windows 11; see
[docs/PLAN.md](docs/PLAN.md) for what comes next. macOS and Linux are
untested.

## License

[MIT](LICENSE) © 2026 Nathan Collins
