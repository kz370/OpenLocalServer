import { useCallback, useEffect, useRef, useState } from 'react'

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
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
