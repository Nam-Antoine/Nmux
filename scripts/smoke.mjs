/**
 * End-to-end smoke test. Drives the real app over the Chrome DevTools Protocol:
 * creates a shell session, runs a command, verifies the output through the
 * attach snapshot, starts a real Claude Code session, takes screenshots, and
 * removes the sessions it created.
 *
 *   pnpm smoke                     build first, then spawn out/ and test it
 *   node scripts/smoke.mjs --attach 9333
 *                                  drive an already running instance, e.g.
 *                                  `pnpm dev -- --remote-debugging-port=9333`
 *   node scripts/smoke.mjs --exe release/win-unpacked/Nmux.exe
 *                                  test a packaged build (after `pnpm dist:dir`)
 *
 * Screenshots land in .smoke/. Set NMUX_SMOKE_SKIP_CLAUDE=1 to skip the
 * Claude Code part (it launches the real CLI, which needs `claude` on PATH).
 */
import { spawn } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const outDir = join(root, '.smoke')
const attachIndex = process.argv.indexOf('--attach')
const port = attachIndex !== -1 ? Number(process.argv[attachIndex + 1]) : 9333
const skipClaude = process.env.NMUX_SMOKE_SKIP_CLAUDE === '1'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const strip = (s) => s.replace(/\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]/g, '')

mkdirSync(outDir, { recursive: true })

// ---- launch ----------------------------------------------------------------

let child = null
if (attachIndex === -1) {
  // A nested `claude` refuses to start if it inherits these from a parent
  // Claude Code session (the app strips them too; this covers the shell part).
  const env = { ...process.env }
  for (const k of Object.keys(env)) if (/^CLAUDE(CODE|_PID|_EFFORT|_CODE_)/.test(k)) delete env[k]
  // --exe <path> runs a packaged binary (e.g. release/win-unpacked/Nmux.exe)
  // instead of the dev Electron + out/.
  const exeIndex = process.argv.indexOf('--exe')
  const exe = exeIndex !== -1 ? resolve(process.argv[exeIndex + 1]) : null
  const electron = exe ?? join(root, 'node_modules', 'electron', 'dist', process.platform === 'win32' ? 'electron.exe' : 'electron')
  const args = [`--remote-debugging-port=${port}`, ...(exe ? [] : [root])]
  child = spawn(electron, args, { env, stdio: ['ignore', 'pipe', 'pipe'] })
  child.stdout.on('data', (d) => process.stdout.write(`[electron] ${d}`))
  child.stderr.on('data', (d) => process.stderr.write(`[electron] ${d}`))
}

// ---- CDP client --------------------------------------------------------------

async function findPage() {
  for (let i = 0; i < 60; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json()
      const page = list.find((t) => t.type === 'page' && /^app:\/\/nmux\/|localhost|index\.html/.test(t.url))
      if (page) return page
    } catch {
      /* not up yet */
    }
    await sleep(500)
  }
  throw new Error(`no page target on port ${port}`)
}

