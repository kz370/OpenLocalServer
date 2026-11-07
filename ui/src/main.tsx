import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { emit } from '@tauri-apps/api/event'
import './index.css'
import App from './App.tsx'
import { TrayConfirmationWindow } from './components/TrayConfirmationWindow'
import { installSiteCacheInvalidation } from './lib/siteCaches'
import { ThemeProvider } from './lib/theme'

const isTrayConfirmationWindow = window.location.hash === '#tray-confirmation'
if (isTrayConfirmationWindow) {
  document.documentElement.classList.add('tray-confirmation-window')
}

if (!isTrayConfirmationWindow) {
  // A delete or edit anywhere (another page, the CLI, the API) clears the cached site and
  // project lists, so the next page that opens rebuilds them instead of painting stale rows.
  installSiteCacheInvalidation()
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      {isTrayConfirmationWindow ? <TrayConfirmationWindow /> : <App />}
    </ThemeProvider>
  </StrictMode>,
)

// The main window is created hidden and revealed by `reveal_window_on_ui_ready`
// once the UI is on screen. The preloaded confirmation window signals readiness
// after its request listener is installed.
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    document.documentElement.classList.remove('booting')
    if (!isTrayConfirmationWindow) {
      void emit('ols:ui-ready').catch((error: unknown) => {
        console.error('Could not report that the main window is ready:', error)
      })
    }
  })
})

// Without a listener a rejected `runCommand` is dropped on the floor: the page that was
// waiting on it keeps its loading state forever and the reason is never stated. This is
// the last net under any call site that forgets a `.catch`.
window.addEventListener('unhandledrejection', (e) => {
  console.error('unhandled rejection', e.reason)
})
