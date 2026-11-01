export type NotificationType = 'success' | 'info' | 'warning' | 'error' | 'progress'

export interface NotificationAction {
  label: string
  onClick: () => void | Promise<void>
  primary?: boolean
  isUndo?: boolean
}

export interface NotificationItem {
  id: string
  type: NotificationType
  title?: string
  message: string
  details?: string | null
  logSource?: string | null
  action?: NotificationAction
  count: number
  createdAt: number
  read?: boolean
  progress?: number | null // 0-100 or null for indeterminate
  statusText?: string
  duration?: number // auto-hide duration in ms (undefined = persistent)
}

export interface ProgressNotificationHandle {
  id: string
  update: (opts: { message?: string; statusText?: string; progress?: number | null }) => void
  success: (message: string, action?: NotificationAction) => void
  error: (message: string, opts?: { details?: string | null; logSource?: string | null }) => void
  dismiss: () => void
}

const MAX_VISIBLE = 4
const MAX_HISTORY = 50
const SUCCESS_INFO_MS = 4000
const WARNING_MS = 8000
const UNDO_ACTION_MS = 9000

const HISTORY_STORAGE_KEY = 'ols_notifications_history'

function loadHistory(): NotificationItem[] {
  if (typeof window === 'undefined') return []
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}

function saveHistory(items: NotificationItem[]) {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(items.slice(0, MAX_HISTORY)))
  } catch {
    // ignore
  }
}

// Global state
let toasts: NotificationItem[] = []
let history: NotificationItem[] = loadHistory()
const listeners = new Set<(items: NotificationItem[]) => void>()
const historyListeners = new Set<(items: NotificationItem[]) => void>()

function emitToasts() {
  listeners.forEach((fn) => fn([...toasts]))
}

function emitHistory() {
  saveHistory(history)
  historyListeners.forEach((fn) => fn([...history]))
}

function addToHistory(item: NotificationItem) {
  // Add to history (skip in-progress updates)
  if (item.type === 'progress') return
  history = [{ ...item, read: false }, ...history.filter((h) => h.id !== item.id)].slice(0, MAX_HISTORY)
  emitHistory()
}

let idCounter = 0
function nextId(): string {
  return `notify_${Date.now()}_${++idCounter}`
}

function pushNotification(
  type: NotificationType,
  message: string,
  opts: {
    id?: string
    title?: string
    details?: string | null
    logSource?: string | null
    action?: NotificationAction
    duration?: number
    progress?: number | null
    statusText?: string
  } = {},
): string {
  const id = opts.id || nextId()

  // De-duplication check: if consecutive identical toast exists, collapse and reset timer
  const existingIndex = toasts.findIndex((t) => t.type === type && t.message === message && !t.action)
  if (existingIndex !== -1) {
    const existing = toasts[existingIndex]
    const updated: NotificationItem = {
      ...existing,
      count: existing.count + 1,
      createdAt: Date.now(),
      details: opts.details ?? existing.details,
      duration: opts.duration ?? existing.duration,
    }
    toasts[existingIndex] = updated
    emitToasts()
    return existing.id
  }

  // Calculate default auto-hide duration
  let duration = opts.duration
  if (duration === undefined) {
    if (type === 'progress') {
      duration = undefined // Progress never auto-hides while active
    } else if (type === 'error') {
      duration = undefined // Errors never auto-hide
    } else if (opts.action) {
      duration = opts.action.isUndo ? UNDO_ACTION_MS : undefined // Actions stay unless Undo
    } else if (type === 'warning') {
      duration = WARNING_MS
    } else {
      duration = SUCCESS_INFO_MS
    }
  }

  const item: NotificationItem = {
    id,
    type,
    title: opts.title,
    message,
    details: opts.details,
    logSource: opts.logSource,
    action: opts.action,
    count: 1,
    createdAt: Date.now(),
    read: false,
    progress: opts.progress,
    statusText: opts.statusText,
    duration,
  }

  toasts = [item, ...toasts].slice(0, MAX_VISIBLE)
  emitToasts()
  addToHistory(item)
  return id
}

export function dismissNotification(id: string) {
  toasts = toasts.filter((t) => t.id !== id)
  emitToasts()
}

export function dismissNewestNotification(): boolean {
  if (toasts.length === 0) return false
  toasts = toasts.slice(1)
  emitToasts()
  return true
}

export function subscribeNotifications(fn: (items: NotificationItem[]) => void): () => void {
  listeners.add(fn)
  fn([...toasts])
  return () => {
    listeners.delete(fn)
  }
}

export function subscribeNotificationHistory(fn: (items: NotificationItem[]) => void): () => void {
  historyListeners.add(fn)
  fn([...history])
  return () => {
    historyListeners.delete(fn)
  }
}

export function clearNotificationHistory() {
  history = []
  emitHistory()
}

export function markAllNotificationsRead() {
  let changed = false
  history = history.map((item) => {
    if (!item.read) {
      changed = true
      return { ...item, read: true }
    }
    return item
  })
  if (changed) emitHistory()
}

export function getUnreadNotificationCount(): number {
  return history.filter((h) => !h.read).length
}

