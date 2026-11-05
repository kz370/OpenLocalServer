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
    // Tauri loads assets from local disk, so chunk size only affects parse time,
    // not download. Split big vendor libs into their own chunks so no single
    // chunk crosses the warning limit. Static imports keep load order unchanged.
    chunkSizeWarningLimit: 1600,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'vendor-codemirror', test: /node_modules[\\/](@codemirror|@lezer|codemirror|@marijn|style-mod|w3c-keyname|crelt)[\\/]/ },
            { name: 'vendor-xterm', test: /node_modules[\\/]@xterm[\\/]/ },
            { name: 'vendor-icons', test: /node_modules[\\/](simple-icons|lucide-react|@fortawesome)[\\/]/ },
            { name: 'vendor-react', test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            { name: 'vendor', test: /node_modules[\\/]/ },
          ],
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
})
