import type { NmuxApi } from '../shared/ipc'

declare global {
  interface Window {
    nmux: NmuxApi
  }
}

export {}
