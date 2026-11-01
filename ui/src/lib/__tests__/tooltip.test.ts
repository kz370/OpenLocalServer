import assert from 'node:assert/strict'
import test from 'node:test'

import { parseTooltipShortcut } from '../tooltip.ts'

test('parseTooltipShortcut extracts shortcut keys with Ctrl/Cmd/Alt/Shift/symbols', () => {
  const t1 = parseTooltipShortcut('Reload config (Ctrl+R)')
  assert.equal(t1.label, 'Reload config')
  assert.equal(t1.shortcut, 'Ctrl+R')

  const t2 = parseTooltipShortcut('Save changes (⌘+S)')
  assert.equal(t2.label, 'Save changes')
  assert.equal(t2.shortcut, '⌘+S')

  const t3 = parseTooltipShortcut('Format document (Shift+Alt+F)')
  assert.equal(t3.label, 'Format document')
  assert.equal(t3.shortcut, 'Shift+Alt+F')

  const t4 = parseTooltipShortcut('Quick open (Cmd+P)')
  assert.equal(t4.label, 'Quick open')
  assert.equal(t4.shortcut, 'Cmd+P')
})

test('parseTooltipShortcut ignores descriptive parentheses', () => {
  const t1 = parseTooltipShortcut('nginx (running)')
  assert.equal(t1.label, 'nginx (running)')
  assert.equal(t1.shortcut, null)

  const t2 = parseTooltipShortcut('Database (3 instances)')
  assert.equal(t2.label, 'Database (3 instances)')
  assert.equal(t2.shortcut, null)

  const t3 = parseTooltipShortcut('Check again')
  assert.equal(t3.label, 'Check again')
  assert.equal(t3.shortcut, null)
})

test('parseTooltipShortcut handles empty and non-string inputs safely', () => {
  const t1 = parseTooltipShortcut('')
  assert.equal(t1.label, '')
  assert.equal(t1.shortcut, null)

  // @ts-expect-error test non-string runtime safety
  const t2 = parseTooltipShortcut(null)
  assert.equal(t2.label, null)
  assert.equal(t2.shortcut, null)
})

