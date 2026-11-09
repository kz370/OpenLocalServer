import { useEffect, useState } from 'react'

import { runCommand } from '@/core'

/** Core setting (app.db): show the Security card on the Dashboard. Default on. */
export const DASHBOARD_SECURITY_KEY = 'ui.dashboard.security'

let enabled = true
let loaded: Promise<void> | null = null
const listeners = new Set<(v: boolean) => void>()

function publish(v: boolean) {
  enabled = v
  for (const l of listeners) l(v)
}

function load(): Promise<void> {
  if (!loaded) {
    loaded = runCommand({ type: 'get_setting', key: DASHBOARD_SECURITY_KEY })
      .then((r) => {
        if (r.type === 'setting') publish(r.value !== false)
      })
      .catch(() => {
        // Unreadable setting: keep showing the card.
        loaded = null
      })
  }
  return loaded
}

/** Whether the Dashboard shows the Security card. Updates live when Settings changes it. */
export function useDashboardSecurity(): boolean {
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

/** Saves the switch. It applies at once; a failed save is reverted and rethrown. */
export async function setDashboardSecurity(on: boolean): Promise<void> {
  const before = enabled
  publish(on)
  try {
    await runCommand({ type: 'set_setting', key: DASHBOARD_SECURITY_KEY, value: on })
  } catch (err) {
    publish(before)
    throw err
  }
}
