import assert from 'node:assert/strict'
import test from 'node:test'

import {
  type NotificationItem,
  clearNotificationHistory,
  dismissAllNotifications,
  dismissNewestNotification,
  dismissNotification,
  evaluateNotificationConditions,
  getUnreadNotificationCount,
  markAllNotificationsRead,
  notify,
  removeNotificationHistoryItem,
  resetNotificationsForTest,
  resolveNotificationByKey,
  resolveNotificationsForService,
  subscribeNotificationHistory,
  subscribeNotifications,
} from '../notify.ts'

// Mock localStorage for test environment if needed
if (typeof globalThis.localStorage === 'undefined') {
  const store = new Map<string, string>()
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(globalThis as any).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => store.set(k, v),
    removeItem: (k: string) => store.delete(k),
    clear: () => store.clear(),
  }
}

test('notify.success creates success toast with default 4000ms duration', () => {
  let latestToasts: ReturnType<typeof notify.success>[] = []
  const unsub = subscribeNotifications((items) => {
    latestToasts = items.map((i) => i.id)
  })

  const id = notify.success('Operation succeeded')
  assert.ok(latestToasts.includes(id))

  dismissNotification(id)
  assert.ok(!latestToasts.includes(id))
  unsub()
})

test('notify deduplicates identical consecutive toasts', () => {
  let latest: { count: number; message: string } | null = null
  const unsub = subscribeNotifications((items) => {
    if (items.length > 0) {
      latest = { count: items[0].count, message: items[0].message }
    }
  })

  const id1 = notify.info('Same message')
  assert.equal(latest?.count, 1)

  const id2 = notify.info('Same message')
  assert.equal(id1, id2)
  assert.equal(latest?.count, 2)

  dismissNotification(id1)
  unsub()
})

test('notify.batch creates correct toast summary', () => {
  let latestMsg = ''
  let latestType = ''
  const unsub = subscribeNotifications((items) => {
    if (items.length > 0) {
      latestMsg = items[0].message
      latestType = items[0].type
    }
  })

  // All succeeded
  notify.batch({ operation: 'services', total: 5, succeeded: 5, failed: 0 })
  assert.equal(latestType, 'success')
  assert.match(latestMsg, /All 5 services finished successfully/)

  // Partial failure
  notify.batch({ operation: 'services', total: 5, succeeded: 3, failed: 2 })
  assert.equal(latestType, 'warning')
  assert.match(latestMsg, /3 services succeeded, 2 failed/)

  // All failed
  notify.batch({ operation: 'services', total: 4, succeeded: 0, failed: 4 })
  assert.equal(latestType, 'error')
  assert.match(latestMsg, /All 4 services failed/)

  unsub()
})

test('notification history records items and marks read/clear', () => {
  clearNotificationHistory()
  assert.equal(getUnreadNotificationCount(), 0)

  notify.warning('Warning event')
  assert.equal(getUnreadNotificationCount(), 1)

  markAllNotificationsRead()
  assert.equal(getUnreadNotificationCount(), 0)

  clearNotificationHistory()
  let historyLen = -1
  const unsub = subscribeNotificationHistory((items) => {
    historyLen = items.length
  })
  assert.equal(historyLen, 0)
  unsub()
})

test('dismissNewestNotification pops the latest notification', () => {
  const id1 = notify.info('First')
  const id2 = notify.info('Second')

  const popped = dismissNewestNotification()
  assert.equal(popped, true)

  dismissNotification(id1)
  dismissNotification(id2)
})

test('dismissAllNotifications clears toasts but keeps progress toasts and history', () => {
  resetNotificationsForTest()
  let visible: NotificationItem[] = []
  const unsub = subscribeNotifications((items) => {
    visible = items
  })

  notify.success('Saved')
  notify.warning('Careful')
  notify.error('Broke')
  const task = notify.progress('Downloading')
  assert.equal(visible.length, 4)

  assert.equal(dismissAllNotifications(), 3)
  assert.deepEqual(visible.map((item) => item.type), ['progress'])

  let historyLen = 0
  const unsubHistory = subscribeNotificationHistory((items) => {
    historyLen = items.length
  })
  assert.ok(historyLen >= 3)

  // Nothing left to dismiss: returns 0 and keeps the progress toast.
  assert.equal(dismissAllNotifications(), 0)
  assert.equal(visible.length, 1)

  task.dismiss()
  unsubHistory()
  unsub()
  resetNotificationsForTest()
})

