/**
 * Renderer hardening that is not specific to one window.
 *
 * Threat model (see docs/SECURITY.md): the renderer displays untrusted bytes
 * from arbitrary processes. If it is ever compromised, it must not be able to
 * navigate elsewhere, load remote content, open arbitrary URLs or protocols,
 * or use browser APIs it does not need.
 */
import { session, shell, type WebContents } from 'electron'
import { isTrustedRendererUrl } from './protocol'

/** Browser permissions the UI actually uses. Everything else is denied. */
const ALLOWED_PERMISSIONS = new Set<string>(['clipboard-sanitized-write', 'clipboard-read'])

/** URL schemes that may be handed to the OS browser from a terminal link. */
const EXTERNAL_SCHEMES = new Set(['http:', 'https:', 'mailto:'])

export function isSafeExternalUrl(raw: string): boolean {
  try {
    const url = new URL(raw)
    return EXTERNAL_SCHEMES.has(url.protocol) && raw.length <= 2048
  } catch {
    return false
  }
}

/** Apply once, after `app.whenReady()`. */
export function hardenDefaultSession(): void {
  const s = session.defaultSession
  s.setPermissionRequestHandler((_wc, permission, callback) => {
    callback(ALLOWED_PERMISSIONS.has(permission))
  })
  s.setPermissionCheckHandler((_wc, permission) => ALLOWED_PERMISSIONS.has(permission))
  // Nothing in the UI should ever start a download.
  s.on('will-download', (event) => event.preventDefault())
}

/** Apply to every WebContents (hook it from `app.on('web-contents-created')`). */
export function hardenWebContents(contents: WebContents): void {
  // No navigation away from our own UI, including redirects.
  contents.on('will-navigate', (event, url) => {
    if (!isTrustedRendererUrl(url)) event.preventDefault()
  })
  contents.on('will-redirect', (event, url) => {
    if (!isTrustedRendererUrl(url)) event.preventDefault()
  })
  // No <webview> tags.
  contents.on('will-attach-webview', (event) => event.preventDefault())

  // window.open / target=_blank: never open a window; hand safe links to the OS.
  contents.setWindowOpenHandler(({ url }) => {
    if (isSafeExternalUrl(url)) void shell.openExternal(url)
    return { action: 'deny' }
  })
}
