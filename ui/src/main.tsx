import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { emit } from '@tauri-apps/api/event'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from './lib/theme'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>,
)

// The native window is created hidden and revealed by `reveal_window_on_ui_ready`
// once the UI is on screen, so the user never sees the empty transparent frame.
// Two frames: the first commits the React tree, the second guarantees the browser
// has actually presented it before the window is shown.
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    void emit('ols:ui-ready').catch(() => undefined)
  })
})
