import { ArrowDownToLine, Layers3, Play, Search, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ScrollableList } from '@/components/ScrollableList'
import { Input } from '@/components/ui/input'
import { Pagination } from '@/components/ui/pagination'
import { type Diagnostic, type DockerImage, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'
import { createPersistentCache } from '@/lib/cache'

const imagesCache = createPersistentCache<DockerImage[] | null>('docker_images', null)

import { CopyId } from './DockerRows'
import { DiskUsageBar } from './DiskUsageBar'
import { RunImageDialog } from './RunImageDialog'
import { ago, formatBytes } from './storageFormat'

type Filter = 'all' | 'used' | 'unused' | 'dangling'

/** §166 Images tab: pull, run, remove; search, filters and pages. Loads when opened. */
export function ImagesTab({ reloadKey, onRunTask, onChanged }: { reloadKey: number; onRunTask: (id: string) => void; onChanged: () => void }) {
  const [images, setImages] = useState<DockerImage[] | null>(imagesCache.get())
  const [error, setError] = useState<Diagnostic | null>(null)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)
  const [pull, setPull] = useState('')
  const [runFor, setRunFor] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const r = await runCommand({ type: 'list_docker_images' })
      if (r.type === 'docker_images') {
        imagesCache.set(r.images)
        setImages((prev) => (JSON.stringify(prev) === JSON.stringify(r.images) ? prev : r.images))
      }
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load, reloadKey])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (images ?? []).filter((i) => {
      if (filter === 'used' && i.used_by.length === 0) return false
      if (filter === 'unused' && i.used_by.length > 0) return false
      if (filter === 'dangling' && !i.dangling) return false
      return !q || i.tags.some((t) => t.toLowerCase().includes(q)) || i.id.includes(q)
    })
  }, [images, query, filter])
  const pageCount = Math.max(1, Math.ceil(shown.length / perPage))
  const current = Math.min(page, pageCount)
  const rows = shown.slice((current - 1) * perPage, current * perPage)

  async function startPull() {
    const ref = pull.trim()
    if (!ref) return
    try {
      const r = await runCommand({ type: 'pull_docker_image', reference: ref })
      if (r.type === 'task_started') {
        setPull('')
        onRunTask(r.id)
      }
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  async function remove(img: DockerImage) {
    const label = img.tags[0] ?? img.id.slice(7, 19)
    const inUse = img.used_by.length > 0
    if (!(await confirmAction(`Remove ${label}? ${inUse ? `Containers still use it (${img.used_by.join(', ')}); remove them first.` : 'It downloads again if a container needs it.'}`, 'Remove image?', 'Remove')))
      return
    setBusy(img.id)
    setError(null)
    try {
      await runCommand({ type: 'remove_docker_image', id: img.tags[0] ?? img.id, force: false })
      notify.success(label, { title: 'Image removed' })
      await load()
      onChanged()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="shrink-0">
        <DiskUsageBar reloadKey={reloadKey} onRunTask={onRunTask} onError={setError} />
      </div>
      <ScrollableList
        className="@container"
        toolbar={
          <div className="flex flex-col gap-3 border-b p-4 @3xl:flex-row @3xl:items-center @3xl:justify-between">
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <div className="relative min-w-0 flex-1">
                <Search className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={query} onChange={(e) => { setQuery(e.target.value); setPage(1) }} placeholder="Search images" className="h-8 pl-8" />
              </div>
              <div className="flex flex-wrap gap-1">
                {(['all', 'used', 'unused', 'dangling'] as Filter[]).map((f) => (
                  <Button key={f} size="sm" variant={filter === f ? 'secondary' : 'ghost'} className="h-8 cursor-pointer text-xs" onClick={() => { setFilter(f); setPage(1) }}>
                    {f === 'all' ? 'All' : f === 'used' ? 'In use' : f === 'unused' ? 'Unused' : 'Untagged'}
                  </Button>
                ))}
              </div>
            </div>
            <form className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); void startPull() }}>
              <Input value={pull} onChange={(e) => setPull(e.target.value)} placeholder="Image to pull, e.g. nginx:1.28" className="h-8 w-full @3xl:w-64" aria-label="Image to pull" />
              <Button size="sm" type="submit" className="h-8 shrink-0 cursor-pointer" disabled={!pull.trim()}>
                <ArrowDownToLine className="size-3.5" /> Pull
              </Button>
            </form>
          </div>
        }
        footer={
          shown.length > 0 ? (
            <Pagination label="images" total={shown.length} page={current} perPage={perPage} onPage={setPage} onPerPage={(n) => { setPerPage(n); setPage(1) }} className="border-t border-border" />
          ) : null
        }
      >
          {images === null ? (
            <p className="flex items-center gap-2 px-4 pb-4 text-sm text-muted-foreground"><Spinner className="size-4" /> Reading images…</p>
          ) : rows.length === 0 ? (
            <p className="px-4 pb-6 text-center text-sm text-muted-foreground">{images.length === 0 ? 'No images yet. Pull one, or build a Compose stack.' : 'No images match.'}</p>
          ) : (
            <div className="divide-y">
              {rows.map((img) => (
                <div key={img.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-4 gap-y-1 px-4 py-3 @3xl:grid-cols-[minmax(0,2.4fr)_6rem_8rem_minmax(0,1.4fr)_auto] @3xl:items-center">
                  <div className="flex min-w-0 items-start gap-2">
                    <Layers3 className="mt-0.5 size-4 shrink-0 text-sky-500" />
                    <div className="min-w-0">
                      <div className="font-medium break-all">{img.tags.join(', ') || <span className="text-muted-foreground">untagged</span>}</div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-1">
                        <CopyId id={img.id.replace(/^sha256:/, '')} />
                        {img.dangling && <Badge variant="outline">untagged</Badge>}
                      </div>
                    </div>
                  </div>
                  <div className="col-start-2 row-start-1 flex justify-end gap-0.5 @3xl:col-start-5">
                    <ServiceIconAction title={img.tags[0] ? `Run ${img.tags[0]}` : 'Tag it first to run it'} disabled={!img.tags[0]} iconColor="text-emerald-400" onClick={() => img.tags[0] && setRunFor(img.tags[0])}>
                      <Play className="size-3.5" />
                    </ServiceIconAction>
                    <ServiceIconAction title={`Remove ${img.tags[0] ?? 'image'}`} disabled={busy === img.id} spinning={busy === img.id} iconColor="text-destructive/70" onClick={() => void remove(img)}>
                      <Trash2 className="size-3.5" />
                    </ServiceIconAction>
                  </div>
                  {/* Narrow: one line under the name. Wide: three columns. */}
                  <div className="col-span-2 flex flex-wrap gap-x-3 gap-y-0.5 pl-6 text-xs text-muted-foreground @3xl:contents">
                    <span className="tabular-nums @3xl:col-start-2 @3xl:row-start-1 @3xl:text-sm @3xl:text-foreground">{formatBytes(img.size)}</span>
                    <span className="@3xl:col-start-3 @3xl:row-start-1">{ago(img.created)}</span>
                    <span className="break-words @3xl:col-start-4 @3xl:row-start-1">{img.used_by.length ? `Used by ${img.used_by.join(', ')}` : 'Not used'}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
      </ScrollableList>
      {runFor && <RunImageDialog image={runFor} onClose={() => setRunFor(null)} onStarted={onChanged} />}
    </div>
  )
}
