import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  // Tauri serves the built files from its own protocol; paths must be relative.
  base: './',
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022', chunkSizeWarningLimit: 1500 }
})
