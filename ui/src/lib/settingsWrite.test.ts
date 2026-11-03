import test from 'node:test'
import assert from 'node:assert/strict'
import { generalSettingWrites, type GeneralSettingsDraft } from './settingsWrite.ts'

const base: GeneralSettingsDraft = {
  editor: '',
  editorId: 'vscode',
  confirmExit: true,
  confirmStopAll: true,
  autoFixDiagnostics: true,
}

test('writes the confirmation and diagnostics switches with their current values', () => {
  const writes = generalSettingWrites({
    ...base,
    confirmExit: false,
    confirmStopAll: false,
    autoFixDiagnostics: false,
  })
  assert.deepEqual(
    writes.filter((w) => w.key.endsWith('.enabled') || w.key === 'diagnostics.auto_fix'),
    [
      { key: 'exit.confirmation.enabled', value: false },
      { key: 'tray.stop_all.confirmation.enabled', value: false },
      { key: 'diagnostics.auto_fix', value: false },
    ]
  )
})

test('preset editor writes its id and blanks the custom command', () => {
  const writes = generalSettingWrites({ ...base, editorId: 'vscode', editor: 'ignored' })
  assert.deepEqual(writes.slice(0, 2), [
    { key: 'editor', value: 'vscode' },
    { key: 'editor.command', value: '' },
  ])
})

test('custom editor blanks the id and writes the path', () => {
  const writes = generalSettingWrites({ ...base, editorId: 'custom', editor: 'C:\\bin\\code.cmd' })
  assert.deepEqual(writes.slice(0, 2), [
    { key: 'editor', value: '' },
    { key: 'editor.command', value: 'C:\\bin\\code.cmd' },
  ])
})
