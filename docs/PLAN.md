# Nmux Plan

**Problem.** Claude Code runs inside an IDE's terminal today. Close VS Code and
the session dies with it.

**Solution.** A small desktop app that owns the terminals. Close the window,
the app keeps running in the tray. Quit and restart, conversations resume.

**Non-goals (for now).** Replacing the IDE, editing files, a custom chat UI
on top of the Agent SDK. Nmux shows the real `claude` TUI in a real terminal.

---

## M0 · Foundation — done (2026-09-06)

Scaffold with a working vertical slice.

- [x] Electron 44 + electron-vite 5 + React 19 + TypeScript, pnpm (hoisted)
- [x] `SessionManager` / `PtySession` engine with no Electron imports
- [x] node-pty via prebuilt N-API binary (no native compile on Windows)
- [x] Headless xterm per session for detach/reattach with exact `seq` bookkeeping
- [x] xterm.js UI: WebGL renderer, fit, links, unicode11, search addon loaded
- [x] Sidebar (select, rename, stop, resume/restart, remove), new-session dialog, status bar
- [x] Hide-to-tray on close; tray menu; quit confirmation when sessions run
- [x] Session metadata persisted; Claude sessions resumable via `--resume`
- [x] Copy `claude --resume <id>` from the status bar
- [x] Shortcuts: Ctrl+Shift+N new, Ctrl+PgUp/PgDn switch, Ctrl+Shift+C/V copy/paste
- [x] `electron-builder.yml` for NSIS + portable Windows builds
- [x] Security hardening: `app://` origin with strict CSP, IPC sender checks and
      input limits, navigation/permission lockdown, Electron fuses (see SECURITY.md)
- [x] `pnpm smoke` end-to-end test including negative security checks

Acceptance: `pnpm dev` opens the app, a Claude session starts in the chosen
folder, closing the window leaves it running, reopening from the tray shows the
same screen, quitting and relaunching offers "Resume conversation".

## M1 · Daily-driver polish

Make it comfortable enough to replace the VS Code terminal.

- [ ] Graceful stop for Claude sessions: send Ctrl+C / `/exit` and wait briefly
      before the hard kill. Today's TerminateProcess makes Claude Code log
      "fullscreen renderer didn't finish starting last time" on its next launch.
- [ ] Settings (font, size, theme, default shell, default claude args) in `settings.json`
- [ ] Search UI for the already-loaded search addon (Ctrl+Shift+F)
- [ ] Right-click context menu: copy, paste, clear, rename
- [ ] Recent folders list in the new-session dialog
- [ ] "Open in Explorer" / "Open in VS Code" for the session cwd
- [ ] Bell / activity indicator on inactive sessions (dot turns orange on output)
- [ ] Window state (size, position, last active session) restored on launch
- [ ] Output batching in `ipc.ts` if profiling shows IPC overhead on large outputs

## M2 · Daemon split (true tmux semantics)

Sessions survive UI crashes and app restarts.

- [ ] Move `src/main/sessions/` + `store/` into `src/daemon/` with a WebSocket
      (localhost, random port + token written to `userData`) or named-pipe server
- [ ] Launch the daemon as a detached child using the Electron binary with
      `ELECTRON_RUN_AS_NODE=1` (same node-pty ABI, no second runtime to ship)
- [ ] `ipc.ts` becomes a thin proxy: renderer ↔ main ↔ daemon
- [ ] Attach on startup replays each session's serialized screen (already supported)
- [ ] Tray "Quit" vs "Quit and stop daemon"
- [ ] Optional: start daemon at login

## M3 · Claude-aware features

Use the fact that Nmux owns the session ID.

- [ ] Detect "Claude is waiting for input" via a `Notification` hook that pings
      Nmux (`NMUX_SESSION_ID` is already in the environment) → tray balloon +
      sidebar badge
- [ ] Import existing conversations from `~/.claude/projects` into the list
- [ ] Per-project defaults (model, `--add-dir`, permission mode)
- [ ] Grid view: several sessions visible at once

## M4 · Distribution

- [ ] Code signing, auto-update (electron-updater), CI build on tag
- [ ] macOS / Linux smoke test (`shellLaunch` already branches per platform)
- [ ] Proper icon artwork (replace `scripts/gen-icons.mjs` output)

---

## Risks and how they are handled

| Risk | Mitigation |
| ---- | ---------- |
| node-pty native build breaks on an Electron upgrade | It is N-API; prebuilds cover Node and Electron. VS 2022 Build Tools are present as a fallback (`pnpm rebuild node-pty`). |
| Claude TUI renders oddly after reattach | Snapshot comes from a real headless terminal, and the first resize after attach triggers a full Ink redraw. |
| Output floods the IPC channel | Each chunk is one message today; batch in `ipc.ts` (M1) if it shows up in profiling. |
| electron-vite / Vite major bumps | Pinned: electron-vite 5 needs Vite 7 and `@vitejs/plugin-react` 5. Do not bump to Vite 8 until electron-vite supports it. |
