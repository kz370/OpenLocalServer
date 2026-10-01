import { useCallback, useEffect, useRef, useState } from 'react'

import { listen } from '@tauri-apps/api/event'

import { asDiagnostic } from '@/components/ErrorCard'
import type { Diagnostic } from '@/core'
import { useTheme } from '@/lib/theme'

/** Runs `fn` now and then every `ms` while the component is mounted. Skips a tick while the previous one is still pending, so slow core commands (dashboard, services) never pile up and starve the IPC thread pool. */
export function usePoll(fn: () => void | Promise<void>, ms: number) {
  const ref = useRef(fn)
  ref.current = fn
  useEffect(() => {
    let alive = true
    let pending = false
    const tick = () => {
      if (!alive || pending) return
      const r = ref.current()
      if (r && typeof (r as Promise<void>).finally === 'function') {
        pending = true
        void (r as Promise<void>).finally(() => {
          pending = false
        })
      }
    }
    tick()
    const id = setInterval(tick, ms)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [ms])
}

/**
 * Wraps an async action with busy + error state, so every button reports failures the same
 * way, and remembers which key last finished successfully.
 *
 * `saved` is what makes a save feel like it happened: a command that answers in 80 ms gives
 * the eye nothing at all, so the button is pressed and the screen is unchanged and the user
 * presses it again. It holds the key for {@link SAVED_FLASH_MS} and then clears itself, and
 * `<SaveButton>` turns that into a "Saved" mark on the button itself.
 */
export function useAction() {
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [saved, setSaved] = useState<string | null>(null)
  const flash = useRef<ReturnType<typeof setTimeout>>(undefined)
  useEffect(() => () => clearTimeout(flash.current), [])
  const run = useCallback(async <T,>(key: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(key)
    setError(null)
    try {
      const r = await fn()
      clearTimeout(flash.current)
      setSaved(key)
      flash.current = setTimeout(() => setSaved(null), SAVED_FLASH_MS)
      return r
    } catch (e) {
      setError(asDiagnostic(e))
      return undefined
    } finally {
      setBusy(null)
    }
  }, [])
  return { busy, error, setError, saved, run }
}

/** How long a "Saved" mark stays on a button before it clears itself. */
export const SAVED_FLASH_MS = 1600

export function timeAgo(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000))
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.round(s / 60)}m ago`
  if (s < 86400) return `${Math.round(s / 3600)}h ago`
  return `${Math.round(s / 86400)}d ago`
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 ** 3).toFixed(1)} GB`
}

/** Exit animation length shared with the `.modal-*` rules in index.css. */
export const MODAL_MS = 160

/**
 * True when nothing is running, so the app can show the red mark. The backend owns
 * the decision (it also drives the tray and taskbar) and pushes it as
 * `ols:status-icon`; without Tauri it falls back to "not stopped".
 *
 * The event is only ever pushed, never fetched, so this state starts at "running"
 * and is corrected by the first event that arrives. `show_main_window` on the
 * Rust side emits the current mark on every reveal for that reason — a window
 * opened from the tray has no `ols:ui-ready` to resync against.
 */
export function useServerStopped(): boolean {
  const [stopped, setStopped] = useState(false)
  useEffect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
    let alive = true
    let unlisten: (() => void) | undefined
    void listen<boolean>('ols:status-icon', (event) => {
      if (alive) setStopped(event.payload)
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])
  return stopped
}

/**
 * Path to the app mark for the current theme and service state — the same four
 * colourways the tray and the taskbar show, so the sidebar, the titlebar and the
 * About card never disagree with the icons outside the window.
 *
 * Must be called inside `ThemeProvider`; components outside it have no resolved
 * theme to pick from.
 */
export function useAppMarkPath(): string {
  const { resolvedTheme } = useTheme()
  const stopped = useServerStopped()
  if (resolvedTheme === 'dark') return stopped ? '/favicon-stopped-dark.svg' : '/favicon-dark.svg'
  return stopped ? '/favicon-stopped.svg' : '/favicon.svg'
}

/** Keeps a modal mounted while its exit animation plays. `state` drives `data-state`. */
export function usePresence(open: boolean, ms = MODAL_MS) {
  const [mounted, setMounted] = useState(open)
  useEffect(() => {
    if (open) {
      setMounted(true)
      return
    }
    const id = setTimeout(() => setMounted(false), ms)
    return () => clearTimeout(id)
  }, [open, ms])
  return { mounted: open || mounted, state: open ? ('open' as const) : ('closed' as const) }
}

/** For modals the parent unmounts on close: plays the exit animation, then calls `onClose`. */
export function useAnimatedClose(onClose: () => void, ms = MODAL_MS) {
  const [closing, setClosing] = useState(false)
  const ref = useRef(onClose)
  ref.current = onClose
  const close = useCallback(() => {
    setClosing((c) => {
      if (!c) setTimeout(() => ref.current(), ms)
      return true
    })
  }, [ms])
  return { state: closing ? ('closed' as const) : ('open' as const), close }
}
