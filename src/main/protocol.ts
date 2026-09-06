/**
 * Serves the built renderer from a custom `app://nmux/` scheme instead of
 * `file://`. That gives the UI a real origin (so sender checks and CSP are
 * meaningful), lets the file protocol's extra privileges be switched off via
 * fuses, and keeps arbitrary local files out of reach of the renderer.
 */
import { readFile } from 'node:fs/promises'
import { extname, join, normalize, resolve, sep } from 'node:path'
import { app, protocol } from 'electron'

export const APP_SCHEME = 'app'
export const APP_HOST = 'nmux'

/** Strict production CSP, delivered as a header so the dev meta tag can stay looser. */
const PRODUCTION_CSP = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'", // xterm sets inline styles
  "img-src 'self' data:",
  "font-src 'self' data:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'"
].join('; ')

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2'
}

/** Must run before `app.whenReady()`. */
export function registerAppScheme(): void {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: APP_SCHEME,
      privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: false }
    }
  ])
}

/** Must run after `app.whenReady()`. */
export function serveRenderer(): void {
  const root = resolve(join(__dirname, '../renderer'))

  protocol.handle(APP_SCHEME, async (request) => {
    const url = new URL(request.url)
    if (url.host !== APP_HOST) return new Response('Not found', { status: 404 })

    let pathname: string
    try {
      pathname = decodeURIComponent(url.pathname)
    } catch {
      return new Response('Bad request', { status: 400 })
    }
    if (pathname === '/' || pathname === '') pathname = '/index.html'

    // Resolve inside the renderer directory only; reject any traversal.
    const file = normalize(join(root, pathname))
    if (file !== root && !file.startsWith(root + sep)) {
      return new Response('Forbidden', { status: 403 })
    }

    try {
      const body = await readFile(file)
      return new Response(body, {
        headers: {
          'content-type': MIME[extname(file).toLowerCase()] ?? 'application/octet-stream',
          'content-security-policy': PRODUCTION_CSP,
          'x-content-type-options': 'nosniff'
        }
      })
    } catch {
      return new Response('Not found', { status: 404 })
    }
  })
}

/** Base URL (with trailing slash) the renderer is loaded from. */
export function rendererBaseUrl(): string {
  const dev = process.env.ELECTRON_RENDERER_URL
  if (!app.isPackaged && dev) return `${new URL(dev).origin}/`
  return `${APP_SCHEME}://${APP_HOST}/`
}

/** True when `url` belongs to our own renderer. */
export function isTrustedRendererUrl(url: string | undefined | null): boolean {
  return typeof url === 'string' && url.startsWith(rendererBaseUrl())
}
