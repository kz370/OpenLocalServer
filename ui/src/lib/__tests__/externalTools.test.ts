import assert from 'node:assert/strict'
import test from 'node:test'
import {
  CURRENT_SCHEMA_VERSION,
  exportToolsJson,
  getValidPlaceholderChips,
  slugify,
  validateImportJson,
} from '../externalTools.ts'
import type { ExternalTool } from '../../core/types/web.ts'

test('validateImportJson accepts valid schema v1', () => {
  const json = JSON.stringify({
    schemaVersion: 1,
    tools: [
      {
        id: 'dbeaver',
        name: 'DBeaver',
        engines: ['mariadb', 'postgres', 'sqlite'],
        launchMode: 'args',
        args: ['-con', '{uri}'],
      },
    ],
  })

  const res = validateImportJson(json)
  assert.equal(res.ok, true)
  if (res.ok) {
    assert.equal(res.schemaVersion, 1)
    assert.equal(res.tools.length, 1)
    assert.equal(res.tools[0].id, 'dbeaver')
  }
})

test('validateImportJson strips any executable property found in file', () => {
  const json = JSON.stringify({
    schemaVersion: 1,
    tools: [
      {
        id: 'testtool',
        name: 'Test Tool',
        engines: ['mariadb'],
        launchMode: 'args',
        args: ['{uri}'],
        executable: 'C:\\secret\\path.exe',
      },
    ],
  })

  const res = validateImportJson(json)
  assert.equal(res.ok, true)
  if (res.ok) {
    assert.equal('executable' in res.tools[0], false)
  }
})

test('validateImportJson rejects missing or non-integer schemaVersion', () => {
  const noVersion = JSON.stringify({ tools: [] })
  const res1 = validateImportJson(noVersion)
  assert.equal(res1.ok, false)

  const floatVersion = JSON.stringify({ schemaVersion: 1.5, tools: [] })
  const res2 = validateImportJson(floatVersion)
  assert.equal(res2.ok, false)
})

test('validateImportJson rejects future schemaVersion', () => {
  const future = JSON.stringify({ schemaVersion: 999, tools: [] })
  const res = validateImportJson(future)
  assert.equal(res.ok, false)
  if (!res.ok) {
    assert.match(res.cause, /supports version 1/)
  }
})

test('validateImportJson rejects unknown database engine', () => {
  const json = JSON.stringify({
    schemaVersion: 1,
    tools: [
      {
        id: 'custom',
        name: 'Custom Tool',
        engines: ['mariadb', 'oracle_unknown'],
        launchMode: 'args',
        args: [],
      },
    ],
  })

  const res = validateImportJson(json)
  assert.equal(res.ok, false)
  if (!res.ok) {
    assert.match(res.problem, /Unknown database engine/i)
  }
})

test('validateImportJson rejects unknown argument placeholder', () => {
  const json = JSON.stringify({
    schemaVersion: 1,
    tools: [
      {
        id: 'custom',
        name: 'Custom Tool',
        engines: ['mariadb'],
        launchMode: 'args',
        args: ['--password={super_secret_unallowed}'],
      },
    ],
  })

  const res = validateImportJson(json)
  assert.equal(res.ok, false)
  if (!res.ok) {
    assert.match(res.problem, /Unknown argument placeholder/i)
  }
})

test('exportToolsJson produces clean schemaVersion: 1 without local paths', () => {
  const registered: ExternalTool[] = [
    {
      id: 'dbeaver',
      name: 'DBeaver',
      engines: ['mariadb', 'postgres'],
      launchMode: 'args',
      args: ['-con', '{uri}'],
      executable: 'C:\\Users\\user\\dbeaver.exe',
      available: true,
    },
  ]

  const output = exportToolsJson(registered)
  const parsed = JSON.parse(output)

  assert.equal(parsed.schemaVersion, CURRENT_SCHEMA_VERSION)
  assert.equal(parsed.tools.length, 1)
  assert.equal(parsed.tools[0].id, 'dbeaver')
  assert.equal('executable' in parsed.tools[0], false)
  assert.equal('available' in parsed.tools[0], false)
})

test('getValidPlaceholderChips filters chips by engine', () => {
  // SQLite only
  const sqliteChips = getValidPlaceholderChips(['sqlite']).map((c) => c.token)
  assert.ok(sqliteChips.includes('{uri}'))
  assert.ok(sqliteChips.includes('{path}'))
  assert.ok(!sqliteChips.includes('{host}'))

  // Network engine only
  const networkChips = getValidPlaceholderChips(['mariadb', 'postgres']).map((c) => c.token)
  assert.ok(networkChips.includes('{uri}'))
  assert.ok(networkChips.includes('{host}'))
  assert.ok(networkChips.includes('{port}'))
  assert.ok(!networkChips.includes('{path}'))

  // All / Empty
  const allChips = getValidPlaceholderChips([]).map((c) => c.token)
  assert.ok(allChips.includes('{uri}'))
  assert.ok(allChips.includes('{path}'))
  assert.ok(allChips.includes('{host}'))
})

test('slugify cleans names into lowercase IDs', () => {
  assert.equal(slugify('DBeaver Community'), 'dbeaver-community')
  assert.equal(slugify('DB Browser for SQLite!'), 'db-browser-for-sqlite')
  assert.equal(slugify('  My_Custom_Tool 2026 '), 'my-custom-tool-2026')
})
