import assert from 'node:assert'
import { beforeEach, test } from 'node:test'

import { createPersistentCache, getCached, removeCached, setCached } from '../cache.ts'

// Mock minimal localStorage for Node environment
class MockLocalStorage {
  private store: Record<string, string> = {}
  getItem(key: string): string | null {
    return this.store[key] ?? null
  }
  setItem(key: string, value: string): void {
    this.store[key] = String(value)
  }
  removeItem(key: string): void {
    delete this.store[key]
  }
  clear(): void {
    this.store = {}
  }
}

const mockStorage = new MockLocalStorage()
// @ts-expect-error mock window for node
globalThis.window = { localStorage: mockStorage }

beforeEach(() => {
  mockStorage.clear()
})

test('getCached returns defaultValue when key not present', () => {
  const result = getCached('test_empty', { count: 0 })
  assert.deepStrictEqual(result, { count: 0 })
})

test('setCached and getCached persist data properly', () => {
  setCached('test_data', { items: ['apple', 'banana'] })
  const result = getCached('test_data', { items: [] })
  assert.deepStrictEqual(result, { items: ['apple', 'banana'] })
})

test('removeCached deletes item', () => {
  setCached('test_del', 'hello')
  removeCached('test_del')
  const result = getCached('test_del', 'default')
  assert.strictEqual(result, 'default')
})

test('createPersistentCache gets initial and updates memory + storage', () => {
  const cache = createPersistentCache('pref_counter', 10)
  assert.strictEqual(cache.get(), 10)

  cache.set(20)
  assert.strictEqual(cache.get(), 20)

  // Verify in localStorage
  assert.strictEqual(mockStorage.getItem('ols_cache_pref_counter'), '20')

  // Verify function update
  cache.set((prev) => prev + 5)
  assert.strictEqual(cache.get(), 25)
  assert.strictEqual(mockStorage.getItem('ols_cache_pref_counter'), '25')

  // Create another instance reading the same key (simulating app restart)
  const restartedCache = createPersistentCache('pref_counter', 0)
  assert.strictEqual(restartedCache.get(), 25)
})

