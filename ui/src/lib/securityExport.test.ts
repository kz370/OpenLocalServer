import test from 'node:test'
import assert from 'node:assert/strict'
import { formatFinding, formatReport } from './securityExport.ts'

const item = {
  project: 'Shop',
  tool: 'composer',
  f: { package: 'symfony/routing', severity: 'high', title: 'CVE-1: bad', advisory: 'CVE-1', url: 'https://x/y', affected: '>=4,<5', fix: '5.1' },
}

test('formatFinding lists every field and skips empty ones', () => {
  assert.equal(
    formatFinding(item),
    '[HIGH] symfony/routing (composer) in Shop\nCVE-1: bad\nAdvisory: CVE-1\nAffected: >=4,<5\nFix: 5.1\nLink: https://x/y',
  )
  const bare = formatFinding({ project: 'P', tool: 'npm', f: { package: 'a', severity: 'low', title: 'T' } })
  assert.equal(bare, '[LOW] a (npm) in P\nT\nFix: no fixed version listed')
})

test('formatReport groups by project and starts with ended versions', () => {
  const r = formatReport([item, { ...item, project: 'Blog' }], [{ runtime: 'php', version: '8.1.34', usedBy: ['shop'] }], new Date('2026-11-09T00:00:00Z'))
  assert.match(r, /^# OLS security report\n\nCreated 2026-11-09T00:00:00.000Z/)
  assert.ok(r.indexOf('php 8.1.34 (used by shop)') < r.indexOf('## Shop'))
  assert.ok(r.includes('## Shop (1 finding)'))
  assert.ok(r.includes('## Blog (1 finding)'))
  assert.ok(r.includes('- Fix: 5.1'))
})

test('formatReport says so when empty', () => {
  assert.ok(formatReport([], [], new Date(0)).includes('Nothing to report.'))
})