export const notify = {
  success(message: string, opts?: { title?: string; action?: NotificationAction; duration?: number }) {
    return pushNotification('success', message, opts)
  },

  info(message: string, opts?: { title?: string; action?: NotificationAction; duration?: number }) {
    return pushNotification('info', message, opts)
  },

  warning(message: string, opts?: { title?: string; action?: NotificationAction; duration?: number; details?: string | null }) {
    return pushNotification('warning', message, opts)
  },

  error(
    message: string,
    opts?: {
      title?: string
      details?: string | null
      logSource?: string | null
      action?: NotificationAction
    },
  ) {
    return pushNotification('error', message, opts)
  },

  progress(title: string, initialStatus?: string): ProgressNotificationHandle {
    const id = nextId()
    pushNotification('progress', title, {
      id,
      title,
      statusText: initialStatus,
      progress: null,
      duration: undefined,
    })

    return {
      id,
      update({ message, statusText, progress }) {
        const idx = toasts.findIndex((t) => t.id === id)
        if (idx !== -1) {
          toasts[idx] = {
            ...toasts[idx],
            message: message ?? toasts[idx].message,
            statusText: statusText ?? toasts[idx].statusText,
            progress: progress !== undefined ? progress : toasts[idx].progress,
          }
          emitToasts()
        }
      },
      success(msg, action) {
        dismissNotification(id)
        pushNotification('success', msg, { action })
      },
      error(msg, opts) {
        dismissNotification(id)
        pushNotification('error', msg, opts)
      },
      dismiss() {
        dismissNotification(id)
      },
    }
  },

  batch({
    operation,
    total,
    succeeded,
    failed,
    failedItems = [],
    onDetails,
  }: {
    operation: string
    total: number
    succeeded: number
    failed: number
    failedItems?: string[]
    onDetails?: () => void
  }) {
    if (failed === 0) {
      return notify.success(`All ${succeeded} ${operation} finished successfully.`)
    }
    if (succeeded === 0) {
      return notify.error(`All ${total} ${operation} failed.`, {
        details: failedItems.length > 0 ? failedItems.join('\n') : null,
        action: onDetails ? { label: 'View details', onClick: onDetails } : undefined,
      })
    }
    return notify.warning(`${succeeded} ${operation} succeeded, ${failed} failed.`, {
      details: failedItems.length > 0 ? failedItems.join('\n') : null,
      action: onDetails ? { label: 'View details', onClick: onDetails } : undefined,
    })
  },
}

// Background Task and Runtime Event Listeners
let backgroundListenersInitialized = false
const activeProgressHandles = new Map<string, ProgressNotificationHandle>()

export function initGlobalNotificationListeners() {
  if (backgroundListenersInitialized || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  backgroundListenersInitialized = true

  void import('@tauri-apps/api/event').then(({ listen }) => {
    // Listen to background tasks (§164)
    void listen<{ task: { id: string; title: string; step?: string | null; state: string; total: number; done: number; problems: string[] } }>(
      'task-event',
      (event) => {
        const task = event.payload.task
        const key = `task:${task.id}`

        if (task.state === 'running') {
          let handle = activeProgressHandles.get(key)
          if (!handle) {
            handle = notify.progress(task.title, task.step ?? undefined)
            activeProgressHandles.set(key, handle)
          } else {
            const pct = task.total > 0 ? Math.round((task.done / task.total) * 100) : null
            handle.update({
              statusText: task.step ?? undefined,
              progress: pct,
            })
          }
        } else if (task.state === 'succeeded') {
          const handle = activeProgressHandles.get(key)
          if (handle) {
            handle.success(`${task.title} finished successfully.`)
            activeProgressHandles.delete(key)
          }
        } else if (task.state === 'failed') {
          const handle = activeProgressHandles.get(key)
          const details = task.problems.length > 0 ? task.problems.join('\n') : null
          if (handle) {
            handle.error(`${task.title} failed.`, { details })
            activeProgressHandles.delete(key)
          } else {
            notify.error(`${task.title} failed.`, { details })
          }
        } else if (task.state === 'cancelled') {
          const handle = activeProgressHandles.get(key)
          if (handle) {
            handle.dismiss()
            activeProgressHandles.delete(key)
          }
          notify.info(`${task.title} was cancelled.`)
        }
      },
    )

    // Listen to runtime events (downloads, installs)
    void listen<{ kind: string; id: string; version: string; state?: string; total?: number; done?: number; message?: string }>(
      'runtime-event',
      (event) => {
        const e = event.payload
        const key = `runtime:${e.id}@${e.version}`

        if (e.kind === 'progress') {
          let handle = activeProgressHandles.get(key)
          const title = `${e.id.toUpperCase()} ${e.version}`
          if (!handle) {
            handle = notify.progress(title, e.state)
            activeProgressHandles.set(key, handle)
          } else {
            const pct = e.total && e.total > 0 ? Math.round(((e.done || 0) / e.total) * 100) : null
            handle.update({
              statusText: `${e.state}${pct !== null ? ` (${pct}%)` : ''}`,
              progress: pct,
            })
          }
        } else if (e.kind === 'installed') {
          const handle = activeProgressHandles.get(key)
          const msg = `${e.id.toUpperCase()} ${e.version} installed successfully.`
          if (handle) {
            handle.success(msg)
            activeProgressHandles.delete(key)
          } else {
            notify.success(msg)
          }
        } else if (e.kind === 'failed') {
          const handle = activeProgressHandles.get(key)
          const msg = `Failed to install ${e.id.toUpperCase()} ${e.version}.`
          if (handle) {
            handle.error(msg, { details: e.message })
            activeProgressHandles.delete(key)
          } else {
            notify.error(msg, { details: e.message })
          }
        } else if (e.kind === 'cancelled') {
          const handle = activeProgressHandles.get(key)
          if (handle) {
            handle.dismiss()
            activeProgressHandles.delete(key)
          }
        }
      },
    )
  })
}
