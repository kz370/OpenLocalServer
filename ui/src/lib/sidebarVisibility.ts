import { useEffect, useState } from 'react'

import { LOCKED_NAV_ITEMS, type Page } from '@/components/layout/navItems'
import { runCommand } from '@/core'

/** Core setting (app.db) holding the sidebar items the user hid: an array of page ids. */
export const SIDEBAR_HIDDEN_KEY = 'ui.sidebar.hidden'

let hidden: ReadonlySet<Page> = new Set()
let loaded: Promise<void> | null = null
const listeners = new Set<(h: ReadonlySet<Page>) => void>()

/** Locked items are dropped, so a hand-edited or older setting can never hide them. */
function sanitize(value: unknown): Set<Page> {
  if (!Array.isArray(value)) return new Set()
  return new Set(
    value.filter((v): v is Page => typeof v === 'string' && !LOCKED_NAV_ITEMS.has(v as Page)),
  )
}

function publish(next: ReadonlySet<Page>) {
  hidden = next
  for (const l of listeners) l(next)
}

/** Reads the setting once per app session; later callers share the same answer. */
function load(): Promise<void> {
  if (!loaded) {
    loaded = runCommand({ type: 'get_setting', key: SIDEBAR_HIDDEN_KEY })
      .then((r) => {
        if (r.type === 'setting') publish(sanitize(r.value))
      })
      .catch(() => {
        // Unreadable setting: show everything rather than fail the sidebar.
        loaded = null
      })
  }
  return loaded
}

/** The sidebar items currently hidden. Updates live when Settings changes them. */
export function useHiddenSidebarItems(): ReadonlySet<Page> {
  const [state, setState] = useState(hidden)
  useEffect(() => {
    listeners.add(setState)
    void load()
    return () => {
      listeners.delete(setState)
    }
  }, [])
  return state
}

/** Saves the hidden list. The sidebar updates at once; a failed save is reverted and rethrown. */
export async function setHiddenSidebarItems(next: Iterable<Page>): Promise<void> {
  const before = hidden
  const clean = sanitize([...next])
  publish(clean)
  try {
    await runCommand({ type: 'set_setting', key: SIDEBAR_HIDDEN_KEY, value: [...clean].sort() })
  } catch (err) {
    publish(before)
    throw err
  }
}
