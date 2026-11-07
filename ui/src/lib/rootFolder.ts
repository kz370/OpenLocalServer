import { useCallback, useState } from 'react'

/**
 * The folder that holds a project or site folder: `I:\OLS\sites` for `I:\OLS\sites\phpdev`.
 * Folders added by hand share a root with auto-discovered ones when they sit in the same parent.
 */
export function rootFolderOf(path: string | null | undefined): string {
  if (!path) return ''
  const trimmed = path.replace(/[\\/]+$/, '')
  const cut = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'))
  if (cut <= 0) return trimmed
  const parent = trimmed.slice(0, cut)
  // `C:` on its own means "the current folder on C:", so keep the slash for a drive root.
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent
}

/** Same root regardless of slash direction, trailing slash or letter case (Windows paths). */
export function rootKeyOf(path: string | null | undefined): string {
  return rootFolderOf(path).replace(/\//g, '\\').toLowerCase()
}

/** Group key for reverse-proxy rows: they have no folder, so they form their own group. */
export const PROXY_ROOT = '\u0001proxy'

/** Orders by root folder first, then by name; rows with no folder go last. */
export function compareByRoot(rootA: string, rootB: string, nameA: string, nameB: string): number {
  if (rootA !== rootB) {
    if (rootA === '') return 1
    if (rootB === '') return -1
    if (rootA === PROXY_ROOT) return 1
    if (rootB === PROXY_ROOT) return -1
    return rootA.localeCompare(rootB)
  }
  return nameA.localeCompare(nameB)
}

export type ListSort = 'name' | 'root'

export interface ListView {
  sort: ListSort
  /** Show a header row above each run of rows that share a root folder (also sorts by root). */
  group: boolean
}

const DEFAULT_VIEW: ListView = { sort: 'name', group: false }

function readView(key: string): ListView {
  try {
    const raw = window.localStorage.getItem(key)
    if (!raw) return DEFAULT_VIEW
    const parsed = JSON.parse(raw) as Partial<ListView>
    return {
      sort: parsed.sort === 'root' ? 'root' : 'name',
      group: parsed.group === true,
    }
  } catch {
    return DEFAULT_VIEW
  }
}

/** Sort and grouping choice for one table, remembered between sessions. */
export function useListView(storageKey: string): [ListView, (next: ListView) => void] {
  const [view, setView] = useState<ListView>(() => readView(storageKey))
  const update = useCallback(
    (next: ListView) => {
      setView(next)
      try {
        window.localStorage.setItem(storageKey, JSON.stringify(next))
      } catch {
        // the choice just will not be remembered
      }
    },
    [storageKey],
  )
  return [view, update]
}

/** True when the rows should be ordered by root folder rather than by name alone. */
export function sortsByRoot(view: ListView): boolean {
  return view.group || view.sort === 'root'
}
