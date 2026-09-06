import { resolve } from 'node:path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'

const shared = resolve('src/shared')

export default defineConfig({
  // Main process: Node + Electron APIs. `dependencies` stay external and are
  // loaded from node_modules at runtime; everything else is bundled.
  //   - node-pty: native module, cannot be bundled.
  //   - @xterm/headless + @xterm/addon-serialize: their package.json `module`
  //     field points at a missing file, which breaks Vite's resolver. Node's
  //     `require` uses `main`, which is correct, so they are kept external.
  main: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { '@shared': shared } }
  },
  // Preload: runs sandboxed. Must be self-contained, so shared code is bundled in.
  preload: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { '@shared': shared } }
  },
  // Renderer: plain web app. No Node access; talks to main only via window.nmux.
  renderer: {
    plugins: [react()],
    resolve: {
      alias: {
        '@renderer': resolve('src/renderer/src'),
        '@shared': shared
      }
    }
  }
})
