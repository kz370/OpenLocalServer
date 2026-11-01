/**
 * Cross-session persistent cache using browser / WebView2 localStorage.
 * Keeps synchronous in-memory references so initial renders take 0ms
 * even after full application restarts.
 */

const CACHE_PREFIX = 'ols_cache_'

export function getCached<T>(key: string, defaultValue: T): T {
  if (typeof window === 'undefined' || !window.localStorage) {
    return defaultValue
  }
  try {
    const raw = window.localStorage.getItem(CACHE_PREFIX + key)
    if (raw === null || raw === undefined) return defaultValue
    return JSON.parse(raw) as T
  } catch {
    return defaultValue
  }
}

export function setCached<T>(key: string, value: T): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return
  }
  try {
    window.localStorage.setItem(CACHE_PREFIX + key, JSON.stringify(value))
  } catch {
    // ignore storage quota / serialization errors
  }
}

export function removeCached(key: string): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return
  }
  try {
    window.localStorage.removeItem(CACHE_PREFIX + key)
  } catch {
    // ignore
  }
}

export interface PersistentCache<T> {
  get(): T
  set(next: T | ((prev: T) => T)): T
}

export function createPersistentCache<T>(key: string, defaultValue: T): PersistentCache<T> {
  let memoryValue: T = getCached(key, defaultValue)

  return {
    get(): T {
      return memoryValue
    },
    set(next: T | ((prev: T) => T)): T {
      if (typeof next === 'function') {
        memoryValue = (next as (prev: T) => T)(memoryValue)
      } else {
        memoryValue = next
      }
      setCached(key, memoryValue)
      return memoryValue
    },
  }
}

