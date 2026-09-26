import { ChevronDown, ExternalLink } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { type Diagnostic, type EditorInfo, type Shortcut, runCommand } from '@/core'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'

/** The editors found on this machine, read once and shared by every menu on the page. */
let editorsCache: Promise<EditorInfo[]> | null = null
function loadEditors(): Promise<EditorInfo[]> {
  editorsCache ??= runCommand({ type: 'list_editors' })
    .then((r) => (r.type === 'editors' ? r.editors : []))
    .catch(() => {
      editorsCache = null
      return []
    })
  return editorsCache
}

/**
 * "Open with" (§97): a folder or file in the chosen editor, a specific editor, Explorer, a
 * terminal, or the file's default app. `isDir` hides the choices that only make sense for files.
 */
export function OpenWithMenu({ path, isDir, onError }: { path: string; isDir: boolean; onError: (e: Diagnostic) => void }) {
  const [open, setOpen] = useState(false)
  const [editors, setEditors] = useState<EditorInfo[]>([])
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    void loadEditors().then(setEditors)
    const away = (e: MouseEvent) => box.current && !box.current.contains(e.target as Node) && setOpen(false)
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    window.addEventListener('mousedown', away)
    window.addEventListener('keydown', esc)
    return () => {
      window.removeEventListener('mousedown', away)
      window.removeEventListener('keydown', esc)
    }
  }, [open])

  const go = (app: string) => {
    setOpen(false)
    runCommand({ type: 'open_with', path, app }).catch((e) => onError(asDiagnostic(e)))
  }

  const items: { app: string; label: string }[] = [
    { app: 'editor', label: 'Code editor (your default)' },
    ...editors.filter((e) => e.path).map((e) => ({ app: e.id, label: e.name })),
    { app: 'explorer', label: isDir ? 'File Explorer' : 'Show in File Explorer' },
    { app: 'terminal', label: 'Terminal here' },
    ...(isDir ? [] : [{ app: 'default', label: 'Default app' }]),
  ]

  return (
    <div ref={box} className="relative inline-flex">
      <Button size="sm" variant="ghost" className="h-7 px-2" title="Open with…" onClick={() => setOpen((o) => !o)}>
        <ExternalLink className="size-3.5" /> Open <ChevronDown className="size-3" />
      </Button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-40 mt-1 min-w-52 rounded-lg border border-border bg-card p-1 text-sm shadow-lg">
          {items.map((i) => (
            <button key={i.app} role="menuitem" className="block w-full rounded-md px-3 py-1.5 text-left hover:bg-muted" onClick={() => go(i.app)}>
              {i.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

/** §100: the places a developer keeps jumping to in a project, each with the "Open with" menu. */
export function ProjectShortcuts({ projectId }: { projectId: string }) {
  const [shortcuts, setShortcuts] = useState<Shortcut[]>([])
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    runCommand({ type: 'list_project_shortcuts', project_id: projectId })
      .then((r) => r.type === 'shortcuts' && setShortcuts(r.shortcuts))
      .catch((e) => setError(asDiagnostic(e)))
  }, [projectId])

  if (shortcuts.length === 0 && !error) return null
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-muted-foreground">Open</span>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="rounded-md border border-border">
        {shortcuts.map((s) => (
          <div key={s.id} className="flex items-center justify-between gap-3 border-b border-border px-3 py-1 text-sm last:border-b-0">
            <div className="min-w-0">
              <div>{s.label}</div>
              <div className="truncate font-mono text-xs text-muted-foreground" title={s.path}>
                {s.path}
              </div>
            </div>
            <OpenWithMenu path={s.path} isDir={s.is_dir} onError={setError} />
          </div>
        ))}
      </div>
    </div>
  )
}
