/**
 * Typed access to the preload bridge. Import this instead of touching
 * `window.nmux` directly so the renderer has one seam to mock in tests.
 */
import type { NmuxApi } from '@shared/ipc'

export const api: NmuxApi = window.nmux