test('notification de-duplication: same error three times in a row produces one entry with x3', () => {
  clearNotificationHistory()

  let historyItems: { count: number; message: string }[] = []
  const unsub = subscribeNotificationHistory((items) => {
    historyItems = items.map((i) => ({ count: i.count, message: i.message }))
  })

  // Trigger same error three times in a row
  notify.error('Database connection failed', { details: 'Port 3306 in use', logSource: 'mysql' })
  notify.error('Database connection failed', { details: 'Port 3306 in use', logSource: 'mysql' })
  notify.error('Database connection failed', { details: 'Port 3306 in use', logSource: 'mysql' })

  assert.equal(historyItems.length, 1)
  assert.equal(historyItems[0].count, 3)
  assert.equal(historyItems[0].message, 'Database connection failed')

  // Unread badge counts entries (1), not occurrences (3)
  assert.equal(getUnreadNotificationCount(), 1)

  unsub()
})

test('notification de-duplication: unrelated entry in between creates separate entries', () => {
  clearNotificationHistory()

  let historyItems: { count: number; message: string }[] = []
  const unsub = subscribeNotificationHistory((items) => {
    historyItems = items.map((i) => ({ count: i.count, message: i.message }))
  })

  notify.error('Database connection failed', { details: 'Port 3306 in use', logSource: 'mysql' })
  notify.info('Unrelated informational message')
  notify.error('Database connection failed', { details: 'Port 3306 in use', logSource: 'mysql' })

  // Should have 3 separate rows since the two errors were not consecutive
  assert.equal(historyItems.length, 3)
  assert.equal(historyItems[0].message, 'Database connection failed')
  assert.equal(historyItems[0].count, 1)
  assert.equal(historyItems[1].message, 'Unrelated informational message')
  assert.equal(historyItems[2].message, 'Database connection failed')
  assert.equal(historyItems[2].count, 1)

  unsub()
})

test('notification de-duplication: two different errors from the same service create separate entries', () => {
  clearNotificationHistory()

  let historyItems: { count: number; message: string; details?: string | null }[] = []
  const unsub = subscribeNotificationHistory((items) => {
    historyItems = items.map((i) => ({ count: i.count, message: i.message, details: i.details }))
  })

  // Two different errors from mysql service
  notify.error('Failed to start service', { details: 'Access denied for user root', logSource: 'mysql' })
  notify.error('Failed to start service', { details: 'Address already in use :3306', logSource: 'mysql' })

  assert.equal(historyItems.length, 2)
  assert.equal(historyItems[0].details, 'Address already in use :3306')
  assert.equal(historyItems[1].details, 'Access denied for user root')

  unsub()
})

test('bell unread count: routine success does not increment, errors/warnings do', () => {
  clearNotificationHistory()
  assert.equal(getUnreadNotificationCount(), 0)

  // Routine success should NOT count toward bell unread badge
  notify.success('All services operational')
  assert.equal(getUnreadNotificationCount(), 0)

  // Routine info should NOT count toward bell unread badge
  notify.info('Configuration reloaded')
  assert.equal(getUnreadNotificationCount(), 0)

  // Warning DOES count
  notify.warning('Port conflict detected')
  assert.equal(getUnreadNotificationCount(), 1)

  // Error DOES count
  notify.error('Nginx failed to start')
  assert.equal(getUnreadNotificationCount(), 2)

  markAllNotificationsRead()
  assert.equal(getUnreadNotificationCount(), 0)
})

test('removeNotificationHistoryItem removes a single entry', () => {
  clearNotificationHistory()

  const id1 = notify.error('Error 1')
  const id2 = notify.error('Error 2')

  let currentHistory: string[] = []
  const unsub = subscribeNotificationHistory((items) => {
    currentHistory = items.map((i) => i.id)
  })

  assert.equal(currentHistory.length, 2)
  assert.ok(currentHistory.includes(id1))
  assert.ok(currentHistory.includes(id2))

  removeNotificationHistoryItem(id1)
  assert.equal(currentHistory.length, 1)
  assert.ok(!currentHistory.includes(id1))
  assert.ok(currentHistory.includes(id2))

  unsub()
})

