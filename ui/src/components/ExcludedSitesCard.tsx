import { Trash2, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DeletedItem, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction } from '@/lib/hooks'
import { invalidateWebSitesCache } from '@/lib/web'

/** Newest entries are the ones a user is looking for; the rest scroll. */
const VISIBLE = 200
const DEFAULT_TLD = 'local'

type ExcludedRow =
  | { kind: 'project'; folder: DeletedItem; domains: DeletedItem[] }
  | { kind: 'standalone'; item: DeletedItem }

function when(item: DeletedItem): string {
  // Entries stored before the history kept a timestamp read as 0.
  return item.deleted_at > 0 ? timeAgo(item.deleted_at) : 'date unknown'
}

function projectHostname(path: string, defaultTld: string): string | null {
  const folderName = path.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1)
  if (!folderName) return null
  const name = folderName.replace(new RegExp(`\\.${defaultTld.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`, 'i'), '')
  const slug = name
    .replace(/[^a-z0-9]+/gi, '-')
    .replace(/^-|-$/g, '')
    .toLowerCase()
  return slug ? `${slug}.${defaultTld}` : null
}

function groupItems(items: DeletedItem[], defaultTld: string): ExcludedRow[] {
  const folders = items.filter((item) => item.kind === 'project')
  const domains = items.filter((item) => item.kind === 'domain')
  const groups = folders.map((folder) => ({ kind: 'project' as const, folder, domains: [] as DeletedItem[] }))
  const foldersByHostname = new Map<string, typeof groups>()

  for (const group of groups) {
    const hostname = projectHostname(group.folder.value, defaultTld)?.toLowerCase()
    if (!hostname) continue
    const matches = foldersByHostname.get(hostname) ?? []
    matches.push(group)
    foldersByHostname.set(hostname, matches)
  }

  const standalone: ExcludedRow[] = []
  for (const item of domains) {
    const matches = foldersByHostname.get(item.value.toLowerCase())
    if (matches?.length === 1) matches[0].domains.push(item)
    else standalone.push({ kind: 'standalone', item })
  }

  return [...groups, ...standalone].sort((a, b) => {
    const aTime = a.kind === 'project' ? a.folder.deleted_at : a.item.deleted_at
    const bTime = b.kind === 'project' ? b.folder.deleted_at : b.item.deleted_at
    return bTime - aTime
  })
}

/**
 * Sites and project folders the user took out of the list. They stay out of folder scans
 * and automatic domains until an entry is cleared here — that is the way back when a scan
 * finds nothing and the folder looks like it was blocked.
 */
