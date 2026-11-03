import { useEffect, useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'

import { type ServiceStatus, runCommand } from '@/core'
import { notify } from '@/lib/notify'

export type ServiceLifecycleState = 'starting' | 'stopping' | 'restarting' | 'running' | 'stopped' | 'error'

interface ServiceStateChange {
  id: string
  state: ServiceLifecycleState
}

interface ServiceStateEvent {
  services: ServiceStateChange[]
}

const listeners = new Set<() => void>()
const EMPTY_STATES: ReadonlyMap<string, ServiceLifecycleState> = new Map()
let states: ReadonlyMap<string, ServiceLifecycleState> = EMPTY_STATES
let unlisten: (() => void) | null = null
let snapshotLoading = false
let snapshotPromise: Promise<void> | null = null
let bufferedEvents: ServiceStateEvent[] = []
let pendingChanges = new Map<string, ServiceLifecycleState>()
let flushTimer: ReturnType<typeof setTimeout> | null = null

function publish(next: Map<string, ServiceLifecycleState>) {
  if (
    next.size === states.size &&
    [...next].every(([id, state]) => states.get(id) === state)
  ) {
    return
  }
  states = next
  listeners.forEach((listener) => listener())
}

function queueChanges(changes: ServiceStateChange[]) {
  for (const change of changes) {
    pendingChanges.set(change.id, change.state)
  }
  if (flushTimer !== null) return
  flushTimer = setTimeout(() => {
    const next = new Map(states)
    for (const [id, state] of pendingChanges) {
      next.set(id, state)
    }
    pendingChanges = new Map()
    flushTimer = null
    publish(next)
  }, 50)
}

function reportStatusLoadError(error: unknown) {
  let details: string
  if (typeof error === 'object' && error !== null && 'problem' in error && 'cause' in error) {
    const problem = typeof error.problem === 'string' ? error.problem : 'The service status request failed.'
    const cause = typeof error.cause === 'string' ? error.cause : 'The core did not provide a cause.'
    const fix = 'fix' in error && typeof error.fix === 'string' ? error.fix : 'Reopen the window and try again.'
    details = `${problem}\nCause: ${cause}\nFix: ${fix}`
  } else {
    const cause = error instanceof Error ? error.message : String(error)
    details = `Cause: ${cause}\nFix: Reopen the window and try again.`
  }
  notify.error('Could not load service status', { details })
}

function initializeStatusSnapshot() {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window) || snapshotPromise) {
    return
  }

  snapshotLoading = true
  bufferedEvents = []
  snapshotPromise = (async () => {
    if (!unlisten) {
      unlisten = await listen<ServiceStateEvent>('service-status-event', (event) => {
        if (snapshotLoading) {
          bufferedEvents.push(event.payload)
        } else {
          queueChanges(event.payload.services)
        }
      })
    }

    const response = await runCommand({ type: 'list_services' })
    if (response.type !== 'services') {
      throw new Error('The core returned an unexpected response for list_services.')
    }

    const snapshot = new Map<string, ServiceLifecycleState>()
    for (const service of response.services) {
      snapshot.set(service.id, service.state)
    }
    publish(snapshot)
  })()
    .catch(reportStatusLoadError)
    .finally(() => {
      snapshotLoading = false
      for (const event of bufferedEvents) {
        queueChanges(event.services)
      }
      bufferedEvents = []
      snapshotPromise = null
    })
}

export function useServiceStatuses(): ReadonlyMap<string, ServiceLifecycleState> {
  const current = useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    () => states,
    () => EMPTY_STATES,
  )

  useEffect(() => {
    initializeStatusSnapshot()
  }, [])

  return current
}

export function getServiceLifecycleState(
  service: ServiceStatus,
  statuses: ReadonlyMap<string, ServiceLifecycleState>,
): ServiceLifecycleState {
  return statuses.get(service.id) ?? service.state
}
