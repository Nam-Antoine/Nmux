import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'
import type { Op, Reply, ServerEvent } from './types'

export function call(op: Op): Promise<Reply> {
  return invoke<Reply>('call', { op })
}

export function send(op: Op): void {
  call(op).catch((err) => console.warn('[nmux]', op.op, err))
}

export function onServerEvent(handler: (event: ServerEvent) => void): Promise<() => void> {
  return listen<ServerEvent>('nmux://event', (e) => handler(e.payload))
}

export function onConnected(handler: () => void): Promise<() => void> {
  return listen('nmux://connected', () => handler())
}

export function onDisconnected(handler: (why: string) => void): Promise<() => void> {
  return listen<string>('nmux://disconnected', (e) => handler(e.payload))
}

export async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title: 'Choose a project folder' })
  return typeof picked === 'string' ? picked : null
}

export function openCloudUrl(url: string): void {
  invoke('open_cloud_url', { url }).catch((err) => console.warn('[nmux] open link', err))
}

export async function notify(title: string, body: string): Promise<void> {
  try {
    let granted = await isPermissionGranted()
    if (!granted) granted = (await requestPermission()) === 'granted'
    if (granted) sendNotification({ title, body })
  } catch (err) {
    console.warn('[nmux] notification', err)
  }
}

export function openPath(path: string): Promise<void> {
  return invoke('open_path', { path })
}

export function revealPath(path: string): Promise<void> {
  return invoke('reveal_path', { path })
}

export function openLocalPort(port: number): void {
  invoke('open_local_port', { port }).catch((err) => console.warn('[nmux] open port', err))
}

export function onFileDrop(handler: (paths: string[], x: number, y: number) => void): Promise<() => void> {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== 'drop') return
    const { paths, position } = event.payload
    handler(paths, position.x / window.devicePixelRatio, position.y / window.devicePixelRatio)
  })
}

export const appWindow = {
  minimize: (): void => void getCurrentWindow().minimize(),
  toggleMaximize: (): void => void getCurrentWindow().toggleMaximize(),
  close: (): void => void getCurrentWindow().close()
}
