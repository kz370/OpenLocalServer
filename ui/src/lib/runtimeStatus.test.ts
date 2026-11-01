import assert from 'node:assert/strict'
import test from 'node:test'
import {
  compareVersionsDesc,
  formatAvailableVersions,
  getRuntimeStatus,
  parseVersionKey,
  type RuntimeVersionRow,
} from './runtimeStatus.ts'

test('parseVersionKey handles semantic, prefixed, and complex versions', () => {
  assert.deepEqual(parseVersionKey('2.4.69'), [2, 4, 69])
  assert.deepEqual(parseVersionKey('v1.33.0'), [1, 33, 0])
  assert.deepEqual(parseVersionKey('2.55.0.windows.5'), [2, 55, 0, 0, 5])
  assert.deepEqual(parseVersionKey('custom'), [0])
})

test('compareVersionsDesc correctly orders version descending', () => {
  assert.ok(compareVersionsDesc('2.4.69', '2.4.68') < 0)
  assert.ok(compareVersionsDesc('2.4.68', '2.4.69') > 0)
  assert.equal(compareVersionsDesc('2.4.69', '2.4.69'), 0)
  assert.ok(compareVersionsDesc('22.23.3', '20.18.0') < 0)
  assert.ok(compareVersionsDesc('8.4.26', '8.3.35') < 0)
})

test('formatAvailableVersions formats singular and plural forms', () => {
  assert.equal(formatAvailableVersions(0), '')
  assert.equal(formatAvailableVersions(1), '1 version available')
  assert.equal(formatAvailableVersions(208), '208 versions available')
})

test('State (1): Up to date when installed version is the latest available', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'node@22.23.3', version: '22.23.3', kind: 'managed', installed: true, isDefault: true },
    { key: 'node@22.23.0', version: '22.23.0', kind: 'managed', installed: false },
    { key: 'node@20.18.0', version: '20.18.0', kind: 'managed', installed: false },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.statusKind, 'up_to_date')
  assert.equal(status.statusLabel, 'Up to date')
  assert.equal(status.hasUpdate, false)
  assert.equal(status.latestAvailableVersion, '22.23.3')
  assert.equal(status.highestInstalledVersion, '22.23.3')
  assert.equal(status.defaultVersion, '22.23.3')
  assert.equal(status.totalInstalled, 1)
  assert.equal(status.installableCount, 2)
})

test('State (2): Update available when a newer version exists in catalog', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'apache@2.4.70', version: '2.4.70', kind: 'managed', installed: false },
    { key: 'apache@2.4.69', version: '2.4.69', kind: 'managed', installed: true, isDefault: true },
    { key: 'apache@2.4.68', version: '2.4.68', kind: 'managed', installed: false },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.statusKind, 'update_available')
  assert.equal(status.statusLabel, 'Update available')
  assert.equal(status.hasUpdate, true)
  assert.equal(status.latestAvailableVersion, '2.4.70')
  assert.equal(status.highestInstalledVersion, '2.4.69')
  assert.equal(status.totalInstalled, 1)
  assert.equal(status.installableCount, 2)
})

test('State (3): Multiple versions installed does not treat older available as updates', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'php@8.4.26', version: '8.4.26', kind: 'managed', installed: true, isDefault: true },
    { key: 'php@8.3.35', version: '8.3.35', kind: 'managed', installed: true, isDefault: false },
    { key: 'php@8.2.27', version: '8.2.27', kind: 'managed', installed: false },
    { key: 'php@8.1.34', version: '8.1.34', kind: 'managed', installed: false },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.statusKind, 'up_to_date')
  assert.equal(status.hasUpdate, false)
  assert.equal(status.totalInstalled, 2)
  assert.equal(status.defaultVersion, '8.4.26')
  assert.equal(status.highestInstalledVersion, '8.4.26')
  assert.equal(status.installableCount, 2)
})

test('Multiple versions installed where an even newer version is available', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'php@8.5.0', version: '8.5.0', kind: 'managed', installed: false },
    { key: 'php@8.4.26', version: '8.4.26', kind: 'managed', installed: true, isDefault: true },
    { key: 'php@8.3.35', version: '8.3.35', kind: 'managed', installed: true, isDefault: false },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.statusKind, 'update_available')
  assert.equal(status.hasUpdate, true)
  assert.equal(status.latestAvailableVersion, '8.5.0')
  assert.equal(status.highestInstalledVersion, '8.4.26')
  assert.equal(status.totalInstalled, 2)
})

test('State (4): Default version identified properly, other marked installed', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'mailpit@1.33.0', version: '1.33.0', kind: 'managed', installed: true, isDefault: false },
    { key: 'mailpit@1.31.3', version: '1.31.3', kind: 'managed', installed: true, isDefault: true },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.defaultVersion, '1.31.3')
  assert.equal(status.highestInstalledVersion, '1.33.0')
  assert.equal(status.totalInstalled, 2)
  assert.equal(status.hasUpdate, false)
  assert.equal(status.statusKind, 'up_to_date')
})

test('Edge cases: No installed versions, checking, error, and empty catalog', () => {
  // No installed versions
  const notInstalled = getRuntimeStatus({
    rows: [
      { key: 'k6@2.3.0', version: '2.3.0', kind: 'managed', installed: false },
    ],
  })
  assert.equal(notInstalled.statusKind, 'not_installed')
  assert.equal(notInstalled.statusLabel, 'Not installed')
  assert.equal(notInstalled.totalInstalled, 0)
  assert.equal(notInstalled.installableCount, 1)

  // Empty catalog checking
  const checking = getRuntimeStatus({ rows: [], isChecking: true })
  assert.equal(checking.statusKind, 'checking')
  assert.equal(checking.statusLabel, 'Checking…')

  // Empty catalog error
  const error = getRuntimeStatus({ rows: [], isError: true })
  assert.equal(error.statusKind, 'error')
  assert.equal(error.statusLabel, 'Could not check')

  // Completely empty catalog (neither error nor checking)
  const empty = getRuntimeStatus({ rows: [] })
  assert.equal(empty.statusKind, 'not_installed')
  assert.equal(empty.totalInstalled, 0)
  assert.equal(empty.installableCount, 0)
  assert.equal(empty.latestAvailableVersion, null)
})

test('Handles custom installs alongside managed installs', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'php@8.4.26', version: '8.4.26', kind: 'managed', installed: false },
    { key: 'custom:php:system', version: '8.4.26', kind: 'custom', installed: true },
  ]

  const status = getRuntimeStatus({ rows })
  assert.equal(status.totalInstalled, 1)
  assert.equal(status.customInstalledCount, 1)
  assert.equal(status.managedInstalledCount, 0)
  assert.equal(status.highestInstalledVersion, '8.4.26')
  assert.equal(status.latestAvailableVersion, '8.4.26')
  assert.equal(status.statusKind, 'up_to_date')
})