const page = await findPage()
const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((res, rej) => {
  ws.onopen = res
  ws.onerror = rej
})
let nextId = 0
const pending = new Map()
ws.onmessage = (e) => {
  const m = JSON.parse(e.data)
  const p = m.id && pending.get(m.id)
  if (!p) return
  pending.delete(m.id)
  m.error ? p.rej(new Error(JSON.stringify(m.error))) : p.res(m.result)
}
const send = (method, params = {}) =>
  new Promise((res, rej) => {
    const id = ++nextId
    pending.set(id, { res, rej })
    ws.send(JSON.stringify({ id, method, params }))
  })
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? JSON.stringify(r.exceptionDetails))
  return r.result.value
}
const screenshot = async (name) => {
  const shot = await send('Page.captureScreenshot', { format: 'png' })
  writeFileSync(join(outDir, name), Buffer.from(shot.data, 'base64'))
  console.log(`screenshot .smoke/${name}`)
}
const clickSession = async (name) => {
  for (let i = 0; i < 20; i++) {
    const ok = await evaluate(
      `(() => { const el = [...document.querySelectorAll('.session-item')].find(li => li.textContent.includes(${JSON.stringify(name)})); if (el) el.click(); return !!el })()`
    )
    if (ok) return
    await sleep(250)
  }
  throw new Error(`session ${name} never appeared in the sidebar`)
}
const check = (cond, label) => {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${label}`)
  if (!cond) failures++
}
let failures = 0

// ---- scenario ------------------------------------------------------------------

try {
  await send('Runtime.enable')
  // Wait for the page to commit and the preload bridge to be exposed.
  let ready = false
  for (let i = 0; i < 60 && !ready; i++) {
    ready = await evaluate(`typeof window.nmux === 'object' && document.readyState === 'complete' && !!document.querySelector('.app')`).catch(() => false)
    if (!ready) await sleep(500)
  }
  if (!ready) throw new Error('UI never became ready (window.nmux missing or app not rendered)')
  await sleep(500)
  const info = await evaluate('window.nmux.app.info()')
  console.log('app', info.version, 'claude:', info.claudeBinary ?? 'NOT FOUND')
  await screenshot('01-empty.png')

  // --- security controls ---------------------------------------------------------
  const expectReject = async (label, expression) => {
    try {
      await evaluate(expression)
      check(false, `${label} (was accepted)`)
    } catch (err) {
      check(true, `${label}: ${String(err.message).split('\n')[0].slice(0, 90)}`)
    }
  }
  await expectReject('rejects relative cwd', `window.nmux.sessions.create({ kind: 'shell', cwd: 'relative/dir' })`)
  await expectReject('rejects missing cwd', `window.nmux.sessions.create({ kind: 'shell', cwd: ${JSON.stringify(join(root, 'does-not-exist'))} })`)
  await expectReject('rejects unknown kind', `window.nmux.sessions.create({ kind: 'bash', cwd: ${JSON.stringify(root)} })`)
  await expectReject('rejects non-uuid id', `window.nmux.sessions.attach('../etc/passwd')`)
  await expectReject('rejects unknown event', `window.nmux.on('__proto__', () => {})`)
  const before = await evaluate('location.href')
  await evaluate(`location.assign('https://example.com/'); new Promise(r => setTimeout(r, 800)).then(() => location.href)`)
  check((await evaluate('location.href')) === before, `navigation away from the UI is blocked (${before})`)
  check((await evaluate(`window.open('file:///C:/Windows/') === null`)) === true, 'window.open is denied')
  if (before.startsWith('app://')) {
    await expectReject('production CSP blocks fetch', `fetch('app://nmux/index.html').then(r => r.status)`)
  }

  const shell = await evaluate(`window.nmux.sessions.create({ kind: 'shell', cwd: ${JSON.stringify(root)}, name: 'smoke-shell' })`)
  check(shell.status === 'running' && shell.pid > 0, 'shell session spawned')
  await clickSession('smoke-shell')
  await sleep(2500)
  await evaluate(`window.nmux.sessions.write(${JSON.stringify(shell.id)}, 'echo NMUX_SMOKE_OK\\r')`)
  await sleep(2000)
  await screenshot('02-shell.png')
  const shellAttach = await evaluate(`window.nmux.sessions.attach(${JSON.stringify(shell.id)})`)
  check(shellAttach.snapshot.includes('NMUX_SMOKE_OK'), `shell output visible in attach snapshot (${shellAttach.cols}x${shellAttach.rows}, seq ${shellAttach.seq})`)
  check(shellAttach.cols > 80 && shellAttach.rows > 20, 'terminal was fitted to the pane')

  if (!skipClaude && info.claudeBinary) {
    const claude = await evaluate(`window.nmux.sessions.create({ kind: 'claude', cwd: ${JSON.stringify(root)}, name: 'smoke-claude' })`)
    check(claude.status === 'running' && typeof claude.claudeSessionId === 'string', 'claude session spawned with owned session id')
    await clickSession('smoke-claude')
    await sleep(9000)
    await screenshot('03-claude.png')
    const claudeAttach = await evaluate(`window.nmux.sessions.attach(${JSON.stringify(claude.id)})`)
    const screen = strip(claudeAttach.snapshot)
    check(/Claude Code/.test(screen), 'Claude Code TUI rendered')
    console.log(screen.split('\n').filter((l) => l.trim()).slice(0, 6).map((l) => '   | ' + l.trimEnd()).join('\n'))

    await clickSession('smoke-shell')
    await sleep(1000)
    await screenshot('04-switch-back.png')
  } else {
    console.log('skipping Claude part')
  }
} finally {
  // Always clean up what we created, even on failure.
  try {
    const list = await evaluate('window.nmux.sessions.list()')
    for (const s of list) if (s.name.startsWith('smoke-')) await evaluate(`window.nmux.sessions.remove(${JSON.stringify(s.id)})`)
    const left = await evaluate('window.nmux.sessions.list()')
    check(!left.some((s) => s.name.startsWith('smoke-')), 'smoke sessions removed')
  } catch (err) {
    console.error('cleanup failed:', err)
    failures++
  }
  ws.close()
  if (child) {
    await sleep(500)
    child.kill()
  }
}

console.log(failures === 0 ? 'SMOKE PASSED' : `SMOKE FAILED (${failures})`)
process.exit(failures === 0 ? 0 : 1)
