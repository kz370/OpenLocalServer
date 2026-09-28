import { useCallback, useEffect, useRef, useState } from 'react'

import { listen } from '@tauri-apps/api/event'

import { asDiagnostic } from '@/components/ErrorCard'
import type { Diagnostic } from '@/core'

/** Runs `fn` now and then every `ms` while the component is mounted. */
export function usePoll(fn: () => void | Promise<void>, ms: number) {
  const ref = useRef(fn)
  ref.current = fn
  useEffect(() => {
    let alive = true
    const tick = () => {
      if (alive) void ref.current()
    }
    tick()
    const id = setInterval(tick, ms)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [ms])
}

/** Wraps an async action with busy + error state, so every button reports failures the same way. */
export function useAction() {
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const run = useCallback(async <T,>(key: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(key)
    setError(null)
    try {
      return await fn()
    } catch (e) {
      setError(asDiagnostic(e))
      return undefined
    } finally {
      setBusy(null)
    }
  }, [])
  return { busy, error, setError, run }
}

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
