import { Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DeletedItem, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction } from '@/lib/hooks'

/** Newest entries are the ones a user is looking for; the rest scroll. */
const VISIBLE = 200

function when(item: DeletedItem): string {
  // Entries stored before the history kept a timestamp read as 0.
  return item.deleted_at > 0 ? timeAgo(item.deleted_at) : 'date unknown'
}

/**
 * Sites and project folders the user took out of the list. They stay out of folder scans
 * and automatic domains until an entry is cleared here — that is the way back when a scan
 * finds nothing and the folder looks like it was blocked.
 */
export function ExcludedSitesCard() {
  const [items, setItems] = useState<DeletedItem[] | null>(null)
  const [saved, setSaved] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_deleted_items' })
    if (r.type === 'deleted_items') setItems(r.items)
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  function flash(message: string) {
    setSaved(message)
    setTimeout(() => setSaved(null), 2500)
  }

  /** Reads the returned list back so the row disappears without a second round trip. */
  async function apply(r: Awaited<ReturnType<typeof runCommand>>, done: string) {
    if (r.type === 'deleted_items') {
      setItems(r.items)
      flash(done)
    } else {
      setError({
        problem: 'The excluded list did not change.',
        cause: 'The app answered with an unexpected result.',
        fix: 'Close Settings and open it again.',
      })
    }
  }

  async function forget(item: DeletedItem) {
    await run('forget', async () => {
      const r = await runCommand({ type: 'forget_deleted_item', value: item.value })
      await apply(r, item.kind === 'domain' ? `${item.value} can be created again.` : `${item.value} can be scanned again.`)
    })
  }

  async function clearAll() {
    const count = items?.length ?? 0
    const ok = await confirmAction(
      `Folder scans and automatic domains will see these ${count === 1 ? 'folder' : 'folders'} again if ${count === 1 ? 'it is' : 'they are'} still on disk. Your files are not deleted.`,
      `Clear all ${count} excluded ${count === 1 ? 'entry' : 'entries'}?`,
      'Clear all',
    )
    if (!ok) return
    await run('clear', async () => {
      const r = await runCommand({ type: 'clear_deleted_items' })
      await apply(r, 'Excluded list cleared.')
    })
  }

  if (!items) return null
  const shown = items.slice(0, VISIBLE)

  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Excluded sites</CardTitle>
          <CardDescription>
            Removing a site or a project folder keeps it out of folder scans and automatic domains. Clear an entry here to
            let it come back; the files on disk are never touched.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {items.length === 0 ? (
            <p className="text-sm text-muted-foreground">Nothing is excluded. Sites and project folders you remove will appear here.</p>
          ) : (
            <>
              <div className="flex items-center justify-between gap-3" aria-live="polite">
                <span className="text-xs text-muted-foreground">
                  {items.length} excluded {items.length === 1 ? 'entry' : 'entries'}
                </span>
                <Button size="sm" variant="ghost" className="text-destructive" disabled={busy !== null} onClick={clearAll}>
                  <Trash2 className="size-3.5" /> Clear all
                </Button>
              </div>
              <div className="divide-y divide-border overflow-hidden rounded-lg border border-border">
                {shown.map((item) => (
                  <div key={`${item.kind}:${item.value}`} className="flex items-center justify-between gap-3 px-3 py-2">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium" title={item.value}>
                        {item.value}
                      </p>
                      <p className="truncate text-xs text-muted-foreground">
                        {when(item)} · <Badge variant="secondary">{item.kind === 'domain' ? 'site' : 'folder'}</Badge>
                      </p>
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="size-7 shrink-0 px-0"
                      disabled={busy !== null}
                      title={item.kind === 'domain' ? 'Let automatic domains create this site again' : 'Let folder scans register this folder again'}
                      aria-label={`Stop excluding ${item.value}`}
                      onClick={() => forget(item)}
                    >
                      {busy === 'forget' ? <Spinner /> : <Trash2 className="size-3.5" />}
                    </Button>
                  </div>
                ))}
              </div>
              {items.length > shown.length && (
                <p className="text-xs text-muted-foreground">Showing the {shown.length} newest of {items.length}.</p>
              )}
            </>
          )}
          {saved && <span className="text-sm text-success">{saved}</span>}
        </CardContent>
      </Card>
    </>
  )
}
