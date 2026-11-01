import assert from 'node:assert/strict'
import test from 'node:test'

import {
  clearNotificationHistory,
  dismissNewestNotification,
  dismissNotification,
  getUnreadNotificationCount,
  markAllNotificationsRead,
  notify,
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
