import assert from 'node:assert'
import { test } from 'node:test'

import {
  filterReleaseNotes,
  getFeatureIcon,
  getSectionBadge,
  parseReleaseNotes,
} from '../releaseNotes.ts'

const SAMPLE_RELEASE_NOTE = `# OLS (Open Local Server) v2.5.0: Spring Update

**Release date:** 2026-10-31 · **Platform:** Windows 10/11 (64-bit) · **License:** GPL-3.0-only

Feature release introducing consolidated missing-folder dashboard alerts, intelligent drive detection, and UI themes.

## Highlights
- **Folder Diagnostics**: Replaced repetitive dashboard diagnostic rows with a single collapsible summary card.
- **Disconnected Drive Detection**: Detects when an external drive (e.g. D:\\) or USB is offline.
- **Appearance Customization**: Dedicated Settings Appearance tab with presets and accent color picker.
- Simple improvement item without a bold prefix for general fixes.

## Fixed
- **PHP 8.5 FastCGI Crash**: Configured opcache.file_cache to prevent ASLR crash-loops on Windows.

## Upgrading
Run OLS-2.5.0-setup.exe to upgrade. Existing configuration is preserved.
`

test('parseReleaseNotes parses metadata correctly', () => {
  const parsed = parseReleaseNotes(SAMPLE_RELEASE_NOTE)

  assert.strictEqual(parsed.metadata.title, 'Open Local Server')
  assert.strictEqual(parsed.metadata.version, 'v2.5.0')
  assert.strictEqual(parsed.metadata.subtitle, 'Spring Update')
  assert.strictEqual(parsed.metadata.date, '2026-10-31')
  assert.strictEqual(parsed.metadata.platform, 'Windows 10/11 (64-bit)')
  assert.strictEqual(parsed.metadata.license, 'GPL-3.0-only')
  assert.ok(parsed.metadata.summary?.includes('Feature release introducing consolidated'))
})

test('parseReleaseNotes parses sections and features', () => {
  const parsed = parseReleaseNotes(SAMPLE_RELEASE_NOTE)

  assert.strictEqual(parsed.sections.length, 3)

  const highlights = parsed.sections[0]
  assert.strictEqual(highlights.title, 'Highlights')
  assert.strictEqual(highlights.badgeType, 'highlights')
  assert.strictEqual(highlights.features.length, 4)

  assert.strictEqual(highlights.features[0].title, 'Folder Diagnostics')
  assert.ok(highlights.features[0].description.includes('Replaced repetitive dashboard'))

  assert.strictEqual(highlights.features[1].title, 'Disconnected Drive Detection')
  assert.strictEqual(highlights.features[2].title, 'Appearance Customization')

  // Simple item without bold prefix
  assert.strictEqual(highlights.features[3].title, '')
  assert.ok(highlights.features[3].description.includes('Simple improvement item'))

  const fixed = parsed.sections[1]
  assert.strictEqual(fixed.title, 'Fixed')
  assert.strictEqual(fixed.badgeType, 'fixed')
  assert.strictEqual(fixed.features.length, 1)
  assert.strictEqual(fixed.features[0].title, 'PHP 8.5 FastCGI Crash')

  const upgrading = parsed.sections[2]
  assert.strictEqual(upgrading.title, 'Upgrading')
  assert.strictEqual(upgrading.content.length, 1)
})

test('getFeatureIcon maps topics to meaningful icons', () => {
  const folderIcon = getFeatureIcon('Missing Folder Alerts')
  assert.ok(folderIcon)

  const driveIcon = getFeatureIcon('Disconnected Drive')
  assert.ok(driveIcon)

  const themeIcon = getFeatureIcon('Appearance and Themes')
  assert.ok(themeIcon)

  const phpIcon = getFeatureIcon('PHP 8.5 OPcache Fix')
  assert.ok(phpIcon)

  const bugIcon = getFeatureIcon('Crash on start', 'Fixed')
  assert.ok(bugIcon)
})

test('getSectionBadge returns matching badge config', () => {
  const highlights = getSectionBadge('Highlights')
  assert.strictEqual(highlights.label, 'Highlights')

  const added = getSectionBadge('New Features')
  assert.strictEqual(added.label, 'New Features')

  const fixes = getSectionBadge('Bug Fixes')
  assert.strictEqual(fixes.label, 'Bug Fixes')

  const security = getSectionBadge('Security')
  assert.strictEqual(security.label, 'Security')
})

test('filterReleaseNotes filters by search query and counts matches', () => {
  const parsed = parseReleaseNotes(SAMPLE_RELEASE_NOTE)

  // Empty query returns all
  const all = filterReleaseNotes(parsed, '')
  assert.strictEqual(all.filteredSections.length, 3)
  assert.strictEqual(all.matchCount, 5)

  // Matching specific query
  const phpMatch = filterReleaseNotes(parsed, 'opcache')
  assert.strictEqual(phpMatch.matchCount, 1)
  assert.strictEqual(phpMatch.filteredSections.length, 1)
  assert.strictEqual(phpMatch.filteredSections[0].title, 'Fixed')

  // Non-matching query
  const noMatch = filterReleaseNotes(parsed, 'nonexistentxyz')
  assert.strictEqual(noMatch.matchCount, 0)
  assert.strictEqual(noMatch.filteredSections.length, 0)
})
