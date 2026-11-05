import { Database, Search, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Pagination } from '@/components/ui/pagination'
import { type Diagnostic, type DockerVolume, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'

import { DiskUsageBar } from './DiskUsageBar'
import { formatBytes } from './storageFormat'

type Filter = 'all' | 'used' | 'unused'

/** §166 Volumes tab: who uses each volume, its size, and removal of unused ones. */
export function VolumesTab({ reloadKey, onRunTask, onChanged }: { reloadKey: number; onRunTask: (id: string) => void; onChanged: () => void }) {
  const [volumes, setVolumes] = useState<DockerVolume[] | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      // Disk usage (read by the bar above) fills in sizes; ask again once it has.
      const r = await runCommand({ type: 'list_docker_volumes' })
      if (r.type === 'docker_volumes') setVolumes(r.volumes)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }, [])

  useEffect(() => {
    void load()
    const t = setTimeout(() => void load(), 1500)
    return () => clearTimeout(t)
  }, [load, reloadKey])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (volumes ?? []).filter((v) => {
      if (filter === 'used' && v.used_by.length === 0) return false
      if (filter === 'unused' && v.used_by.length > 0) return false
      return !q || v.name.toLowerCase().includes(q) || (v.compose_project ?? '').toLowerCase().includes(q)
    })
  }, [volumes, query, filter])
  const pageCount = Math.max(1, Math.ceil(shown.length / perPage))
  const current = Math.min(page, pageCount)
  const rows = shown.slice((current - 1) * perPage, current * perPage)

  async function remove(v: DockerVolume) {
    if (!(await confirmAction(`Delete volume "${v.name}" and all the data in it${v.size ? ` (${formatBytes(v.size)})` : ''}? This cannot be undone.`, 'Delete volume?', 'Delete')))
      return
    setBusy(v.name)
    setError(null)
    try {
      await runCommand({ type: 'remove_docker_volume', name: v.name })
      notify.success(v.name, { title: 'Volume deleted' })
      await load()
      onChanged()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <DiskUsageBar reloadKey={reloadKey} onRunTask={onRunTask} onError={setError} />
      <Card className="@container">
        <CardContent className="flex flex-col gap-3 p-0">
          <div className="flex flex-col gap-2 border-b p-4 @2xl:flex-row @2xl:items-center">
            <div className="relative min-w-0 flex-1">
              <Search className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={query} onChange={(e) => { setQuery(e.target.value); setPage(1) }} placeholder="Search volumes" className="h-8 pl-8" />
            </div>
            <div className="flex flex-wrap gap-1">
              {(['all', 'used', 'unused'] as Filter[]).map((f) => (
                <Button key={f} size="sm" variant={filter === f ? 'secondary' : 'ghost'} className="h-8 cursor-pointer text-xs" onClick={() => { setFilter(f); setPage(1) }}>
                  {f === 'all' ? 'All' : f === 'used' ? 'In use' : 'Unused'}
                </Button>
              ))}
            </div>
          </div>
          {volumes === null ? (
            <p className="flex items-center gap-2 px-4 pb-4 text-sm text-muted-foreground"><Spinner className="size-4" /> Reading volumes…</p>
          ) : rows.length === 0 ? (
            <p className="px-4 pb-6 text-center text-sm text-muted-foreground">{volumes.length === 0 ? 'No volumes yet.' : 'No volumes match.'}</p>
          ) : (
            <div className="divide-y">
              {rows.map((v) => (
                <div key={v.name} className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-4 gap-y-1 px-4 py-3 @3xl:grid-cols-[minmax(0,2.4fr)_6rem_minmax(0,1.6fr)_auto] @3xl:items-center">
                  <div className="flex min-w-0 items-start gap-2">
                    <Database className="mt-0.5 size-4 shrink-0 text-amber-500" />
                    <div className="min-w-0">
                      <div className="font-mono text-sm break-all">{v.name}</div>
                      <div className="mt-1 flex flex-wrap gap-1">
                        {v.compose_project && <Badge variant="outline">Compose · {v.compose_project}</Badge>}
                        {v.anonymous && <Badge variant="outline">unnamed</Badge>}
                        {v.driver !== 'local' && <Badge variant="outline">{v.driver}</Badge>}
                      </div>
                    </div>
                  </div>
                  <div className="col-start-2 row-start-1 flex justify-end @3xl:col-start-4">
                    <ServiceIconAction
                      title={v.used_by.length ? `In use by ${v.used_by.join(', ')}` : `Delete ${v.name}`}
                      disabled={v.used_by.length > 0 || busy === v.name}
                      spinning={busy === v.name}
                      iconColor="text-destructive/70"
                      onClick={() => void remove(v)}
                    >
                      <Trash2 className="size-3.5" />
                    </ServiceIconAction>
                  </div>
                  <div className="col-span-2 flex flex-wrap gap-x-3 gap-y-0.5 pl-6 text-xs text-muted-foreground @3xl:contents">
                    <span className="tabular-nums @3xl:col-start-2 @3xl:row-start-1 @3xl:text-sm @3xl:text-foreground">{formatBytes(v.size)}</span>
                    <span className="break-words @3xl:col-start-3 @3xl:row-start-1">{v.used_by.length ? `Used by ${v.used_by.join(', ')}` : 'Not used'}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
          {shown.length > 0 && (
            <Pagination label="volumes" total={shown.length} page={current} perPage={perPage} onPage={setPage} onPerPage={(n) => { setPerPage(n); setPage(1) }} className="border-t border-border" />
          )}
        </CardContent>
      </Card>
    </div>
  )
}
