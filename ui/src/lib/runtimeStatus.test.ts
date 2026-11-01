import assert from 'node:assert/strict'
import test from 'node:test'
import {
  compareVersionsDesc,
  formatAvailableVersions,
  getRuntimeStatus,
  isMajorUpgrade,
  isNodeLts,
  isPrerelease,
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

test('isPrerelease identifies alpha, beta, rc, preview, and dev versions', () => {
  assert.equal(isPrerelease('8.4.0alpha1'), true)
  assert.equal(isPrerelease('8.4.0-alpha.2'), true)
  assert.equal(isPrerelease('8.4.0beta1'), true)
  assert.equal(isPrerelease('8.4.0-beta.3'), true)
  assert.equal(isPrerelease('8.4.0RC1'), true)
  assert.equal(isPrerelease('8.4.0-rc.2'), true)
  assert.equal(isPrerelease('1.0.0-dev'), true)
  assert.equal(isPrerelease('2.0.0-preview.1'), true)
  assert.equal(isPrerelease('3.0.0-canary'), true)
  assert.equal(isPrerelease('8.4.0'), false)
  assert.equal(isPrerelease('2.4.69'), false)
  assert.equal(isPrerelease('22.11.0'), false)
  assert.equal(isPrerelease('1.33.0'), false)
})

test('isNodeLts correctly identifies even-numbered Node.js major versions', () => {
  assert.equal(isNodeLts('22.11.0'), true)
  assert.equal(isNodeLts('20.18.0'), true)
  assert.equal(isNodeLts('18.20.5'), true)
  assert.equal(isNodeLts('23.1.0'), false) // Odd major: Current / non-LTS
  assert.equal(isNodeLts('21.7.3'), false) // Odd major: Current / non-LTS
  assert.equal(isNodeLts('22.0.0-rc.1'), false) // Pre-release cannot be LTS
})

test('isMajorUpgrade identifies major semver boundary changes', () => {
  assert.equal(isMajorUpgrade('8.0.4', '9.0.0'), true) // MongoDB 8 -> 9
  assert.equal(isMajorUpgrade('8.0.4', '8.1.0'), false) // Minor upgrade
  assert.equal(isMajorUpgrade('8.0.4', '8.0.5'), false) // Patch upgrade
  assert.equal(isMajorUpgrade('20.18.0', '22.11.0'), true) // Node 20 -> 22
  assert.equal(isMajorUpgrade('22.10.0', '22.11.0'), false)
  assert.equal(isMajorUpgrade(null, '9.0.0'), false)
})

test('formatAvailableVersions formats singular and plural forms', () => {
  assert.equal(formatAvailableVersions(0), '')
  assert.equal(formatAvailableVersions(1), '1 version available')
  assert.equal(formatAvailableVersions(208), '208 versions available')
})

test('Requirement 1: Never offers pre-release as default update target', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'mongodb@9.1.0-rc1', version: '9.1.0-rc1', kind: 'managed', installed: false },
    { key: 'mongodb@9.0.0', version: '9.0.0', kind: 'managed', installed: false },
    { key: 'mongodb@8.0.4', version: '8.0.4', kind: 'managed', installed: true, isDefault: true },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'mongodb' })
  assert.equal(status.statusKind, 'update_available')
  assert.equal(status.hasUpdate, true)
  // Target MUST point to 9.0.0 stable, NOT 9.1.0-rc1
  assert.equal(status.targetUpdateVersion, '9.0.0')
  assert.equal(status.latestAvailableVersion, '9.1.0-rc1')
  assert.equal(status.isMajorUpgrade, true)
})

test('Requirement 1 (Node.js): Update target points to latest LTS, not odd-numbered Current', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'node@23.1.0', version: '23.1.0', kind: 'managed', installed: false }, // Current
    { key: 'node@22.11.0', version: '22.11.0', kind: 'managed', installed: false }, // LTS
    { key: 'node@20.18.0', version: '20.18.0', kind: 'managed', installed: true, isDefault: true },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'node' })
  assert.equal(status.statusKind, 'update_available')
  assert.equal(status.hasUpdate, true)
  // Points to 22.11.0 (LTS), skipping 23.1.0
  assert.equal(status.targetUpdateVersion, '22.11.0')
  assert.equal(status.isMajorUpgrade, true)
})

