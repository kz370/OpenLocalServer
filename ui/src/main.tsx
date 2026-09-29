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
    // The boot background stays until here, so any earlier reveal — the Rust fallback
    // timer, or a slow first paint — still shows the app's background rather than a
    // see-through frame. Removed in the same frame the reveal is requested.
    document.documentElement.classList.remove('booting')
    void emit('ols:ui-ready').catch(() => undefined)
  })
})

// Without a listener a rejected `runCommand` is dropped on the floor: the page that was
// waiting on it keeps its loading state forever and the reason is never stated. This is
// the last net under any call site that forgets a `.catch`.
window.addEventListener('unhandledrejection', (e) => {
  console.error('unhandled rejection', e.reason)
})
