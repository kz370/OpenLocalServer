import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  clearScreen: false,
  build: {
    // Main bundle ~1.5 MB (xterm + CodeMirror). Warn only past 1600 kB.
    chunkSizeWarningLimit: 1600,
  },
  server: {
    port: 1420,
    strictPort: true,
  },
})
