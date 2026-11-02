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
  needsAttention?: boolean
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
    if (!Array.isArray(parsed)) return []
    return (parsed as NotificationItem[])
      .filter((item) => {
        if (!item || typeof item.message !== 'string') return false
        // Drop legacy pre-fix startup diagnostics and false stop-exit history entries
        if (
          (item.logSource === 'diagnostics' &&
            (item.message === 'Diagnostics: All issues resolved.' ||
              /^Diagnostics:\s*\d+\s+issues?\s+require/i.test(item.message))) ||
          item.title === 'Service exited unexpectedly'
        ) {
          return false
        }
        return true
      })
      .map((item) => ({
        ...item,
        // Rule 6: Previous-session items do not carry an unread badge into a new launch
        read: true,
      }))
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

export const MERGE_WINDOW_MS = 5 * 60 * 1000 // 5 minutes

export function canMergeNotification(
  prev: NotificationItem,
  type: NotificationType,
  message: string,
  logSource?: string | null,
  details?: string | null,
): boolean {
  if (prev.type !== type) return false
  if (prev.message !== message) return false
  if ((prev.logSource ?? null) !== (logSource ?? null)) return false
  if ((prev.details ?? null) !== (details ?? null)) return false
  if (Date.now() - prev.createdAt > MERGE_WINDOW_MS) return false
  return true
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
    toast?: boolean
    read?: boolean
    needsAttention?: boolean
  } = {},
): string {
  const showToast = opts.toast !== false
  const now = Date.now()
  let targetId = opts.id || nextId()

  // 1. Toast de-duplication: only merge with consecutive toasts[0] within 5 minutes
  if (showToast && toasts.length > 0 && !opts.action && canMergeNotification(toasts[0], type, message, opts.logSource, opts.details)) {
    const prev = toasts[0]
    targetId = prev.id
    const updated: NotificationItem = {
      ...prev,
      count: prev.count + 1,
      createdAt: now,
      duration: opts.duration ?? prev.duration,
    }
    toasts[0] = updated
    emitToasts()
  } else if (showToast) {
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
      id: targetId,
      type,
      title: opts.title,
      message,
      details: opts.details,
      logSource: opts.logSource,
      action: opts.action,
      count: 1,
      createdAt: now,
      read: opts.read ?? false,
      progress: opts.progress,
      statusText: opts.statusText,
      duration,
      needsAttention: opts.needsAttention,
    }

    const existingToastIdx = opts.id ? toasts.findIndex((t) => t.id === opts.id) : -1
    if (existingToastIdx !== -1) {
      toasts[existingToastIdx] = item
    } else {
      toasts = [item, ...toasts].slice(0, MAX_VISIBLE)
    }
    emitToasts()
  }

  // 2. History de-duplication: replace by explicit id or merge with consecutive history[0] within 5 minutes
  if (type !== 'progress') {
    const existingHistoryIdx = opts.id ? history.findIndex((h) => h.id === opts.id) : -1
    if (existingHistoryIdx !== -1) {
      history[existingHistoryIdx] = {
        id: targetId,
        type,
        title: opts.title,
        message,
        details: opts.details,
        logSource: opts.logSource,
        action: opts.action,
        count: 1,
        createdAt: now,
        read: opts.read ?? false,
        duration: opts.duration,
        needsAttention: opts.needsAttention,
      }
      emitHistory()
    } else if (history.length > 0 && !opts.action && canMergeNotification(history[0], type, message, opts.logSource, opts.details)) {
      const prev = history[0]
      const updated: NotificationItem = {
        ...prev,
        count: prev.count + 1,
        createdAt: now,
        read: prev.read, // unread stays unread and counts as one unread item
        needsAttention: opts.needsAttention ?? prev.needsAttention,
      }
      history[0] = updated
      emitHistory()
    } else {
      const historyItem: NotificationItem = {
        id: targetId,
        type,
        title: opts.title,
        message,
        details: opts.details,
        logSource: opts.logSource,
        action: opts.action,
        count: 1,
        createdAt: now,
        read: opts.read ?? false,
        duration: opts.duration,
        needsAttention: opts.needsAttention,
      }
      history = [historyItem, ...history].slice(0, MAX_HISTORY)
      emitHistory()
    }
  }

  return targetId
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

export function removeNotificationHistoryItem(id: string) {
  history = history.filter((h) => h.id !== id)
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

export function isAttentionItem(item: NotificationItem): boolean {
  if (item.needsAttention !== undefined) return item.needsAttention
  return item.type === 'error' || item.type === 'warning'
}

export function getUnreadNotificationCount(): number {
  return history.filter((h) => !h.read && isAttentionItem(h)).length
}

export const notify = {
  success(
    message: string,
    opts?: {
      title?: string
      action?: NotificationAction
      duration?: number
      logSource?: string | null
      details?: string | null
    },
  ) {
    return pushNotification('success', message, opts)
  },

  info(
    message: string,
    opts?: {
      title?: string
      action?: NotificationAction
      duration?: number
      logSource?: string | null
      details?: string | null
    },
  ) {
    return pushNotification('info', message, opts)
  },

  warning(
    message: string,
    opts?: {
      title?: string
      action?: NotificationAction
      duration?: number
      details?: string | null
      logSource?: string | null
    },
  ) {
    return pushNotification('warning', message, opts)
  },

  error(
    message: string,
    opts?: {
      id?: string
      title?: string
      details?: string | null
      logSource?: string | null
      action?: NotificationAction
    },
  ) {
    return pushNotification('error', message, opts)
  },

  recordHistory(
    type: NotificationType,
    message: string,
    opts: {
      title?: string
      details?: string | null
      logSource?: string | null
      read?: boolean
      needsAttention?: boolean
    } = {},
  ) {
    return pushNotification(type, message, {
      ...opts,
      toast: false,
    })
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

  void import('@/lib/diagnosticsNotify').then(({ initDiagnosticsNotificationMonitor }) => {
    initDiagnosticsNotificationMonitor()
  })

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
