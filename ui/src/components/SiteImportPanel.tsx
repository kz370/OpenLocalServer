import { RotateCw } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { type Diagnostic, type SiteImportCandidate, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'

const keyOf = (c: SiteImportCandidate) => `${c.hostname}\u0000${c.document_root}`

/**
 * §172: the sites Laragon, XAMPP and WampServer have, ready to add to OLS. OLS copies only the
 * list; their files and settings are never changed.
 */
export function SiteImportPanel({ onImported }: { onImported?: () => void }) {
  const [items, setItems] = useState<SiteImportCandidate[] | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [names, setNames] = useState<Record<string, string>>({})
  const [taskId, setTaskId] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const load = useCallback(async () => {
    setItems(null)
    try {
      const r = await runCommand({ type: 'list_site_import_candidates' })
      if (r.type === 'site_import_candidates') setItems(r.candidates)
    } catch (e) {
      setError(asDiagnostic(e))
      setItems([])
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  async function start() {
    if (!items) return
    const chosen = items.filter((c) => picked.has(keyOf(c)))
    try {
      const r = await runCommand({
        type: 'import_site_candidates',
        items: chosen.map((c) => ({ hostname: c.hostname, document_root: c.document_root, new_hostname: names[keyOf(c)]?.trim() || null })),
      })
      if (r.type === 'task_started') setTaskId(r.id)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  const selectable = (c: SiteImportCandidate) => c.exists && !c.already_in_ols

  return (
    <div className="flex flex-col gap-3 text-sm">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <p className="rounded-md border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        OLS copies only the list of sites. Your files stay where they are, and Laragon, XAMPP and WampServer are not changed. Stop their Apache or nginx before starting OLS's web server, because both want ports 80 and 443.
      </p>
      {items === null ? (
        <p className="flex items-center gap-2 text-muted-foreground">
          <Spinner /> Looking for sites…
        </p>
      ) : items.length === 0 ? (
        <p className="text-muted-foreground">No sites found in Laragon, XAMPP or WampServer.</p>
      ) : (
        <div className="flex max-h-96 flex-col divide-y overflow-y-auto rounded-md border">
          {items.map((c) => {
            const k = keyOf(c)
            return (
              <div key={k} className={`flex flex-wrap items-center gap-2 px-3 py-2 ${selectable(c) ? '' : 'opacity-60'}`}>
                <Checkbox
                  checked={picked.has(k)}
                  disabled={!selectable(c)}
                  onChange={(v) => {
                    const next = new Set(picked)
                    if (v) next.add(k)
                    else next.delete(k)
                    setPicked(next)
                  }}
                />
                <div className="min-w-0 flex-1 basis-56">
                  <Input
                    aria-label={`Name for ${c.hostname}`}
                    className="h-7"
                    disabled={!selectable(c)}
                    value={names[k] ?? c.hostname}
                    onChange={(e) => setNames({ ...names, [k]: e.target.value })}
                  />
                  <p className="mt-0.5 truncate font-mono text-xs text-muted-foreground" title={c.document_root}>
                    {c.document_root}
                  </p>
                </div>
                <span className="text-xs text-muted-foreground">{c.source}</span>
                {c.already_in_ols && <Badge variant="secondary">already in OLS</Badge>}
                {!c.exists && <Badge variant="destructive">{c.note ?? 'folder missing'}</Badge>}
              </div>
            )
          })}
        </div>
      )}
      <div className="flex gap-2">
        <Button size="sm" disabled={picked.size === 0} className="cursor-pointer" onClick={() => void start()}>
          Import {picked.size || ''} selected
        </Button>
        <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => void load()}>
          <RotateCw /> Look again
        </Button>
      </div>
      <TaskOutputDialog
        taskId={taskId}
        onClose={() => setTaskId(null)}
        onFinished={() => {
          setPicked(new Set())
          void load()
          onImported?.()
        }}
      />
    </div>
  )
}
