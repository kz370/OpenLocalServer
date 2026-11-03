/** The General section's deferred form values. */
export type GeneralSettingsDraft = {
  confirmExit: boolean
  confirmStopAll: boolean
  autoFixDiagnostics: boolean
}

/** One `set_setting` call the General section's Save button makes. */
export type SettingWrite = { key: string; value: string | boolean }

/**
 * The deferred General switches written by the Settings Save button.
 */
export function generalSettingWrites(draft: GeneralSettingsDraft): SettingWrite[] {
  return [
    { key: 'exit.confirmation.enabled', value: draft.confirmExit },
    { key: 'tray.stop_all.confirmation.enabled', value: draft.confirmStopAll },
    { key: 'diagnostics.auto_fix', value: draft.autoFixDiagnostics },
  ]
}
