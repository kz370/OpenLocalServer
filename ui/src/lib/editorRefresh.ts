/** The fields of an editor entry the refresh summary needs. */
export interface DetectedEditor {
  id: string
  name: string
  path?: string | null
}

export interface EditorRefreshSummary {
  /** `success` when something changed, `info` when the list is the same as before. */
  kind: 'success' | 'info'
  message: string
}

const names = (list: DetectedEditor[]) => list.map((e) => e.name).join(', ')
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/**
 * What Settings → Editors → Refresh found, compared with the list shown before it ran:
 * editors that appeared, editors that are gone, and how many are installed now.
 */
export function summarizeEditorRefresh(before: DetectedEditor[], after: DetectedEditor[]): EditorRefreshSummary {
  const installed = (list: DetectedEditor[]) => list.filter((e) => e.path)
  const was = installed(before)
  const now = installed(after)
  const added = now.filter((e) => !was.some((w) => w.id === e.id))
  const removed = was.filter((e) => !now.some((n) => n.id === e.id))
  const total = plural(now.length, 'editor', 'editors')

  const parts: string[] = []
  if (added.length) parts.push(`Found ${plural(added.length, 'new editor', 'new editors')}: ${names(added)}.`)
  if (removed.length) parts.push(`No longer found: ${names(removed)}.`)
  if (parts.length) return { kind: 'success', message: `${parts.join(' ')} ${total} available.` }
  if (now.length === 0) return { kind: 'info', message: 'No editors found. Use "Add editor" to pick one by its program file.' }
  return { kind: 'info', message: `No new editors found. ${total} available.` }
}
