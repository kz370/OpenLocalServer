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
  /** Drops the stored value: memory and storage go back to the default until it is rebuilt. */
  clear(): void
}

/** Every live cache by key, so the in-memory copy can be reset or replaced as well as storage. */
const registry = new Map<string, { clear: () => void; set: (value: unknown) => void }>()

export function createPersistentCache<T>(key: string, defaultValue: T): PersistentCache<T> {
  let memoryValue: T = getCached(key, defaultValue)
  const clear = () => {
    memoryValue = defaultValue
    removeCached(key)
  }
  const set = (value: unknown) => {
    memoryValue = value as T
    setCached(key, memoryValue)
  }
  registry.set(key, { clear, set })

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
    clear,
  }
}

/**
 * Clears the named caches (memory and storage). Pages that mounted before the clear keep the
 * data they already fetched; the next mount starts empty and rebuilds from the backend instead
 * of painting a stale list first.
 */
export function clearCaches(keys: readonly string[]): void {
  for (const key of keys) {
    const entry = registry.get(key)
    if (entry) entry.clear()
    else removeCached(key)
  }
}

/** Replaces the named caches with fresh data (memory and storage), so no page paints a stale copy. */
export function writeCaches(keys: readonly string[], value: unknown): void {
  for (const key of keys) {
    const entry = registry.get(key)
    if (entry) entry.set(value)
    else setCached(key, value)
  }
}