test('Requirement 1 (Node.js): User on latest LTS is Up to date even if odd-numbered Current exists', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'node@23.1.0', version: '23.1.0', kind: 'managed', installed: false },
    { key: 'node@22.11.0', version: '22.11.0', kind: 'managed', installed: true, isDefault: true },
    { key: 'node@20.18.0', version: '20.18.0', kind: 'managed', installed: true },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'node' })
  assert.equal(status.statusKind, 'up_to_date')
  assert.equal(status.hasUpdate, false)
  assert.equal(status.targetUpdateVersion, null)
})

test('Requirement 2: Minor and patch updates have isMajorUpgrade=false', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'apache@2.4.70', version: '2.4.70', kind: 'managed', installed: false },
    { key: 'apache@2.4.69', version: '2.4.69', kind: 'managed', installed: true, isDefault: true },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'apache' })
  assert.equal(status.statusKind, 'update_available')
  assert.equal(status.hasUpdate, true)
  assert.equal(status.targetUpdateVersion, '2.4.70')
  assert.equal(status.isMajorUpgrade, false)
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
  assert.equal(status.hasDefaultBadge, true)
})

test('State (3): Multiple versions installed does not treat older available as updates', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'php@8.4.26', version: '8.4.26', kind: 'managed', installed: true, isDefault: true },
    { key: 'php@8.3.35', version: '8.3.35', kind: 'managed', installed: true, isDefault: false },
    { key: 'php@8.2.27', version: '8.2.27', kind: 'managed', installed: false },
    { key: 'php@8.1.34', version: '8.1.34', kind: 'managed', installed: false },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'php' })
  assert.equal(status.statusKind, 'up_to_date')
  assert.equal(status.hasUpdate, false)
  assert.equal(status.totalInstalled, 2)
  assert.equal(status.defaultVersion, '8.4.26')
  assert.equal(status.hasDefaultBadge, true)
  assert.equal(status.highestInstalledVersion, '8.4.26')
  assert.equal(status.installableCount, 2)
})

test('State (4): Default version identified properly with multiple versions', () => {
  const rows: RuntimeVersionRow[] = [
    { key: 'mailpit@1.33.0', version: '1.33.0', kind: 'managed', installed: true, isDefault: false },
    { key: 'mailpit@1.31.3', version: '1.31.3', kind: 'managed', installed: true, isDefault: true },
  ]

  const status = getRuntimeStatus({ rows, runtimeId: 'mailpit' })
  assert.equal(status.defaultVersion, '1.31.3')
  assert.equal(status.hasDefaultBadge, true)
  assert.equal(status.highestInstalledVersion, '1.33.0')
  assert.equal(status.totalInstalled, 2)
  assert.equal(status.hasUpdate, false)
  assert.equal(status.statusKind, 'up_to_date')
})

test('Edge cases: No installed versions, checking, error, and empty catalog', () => {
  const notInstalled = getRuntimeStatus({
    rows: [
      { key: 'k6@2.3.0', version: '2.3.0', kind: 'managed', installed: false },
    ],
  })
  assert.equal(notInstalled.statusKind, 'not_installed')
  assert.equal(notInstalled.statusLabel, 'Not installed')
  assert.equal(notInstalled.totalInstalled, 0)
  assert.equal(notInstalled.hasDefaultBadge, false)
  assert.equal(notInstalled.installableCount, 1)

  const checking = getRuntimeStatus({ rows: [], isChecking: true })
  assert.equal(checking.statusKind, 'checking')
  assert.equal(checking.statusLabel, 'Checking…')

  const error = getRuntimeStatus({ rows: [], isError: true })
  assert.equal(error.statusKind, 'error')
  assert.equal(error.statusLabel, 'Could not check')

  const empty = getRuntimeStatus({ rows: [] })
  assert.equal(empty.statusKind, 'not_installed')
  assert.equal(empty.totalInstalled, 0)
  assert.equal(empty.installableCount, 0)
  assert.equal(empty.latestAvailableVersion, null)
})