export function ExcludedSitesCard() {
  const [items, setItems] = useState<DeletedItem[] | null>(null)
  const [defaultTld, setDefaultTld] = useState(DEFAULT_TLD)
  const [saved, setSaved] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    try {
      const r = await runCommand({ type: 'list_deleted_items' })
      if (r.type !== 'deleted_items') {
        setError({
          problem: 'The excluded list could not be loaded.',
          cause: 'The app answered with an unexpected result.',
          fix: 'Close Settings and open it again.',
        })
        return
      }
      setItems(r.items)
    } catch (e) {
      setError(asDiagnostic(e))
      return
    }

    try {
      const r = await runCommand({ type: 'get_setting', key: 'domains.default_tld' })
      if (r.type !== 'setting') {
        setError({
          problem: 'Hidden sites could not be matched to their project folders.',
          cause: 'The app answered with an unexpected result.',
          fix: 'Close Settings and open it again.',
        })
        return
      }
      const tld = typeof r.value === 'string' ? r.value.trim().replace(/^\.+/, '') : DEFAULT_TLD
      setDefaultTld(tld || DEFAULT_TLD)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }, [setError])
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

  async function forget(itemsToForget: DeletedItem[], done: string, message: string, title: string) {
    const ok = await confirmAction(message, title, 'Restore')
    if (!ok) return
    await run('forget', async () => {
      let latestItems: DeletedItem[] | null = null
      try {
        for (const item of itemsToForget) {
          const r = await runCommand({ type: 'forget_deleted_item', value: item.value })
          if (r.type !== 'deleted_items') {
            throw new Error('The app answered with an unexpected result while restoring hidden items.')
          }
          latestItems = r.items
        }
        if (latestItems) {
          setItems(latestItems)
          flash(done)
        }
      } finally {
        if (latestItems) {
          setItems(latestItems)
          invalidateWebSitesCache()
        }
      }
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
  const rows = groupItems(items, defaultTld)
  const shown = rows.slice(0, VISIBLE)

  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Hidden sites</CardTitle>
          <CardDescription>
            Sites and project folders hidden from folder scans and automatic domains via Hide/Exclude. Clear an entry here to
            let it come back; files on disk are never touched.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {items.length === 0 ? (
            <p className="text-sm text-muted-foreground">Nothing is hidden or excluded. Sites and project folders you hide/exclude will appear here.</p>
          ) : (
            <>
              <div className="flex items-center justify-between gap-3" aria-live="polite">
                <span className="text-xs text-muted-foreground">
                  {rows.length} hidden {rows.length === 1 ? 'project' : 'projects'}
                </span>
                <Button size="sm" variant="ghost" className="text-destructive" disabled={busy !== null} onClick={clearAll}>
                  <Trash2 className="size-3.5" /> Clear all
                </Button>
              </div>
              <div className="divide-y divide-border overflow-hidden rounded-lg border border-border">
                {shown.map((row) => {
                  if (row.kind === 'standalone') {
                    return (
                      <div key={`${row.item.kind}:${row.item.value}`} className="flex items-center justify-between gap-3 px-3 py-2">
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium" title={row.item.value}>
                            {row.item.value}
                          </p>
                          <div className="truncate text-xs text-muted-foreground">
                            {when(row.item)} · <Badge variant="secondary">site</Badge>
                          </div>
                        </div>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-7 shrink-0 px-0"
                          disabled={busy !== null}
                          title="Restore this domain only"
                          aria-label={`Restore ${row.item.value} only`}
                          onClick={() =>
                            void forget(
                              [row.item],
                              `${row.item.value} can be created again.`,
                              `Restore ${row.item.value}? It will be available to automatic domains again.`,
                              'Restore site',
                            )
                          }
                        >
                          {busy === 'forget' ? <Spinner /> : <Trash2 className="size-3.5" />}
                        </Button>
                      </div>
                    )
                  }

                  const groupItems = [row.folder, ...row.domains]
                  return (
                    <div key={`project:${row.folder.value}`} className="flex items-start justify-between gap-3 px-3 py-2">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium" title={row.folder.value}>
                          {row.folder.value}
                        </p>
                        <div className="truncate text-xs text-muted-foreground">
                          {when(row.folder)} · <Badge variant="secondary">folder</Badge>
                        </div>
                        {row.domains.length > 0 && (
                          <div className="flex min-w-0 flex-wrap gap-1.5 pt-1.5">
                            {row.domains.map((domain) => (
                              <Badge key={domain.value} variant="secondary" className="max-w-full min-w-0 gap-1 px-2 py-0.5">
                                <span className="min-w-0 truncate" title={domain.value}>
                                  {domain.value}
                                </span>
                                <span className="shrink-0 text-muted-foreground">site</span>
                                <Button
                                  size="sm"
                                  variant="ghost"
                                  className="size-4 shrink-0 rounded-full p-0 text-muted-foreground [&_svg]:size-3"
                                  disabled={busy !== null}
                                  title="Restore this domain only"
                                  aria-label={`Restore ${domain.value} only`}
                                  onClick={() =>
                                    void forget(
                                      [domain],
                                      `${domain.value} can be created again.`,
                                      `Restore ${domain.value} only? Its project folder and other hidden domains will remain hidden.`,
                                      'Restore site',
                                    )
                                  }
                                >
                                  <X />
                                </Button>
                              </Badge>
                            ))}
                          </div>
                        )}
                      </div>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="size-7 shrink-0 px-0"
                        disabled={busy !== null}
                        title="Restore project and its domains"
                        aria-label={`Restore project and its domains: ${row.folder.value}`}
                        onClick={() =>
                          void forget(
                            groupItems,
                            `${row.folder.value} and its domains can be scanned again.`,
                            `Restore ${row.folder.value} and its ${row.domains.length} hidden ${row.domains.length === 1 ? 'domain' : 'domains'}? They will be available to folder scans and automatic domains again.`,
                            'Restore project',
                          )
                        }
                      >
                        {busy === 'forget' ? <Spinner /> : <Trash2 className="size-3.5" />}
                      </Button>
                    </div>
                  )
                })}
              </div>
              {rows.length > shown.length && (
                <p className="text-xs text-muted-foreground">Showing the {shown.length} newest of {rows.length}.</p>
              )}
            </>
          )}
          <span className="block min-h-5 text-sm text-success">{saved ?? '\u00a0'}</span>
        </CardContent>
      </Card>
    </>
  )
}
