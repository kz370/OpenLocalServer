import test from 'node:test'
import assert from 'node:assert/strict'
import { getServiceGroupId, groupServices } from './serviceGrouping.ts'

interface MockService {
  id: string
  name: string
  category?: string | null
  kind?: string | null
  running?: boolean
  installed?: boolean
}

test('getServiceGroupId maps known categories correctly', () => {
  // Web servers
  assert.equal(getServiceGroupId({ category: 'web' }), 'web')
  assert.equal(getServiceGroupId({ category: 'web_server' }), 'web')
  assert.equal(getServiceGroupId({ category: 'Web servers' }), 'web')

  // Databases
  assert.equal(getServiceGroupId({ category: 'database' }), 'database')
  assert.equal(getServiceGroupId({ category: 'databases' }), 'database')
  assert.equal(getServiceGroupId({ category: 'sql' }), 'database')
  assert.equal(getServiceGroupId({ category: 'document' }), 'database')
  assert.equal(getServiceGroupId({ category: 'Databases' }), 'database')

  // Cache & queues
  assert.equal(getServiceGroupId({ category: 'cache' }), 'cache')
  assert.equal(getServiceGroupId({ category: 'queue' }), 'cache')
  assert.equal(getServiceGroupId({ category: 'cache_queue' }), 'cache')
  assert.equal(getServiceGroupId({ category: 'Cache & queues' }), 'cache')

  // Mail
  assert.equal(getServiceGroupId({ category: 'mail' }), 'mail')
  assert.equal(getServiceGroupId({ category: 'email' }), 'mail')
  assert.equal(getServiceGroupId({ category: 'smtp' }), 'mail')
  assert.equal(getServiceGroupId({ category: 'Mail' }), 'mail')
})

test('getServiceGroupId falls back to kind if category is not provided', () => {
  assert.equal(getServiceGroupId({ kind: 'web' }), 'web')
  assert.equal(getServiceGroupId({ kind: 'sql' }), 'database')
  assert.equal(getServiceGroupId({ kind: 'document' }), 'database')
  assert.equal(getServiceGroupId({ kind: 'cache' }), 'cache')
  assert.equal(getServiceGroupId({ kind: 'mail' }), 'mail')
})

test('unknown type falls to Other group', () => {
  assert.equal(getServiceGroupId({ category: 'unknown_type' }), 'other')
  assert.equal(getServiceGroupId({ category: 'custom' }), 'other')
  assert.equal(getServiceGroupId({ category: 'tools' }), 'other')
  assert.equal(getServiceGroupId({ category: '' }), 'other')
  assert.equal(getServiceGroupId({ category: null }), 'other')
  assert.equal(getServiceGroupId({ category: undefined }), 'other')
  assert.equal(getServiceGroupId({}), 'other')
})

test('groupServices hides empty groups', () => {
  const services: MockService[] = [
    { id: 'nginx', name: 'Nginx', category: 'web' },
    { id: 'mariadb', name: 'MariaDB', category: 'database' },
  ]

  const groups = groupServices(services)
  assert.equal(groups.length, 2)
  assert.deepEqual(groups.map((g) => g.id), ['web', 'database'])
  assert.deepEqual(groups.map((g) => g.label), ['Web servers', 'Databases'])
})

test('groupServices returns empty array for null, undefined or empty input', () => {
  assert.deepEqual(groupServices(null), [])
  assert.deepEqual(groupServices(undefined), [])
  assert.deepEqual(groupServices([]), [])
})

test('groupServices enforces strict group order (Web servers -> Databases -> Cache & queues -> Mail -> Other)', () => {
  // Input in shuffled order
  const shuffled: MockService[] = [
    { id: 'my-custom', name: 'Plugin Service', category: 'unknown' },
    { id: 'mailpit', name: 'Mailpit', category: 'mail' },
    { id: 'redis', name: 'Redis', category: 'cache' },
    { id: 'mariadb', name: 'MariaDB', category: 'database' },
    { id: 'caddy', name: 'Caddy', category: 'web' },
  ]

  const groups = groupServices(shuffled)
  assert.equal(groups.length, 5)
  assert.equal(groups[0].id, 'web')
  assert.equal(groups[0].label, 'Web servers')
  assert.equal(groups[1].id, 'database')
  assert.equal(groups[1].label, 'Databases')
  assert.equal(groups[2].id, 'cache')
  assert.equal(groups[2].label, 'Cache & queues')
  assert.equal(groups[3].id, 'mail')
  assert.equal(groups[3].label, 'Mail')
  assert.equal(groups[4].id, 'other')
  assert.equal(groups[4].label, 'Other')
})

test('services inside a group preserve their original input order', () => {
  const services: MockService[] = [
    { id: 'apache', name: 'Apache', category: 'web' },
    { id: 'postgres', name: 'PostgreSQL', category: 'database' },
    { id: 'nginx', name: 'Nginx', category: 'web' },
    { id: 'mongodb', name: 'MongoDB', category: 'database' },
    { id: 'caddy', name: 'Caddy', category: 'web' },
    { id: 'mariadb', name: 'MariaDB', category: 'database' },
  ]

  const groups = groupServices(services)
  const webGroup = groups.find((g) => g.id === 'web')
  assert.ok(webGroup)
  assert.deepEqual(webGroup!.services.map((s) => s.id), ['apache', 'nginx', 'caddy'])

  const dbGroup = groups.find((g) => g.id === 'database')
  assert.ok(dbGroup)
  assert.deepEqual(dbGroup!.services.map((s) => s.id), ['postgres', 'mongodb', 'mariadb'])
})

test('plugin-provided services land in right group or Other if they declare nothing', () => {
  const services: MockService[] = [
    { id: 'plugin-db', name: 'CockroachDB', category: 'database' },
    { id: 'plugin-cache', name: 'KeyDB', category: 'cache' },
    { id: 'plugin-worker', name: 'Custom Worker' }, // declares nothing
    { id: 'plugin-other', name: 'Meilisearch', category: 'search' }, // unknown category
  ]

  const groups = groupServices(services)
  assert.deepEqual(groups.map((g) => g.id), ['database', 'cache', 'other'])

  const otherGroup = groups.find((g) => g.id === 'other')
  assert.ok(otherGroup)
  assert.deepEqual(otherGroup!.services.map((s) => s.id), ['plugin-worker', 'plugin-other'])
})
