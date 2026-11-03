import test from 'node:test'
import assert from 'node:assert/strict'
import { generalSettingWrites, type GeneralSettingsDraft } from './settingsWrite.ts'

const base: GeneralSettingsDraft = {
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

test('does not write legacy editor settings', () => {
  const keys = generalSettingWrites(base).map((write) => write.key)
  assert.deepEqual(keys, [
    'exit.confirmation.enabled',
    'tray.stop_all.confirmation.enabled',
    'diagnostics.auto_fix',
  ])
})
