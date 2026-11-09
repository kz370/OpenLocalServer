import { type EditorInfo, runCommand } from '@/core'

/** The editors found on this machine, read once and shared by every "Open with" menu. */
let editorsCache: Promise<EditorInfo[]> | null = null

export function loadEditors(): Promise<EditorInfo[]> {
  editorsCache ??= runCommand({ type: 'list_editors' })
    .then((r) => (r.type === 'editors' ? r.editors : []))
    .catch(() => {
      editorsCache = null
      return []
    })
  return editorsCache
}

/** Forget the shared list so the next menu reads it again (Settings → Editors → Refresh). */
export function invalidateEditorsCache() {
  editorsCache = null
}
