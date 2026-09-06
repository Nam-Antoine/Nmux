# Nmux Security

## Threat model

Nmux spawns processes as the logged-in user and renders their raw output.
Two things are worth protecting:

1. **The renderer processes untrusted bytes.** Anything running inside a
   session (Claude Code, a build tool, a file that gets `cat`ed, a web page
   Claude fetches) can emit arbitrary text and escape sequences. If the
   renderer were ever compromised, it could ask the main process to spawn a
   shell and type into it, which is code execution as the user.
2. **The main process runs unsandboxed** and owns every PTY.

Out of scope: other users or malware already running on the same machine
under the same account (they can already do everything Nmux can), and the
safety of Claude Code itself (its permission system is its own).

## Controls

| Area | Control | Where |
| ---- | ------- | ----- |
| Renderer isolation | `app.enableSandbox()`, `sandbox: true`, `contextIsolation: true`, `nodeIntegration: false`, `webviewTag: false` | `main/index.ts`, `main/window.ts` |
| Attack surface | DevTools disabled and application menu removed in packaged builds (no reload / inspect accelerators); spellcheck off (no dictionary downloads) | `main/window.ts` |
| Origin | UI is served from `app://nmux/`, a registered standard+secure scheme, never `file://`. Only files under `out/renderer` are served, path traversal is rejected, MIME types are fixed, `X-Content-Type-Options: nosniff` | `main/protocol.ts` |
| CSP | Production header: `default-src 'none'`, scripts from self only, no `connect-src`, `object-src 'none'`, `base-uri 'none'`, `form-action 'none'`. Dev meta tag additionally allows Vite's HMR websocket; browsers apply both | `main/protocol.ts`, `renderer/index.html` |
| Navigation | `will-navigate` / `will-redirect` blocked unless the target is our own UI; `<webview>` attach denied; `window.open` always denied | `main/security.ts` |
| External links | Terminal links go to the OS browser only for `http:`, `https:`, `mailto:` and at most 2048 chars. `file:`, `ms-msdt:` and other protocol handlers are never invoked | `main/security.ts` |
| Browser permissions | Request and check handlers deny everything except clipboard write/read; downloads are cancelled | `main/security.ts` |
| IPC sender | Every channel verifies the sender is the **main frame** of our own renderer URL before doing anything | `main/ipc.ts` `isTrustedSender` |
| IPC input | Session ids must be UUIDs; `cwd` must be an absolute existing directory; names ≤ 80 chars without control characters; ≤ 32 extra args of ≤ 512 chars; write chunks ≤ 1 MiB; cols/rows bounded; at most 64 sessions | `main/ipc.ts`, `shared/session.ts` `LIMITS` |
| Preload surface | Exposes exactly `NmuxApi`. Event names are allow-listed; `ipcRenderer` itself is never exposed | `preload/index.ts` |
| Child environment | Sessions inherit the user's environment minus `ELECTRON_*` and nested-Claude markers. No secrets are added | `main/sessions/launch.ts` |
| Packaged binary (fuses) | `NODE_OPTIONS` ignored, `--inspect` ignored, cookie encryption on, ASAR integrity validated, app code loads only from ASAR, no file-protocol extra privileges. `RunAsNode` stays **on** because node-pty forks a helper with `ELECTRON_RUN_AS_NODE` (and the M2 daemon will) | `electron-builder.yml` |
| Data at rest | `sessions.json` stores names, paths and session UUIDs only. Conversation content stays in Claude Code's own `~/.claude` | `main/store/SessionStore.ts` |
| Supply chain | 3 runtime dependencies (node-pty, @xterm/headless, @xterm/addon-serialize); lockfile committed; `pnpm audit` clean as of 2026-09-06 | `package.json` |

## Residual risks

- **A compromised renderer is still game over.** `sessions.create` + `write`
  is the product. The defence is keeping the renderer uncompromisable: no
  remote content, sandbox, strict CSP, sender checks. Same posture as VS
  Code's terminal or Windows Terminal.
- **`--remote-debugging-port` cannot be fused off** (it is a Chromium flag).
  Anyone who can launch Nmux with flags already runs as the user. The smoke
  test depends on it.
- **`RunAsNode` is on.** Anyone with the Nmux binary gets a Node runtime.
  That equals having `node.exe`, grants no extra privilege, and is required
  by node-pty today.
- **No code signing or auto-update yet** (Plan M4). SmartScreen will warn,
  and security fixes need a manual reinstall.
- **Clickable links** are a phishing surface, as in any terminal. The scheme
  allow-list stops protocol-handler abuse, not a convincing `https://` link.
- **Dev server** (`pnpm dev`) listens on localhost. Do not run it on a
  shared host.

## Rules for changes

- New IPC channel: add it to `shared/ipc.ts`, register through the `handle`
  / `on` wrappers in `main/ipc.ts` so the sender check applies, validate
  every argument there. Never call `ipcMain.handle` directly elsewhere.
- Never load a remote URL into any `BrowserWindow`. Never enable
  `nodeIntegration`. Never widen the CSP for convenience.
- A new browser permission goes into `ALLOWED_PERMISSIONS` with a comment
  saying which feature needs it.
- New dependencies go into `devDependencies` (bundled) unless they are native
  or, like `@xterm/headless`, cannot be bundled.
- Verify with `pnpm smoke` and, for packaging changes,
  `pnpm dist:dir && node scripts/smoke.mjs --exe release/win-unpacked/Nmux.exe`.
