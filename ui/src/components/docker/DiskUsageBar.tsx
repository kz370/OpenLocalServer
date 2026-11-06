import { Eraser, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { asDiagnostic } from '@/lib/diagnostic'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { type Diagnostic, type DockerDiskUsage, runCommand } from '@/core'
import { createPersistentCache } from '@/lib/cache'
import { cn } from '@/lib/utils'

import { ReclaimDialog } from './ReclaimDialog'
import { formatBytes } from './storageFormat'

const usageCache = createPersistentCache<DockerDiskUsage | null>('docker_disk_usage', null)

/**
 * §166: how much disk Docker uses and how much can be reclaimed. Shows the last known
 * numbers at once (cached, also across restarts) and refreshes them in the background when the Images
 * or Volumes tab opens; the screen only updates if they changed. Refresh forces a fresh read.
 */
export function DiskUsageBar({ reloadKey, onRunTask, onError }: { reloadKey: number; onRunTask: (taskId: string) => void; onError: (d: Diagnostic) => void }) {
  const [usage, setUsage] = useState<DockerDiskUsage | null>(usageCache.get())
  const [loading, setLoading] = useState(true)
  const [reclaim, setReclaim] = useState(false)

  const load = useCallback(
    async (fresh: boolean) => {
      try {
        const r = await runCommand({ type: 'get_docker_disk_usage', fresh })
        if (r.type === 'docker_disk_usage') {
          usageCache.set(r.usage)
          setUsage((prev) => (JSON.stringify(prev) === JSON.stringify(r.usage) ? prev : r.usage))
        }
      } catch (err) {
        onError(asDiagnostic(err))
      } finally {
        setLoading(false)
      }
    },
    [onError],
  )

  useEffect(() => {
    void load(reloadKey > 0)
  }, [load, reloadKey])

  const cells: [string, number, number][] = usage
    ? [
        ['Images', usage.images, usage.images_reclaimable],
        ['Containers', usage.containers, usage.containers_reclaimable],
        ['Volumes', usage.volumes, usage.volumes_reclaimable],
        ['Build cache', usage.build_cache, usage.build_cache_reclaimable],
      ]
    : []
  const reclaimable = usage ? usage.images_reclaimable + usage.containers_reclaimable + usage.build_cache_reclaimable : 0

  return (
    <>
    <Card className="@container">
      <CardContent className="flex flex-col gap-3 py-4 @3xl:flex-row @3xl:items-center @3xl:justify-between">
        <div className="grid grid-cols-2 gap-3 @xl:grid-cols-4 @3xl:flex-1">
          {usage
            ? cells.map(([label, total, free]) => (
                <div key={label} className="min-w-0">
                  <p className="text-xs text-muted-foreground">{label}</p>
                  <p className="text-sm font-medium tabular-nums">{formatBytes(total)}</p>
                  <p className="text-xs text-muted-foreground tabular-nums">{formatBytes(free)} unused</p>
                </div>
              ))
            : <p className="col-span-full text-sm text-muted-foreground">{loading ? 'Reading Docker disk usage…' : 'Disk usage unavailable.'}</p>}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            className="cursor-pointer"
            disabled={loading}
            onClick={() => {
              setLoading(true)
              void load(true)
            }}
          >
            <RefreshCw className={cn('size-3.5', loading && 'animate-spin')} /> Refresh
          </Button>
          <Button size="sm" className="cursor-pointer" onClick={() => setReclaim(true)} title={`About ${formatBytes(reclaimable)} can be freed without touching volumes`}>
            <Eraser className="size-3.5" /> Reclaim space
          </Button>
        </div>
      </CardContent>
    </Card>
    {/* Outside the card: a card's blur makes its own layer, which would draw later cards over the dialog. */}
    {reclaim && <ReclaimDialog onClose={() => setReclaim(false)} onRunTask={onRunTask} />}
    </>
  )
}
