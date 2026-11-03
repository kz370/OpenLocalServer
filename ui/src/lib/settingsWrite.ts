/** The General section's deferred form values. */
export type GeneralSettingsDraft = {
  editor: string
  editorId: string
  confirmExit: boolean
  confirmStopAll: boolean
  autoFixDiagnostics: boolean
}

/** One `set_setting` call the General section's Save button makes. */
export type SettingWrite = { key: string; value: string | boolean }

/**
 * The settings written when General is saved. One builder so the editor pair and
 * the three switches are always written together, never one without the other.
 */
export function generalSettingWrites(draft: GeneralSettingsDraft): SettingWrite[] {
  return [
    { key: 'editor', value: draft.editorId === 'custom' ? '' : draft.editorId },
    { key: 'editor.command', value: draft.editorId === 'custom' ? draft.editor : '' },
    { key: 'exit.confirmation.enabled', value: draft.confirmExit },
    { key: 'tray.stop_all.confirmation.enabled', value: draft.confirmStopAll },
    { key: 'diagnostics.auto_fix', value: draft.autoFixDiagnostics },
  ]
}
