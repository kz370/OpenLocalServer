import { useEffect, useState } from 'react'

import { runCommand } from '@/core'
import { useHiddenSidebarItems } from '@/lib/sidebarVisibility'

/** Core setting (app.db): show "Dockerize…" in the Projects and Sites right-click menus. Default on. */
export const DOCKERIZE_MENU_KEY = 'ui.docker.dockerize_menu'

let enabled = true
let loaded: Promise<void> | null = null
const listeners = new Set<(v: boolean) => void>()

function publish(v: boolean) {
  enabled = v
  for (const l of listeners) l(v)
}

function load(): Promise<void> {
  if (!loaded) {
    loaded = runCommand({ type: 'get_setting', key: DOCKERIZE_MENU_KEY })
      .then((r) => {
        if (r.type === 'setting') publish(r.value !== false)
      })
      .catch(() => {
        loaded = null
      })
  }
  return loaded
}

/** The user's own switch (Settings → Sidebar), ignoring whether the Docker page is shown. */
export function useDockerizeMenuSetting(): boolean {
  const [state, setState] = useState(enabled)
  useEffect(() => {
    listeners.add(setState)
    void load()
    return () => {
      listeners.delete(setState)
    }
  }, [])
  return state
}

/**
 * Whether the right-click "Dockerize…" items show: the switch is on AND the Docker page is
 * not hidden from the sidebar (hiding Docker hides its shortcuts too).
 */
export function useDockerizeMenu(): boolean {
  const setting = useDockerizeMenuSetting()
  const hidden = useHiddenSidebarItems()
  return setting && !hidden.has('docker')
}

/** Saves the switch. It applies at once; a failed save is reverted and rethrown. */
export async function setDockerizeMenu(on: boolean): Promise<void> {
  const before = enabled
  publish(on)
  try {
    await runCommand({ type: 'set_setting', key: DOCKERIZE_MENU_KEY, value: on })
  } catch (err) {
    publish(before)
    throw err
  }
}
