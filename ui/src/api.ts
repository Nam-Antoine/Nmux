// The one seam between the UI and the Rust shell.
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification'
import type { Op, Reply, ServerEvent } from './types'

/** Send one request to nmux-server. Rejects with the server's reason. */
export function call(op: Op): Promise<Reply> {
  return invoke<Reply>('call', { op })
}

/** Fire-and-forget, for input and resize where a lost call cannot be acted on anyway. */
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