test('condition-based notifications replace in-place on same key and auto-resolve when condition clears', async () => {
  resetNotificationsForTest()

  let toasts: NotificationItem[] = []
  let history: NotificationItem[] = []
  const unsubToasts = subscribeNotifications((items) => {
    toasts = items
  })
  const unsubHistory = subscribeNotificationHistory((items) => {
    history = items
  })

  // 1. Fire start-failed:apache
  notify.error('Apache failed to start.', {
    id: 'start-failed:apache',
    conditionKey: 'start-failed:apache',
    conditionKeys: ['start-failed:apache', 'port-conflict:apache'],
    title: 'Service failed to start',
    details: 'Port 80 is already in use by another program',
    logSource: 'web:apache:error',
  })

  assert.equal(toasts.length, 1)
  assert.equal(history.length, 1)
  assert.equal(getUnreadNotificationCount(), 1)
  assert.equal(history[0].resolved, false)

  // 2. Same condition fires again -> replaces existing notification instead of stacking (same key = one notification)
  notify.error('Apache failed to start.', {
    id: 'start-failed:apache',
    conditionKey: 'start-failed:apache',
    conditionKeys: ['start-failed:apache', 'port-conflict:apache'],
    title: 'Service failed to start',
    details: 'Port 8080 is already in use by another program',
    logSource: 'web:apache:error',
  })

  assert.equal(toasts.length, 1)
  assert.equal(history.length, 1)
  assert.equal(history[0].details, 'Port 8080 is already in use by another program')
  assert.equal(getUnreadNotificationCount(), 1)

  // 3. One-off event (e.g. "Backup finished") does not auto-resolve
  notify.success('Backup finished')
  assert.equal(history.length, 2)

  // 4. When Apache starts and becomes ready, resolveNotificationsForService('apache'):
  //    - marks toast closing (animating out) and then removes it
  //    - updates history entry in-place to resolved=true with resolvedAt timestamp (keeps it in history)
  //    - decrements unread count to 0
  const didResolve = resolveNotificationsForService('apache')
  assert.equal(didResolve, true)
  assert.equal(getUnreadNotificationCount(), 0)

  const apacheHistory = history.find((h) => h.conditionKey === 'start-failed:apache')
  assert.ok(apacheHistory, 'history entry must be kept, not deleted')
  assert.equal(apacheHistory.resolved, true)
  assert.equal(typeof apacheHistory.resolvedAt, 'number')
  assert.equal(apacheHistory.read, true)

  const backupHistory = history.find((h) => h.message === 'Backup finished')
  assert.ok(backupHistory)
  assert.ok(!backupHistory.resolved, 'one-off events do not auto-resolve')

  // Toast animates out (closing = true immediately, then removed after 200ms)
  await new Promise((r) => setTimeout(r, 250))
  assert.equal(
    toasts.filter((t) => t.conditionKey === 'start-failed:apache').length,
    0,
    'toast must be dismissed after animate-out',
  )

  unsubToasts()
  unsubHistory()
})

test('evaluateNotificationConditions resolves notifications when resolveWhen predicate becomes true', () => {
  resetNotificationsForTest()

  let isApacheReady = false
  let history: NotificationItem[] = []
  const unsubHistory = subscribeNotificationHistory((items) => {
    history = items
  })

  notify.error('Exit code 1: bind() to 0.0.0.0:80 failed', {
    id: 'crashed:apache',
    conditionKey: 'crashed:apache',
    resolveWhen: () => isApacheReady,
    title: 'Apache stopped unexpectedly',
  })

  assert.equal(getUnreadNotificationCount(), 1)
  evaluateNotificationConditions()
  assert.equal(getUnreadNotificationCount(), 1)

  // Now Apache is running and ready
  isApacheReady = true
  evaluateNotificationConditions()

  assert.equal(getUnreadNotificationCount(), 0)
  assert.equal(history.length, 1)
  assert.equal(history[0].resolved, true)
  assert.equal(typeof history[0].resolvedAt, 'number')

  // Direct resolveNotificationByKey on diagnostic issue also resolves it
  notify.recordHistory('warning', 'Diagnostics: 1 issue requires attention.', {
    id: 'diagnostic:port_conflict_80',
    conditionKey: 'diagnostic:port_conflict_80',
    needsAttention: true,
    read: false,
  })
  assert.equal(getUnreadNotificationCount(), 1)
  resolveNotificationByKey('diagnostic:port_conflict_80')
  assert.equal(getUnreadNotificationCount(), 0)
  assert.equal(history.find((h) => h.conditionKey === 'diagnostic:port_conflict_80')?.resolved, true)

  unsubHistory()
})

test('resolving every service in a stop-all crash notification dismisses it without unread errors', async () => {
  resetNotificationsForTest()
  let visibleToasts: NotificationItem[] = []
  let history: NotificationItem[] = []
  const unsubscribeToasts = subscribeNotifications((items) => {
    visibleToasts = items
  })
  const unsubscribeHistory = subscribeNotificationHistory((items) => {
    history = items
  })
  const serviceIds = ['mariadb', 'nginx', 'apache', 'caddy']
  const conditionKeys = serviceIds.map((id) => `crashed:${id}`)
  const groupKey = `crashed:${serviceIds.join(',')}`
  const toastId = notify.error('MariaDB, Nginx, Apache, Caddy stopped unexpectedly.', {
    id: groupKey,
    conditionKey: groupKey,
    conditionKeys,
    title: '4 services stopped unexpectedly',
  })

  for (const id of serviceIds) {
    resolveNotificationsForService(id)
  }

  assert.equal(getUnreadNotificationCount(), 0)
  assert.equal(history[0]?.resolved, true)
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.ok(!visibleToasts.some((item) => item.id === toastId), 'grouped error toast should be dismissed')
  unsubscribeToasts()
  unsubscribeHistory()
})
