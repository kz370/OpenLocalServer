import { AlertTriangle, Eraser } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { type Diagnostic, type PrunePlan, type PruneTargets, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'

import { formatBytes } from './storageFormat'

const OPTIONS: { key: keyof PruneTargets; label: string; hint: string; danger?: boolean }[] = [
  { key: 'stopped_containers', label: 'Stopped containers', hint: 'Containers that are not running. Their volumes are kept.' },
  { key: 'dangling_images', label: 'Untagged images', hint: 'Leftover image layers with no name, from rebuilds and updates.' },
  { key: 'unused_images', label: 'All unused images', hint: 'Every image no container uses. They download again when needed.' },
  { key: 'build_cache', label: 'Build cache', hint: 'Cached build steps nothing is using right now.' },
  { key: 'unused_volumes', label: 'Unused volumes', hint: 'Volumes no container uses. This deletes the data in them.', danger: true },
]

/**
 * §166 Reclaim space: choose what to clean, see exactly what would be removed and how much
 * space it frees (a dry run from Docker's own disk usage), then run it as a background task.
 */
export function ReclaimDialog({ onClose, onRunTask }: { onClose: () => void; onRunTask: (taskId: string) => void }) {
  const [targets, setTargets] = useState<PruneTargets>({ stopped_containers: false, dangling_images: true, build_cache: true })
  const [plan, setPlan] = useState<PrunePlan | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<Diagnostic | null>(null)

  const any = useMemo(() => Object.values(targets).some(Boolean), [targets])

  useEffect(() => {
    let alive = true
    const t = setTimeout(() => {
      void runCommand({ type: 'plan_docker_prune', targets })
        .then((r) => alive && r.type === 'docker_prune_plan' && setPlan(r.plan))
        .catch((err) => alive && setError(asDiagnostic(err)))
        .finally(() => alive && setLoading(false))
    }, 250)
    return () => {
      alive = false
      clearTimeout(t)
    }
  }, [targets])

  async function run() {
    if (!plan || plan.items.length === 0) return
    if (
      targets.unused_volumes &&
      !(await confirmAction(
        'Unused volumes will be deleted with the data inside them. This cannot be undone.',
        'Delete unused volumes?',
        'Delete',
      ))
    )
      return
    try {
      const r = await runCommand({ type: 'prune_docker', targets })
      if (r.type === 'task_started') {
        onRunTask(r.id)
        onClose()
      }
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      size="wide"
      icon={<Eraser className="size-5 text-emerald-500" />}
      title="Reclaim space"
      description="Choose what to clean. You see exactly what will be removed before anything is."
      footer={
        <div className="flex w-full flex-wrap items-center justify-between gap-2">
          <span className="text-sm text-muted-foreground">
            {plan && any ? `${plan.items.length} item${plan.items.length === 1 ? '' : 's'} · about ${formatBytes(plan.total)}` : 'Nothing chosen'}
          </span>
          <span className="flex gap-2">
            <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
              Cancel
            </Button>
            <Button
              size="sm"
              variant={targets.unused_volumes ? 'destructive' : 'default'}
              className="cursor-pointer"
              disabled={!any || loading || !plan || plan.items.length === 0}
              onClick={() => void run()}
            >
              Clean up
            </Button>
          </span>
        </div>
      }
    >
      <div className="@container flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-2 @xl:grid-cols-2">
          {OPTIONS.map((o) => (
            <label key={o.key} className="flex cursor-pointer items-start gap-2 rounded-md border p-3">
              <Checkbox
                checked={!!targets[o.key]}
                onChange={(v) => {
                  setLoading(true)
                  setTargets({ ...targets, [o.key]: v })
                }}
              />
              <span>
                <span className={o.danger ? 'flex items-center gap-1 font-medium text-destructive' : 'font-medium'}>
                  {o.danger && <AlertTriangle className="size-3.5" />} {o.label}
                </span>
                <span className="block text-xs text-muted-foreground">{o.hint}</span>
              </span>
            </label>
          ))}
        </div>
        <div className="flex flex-col gap-1">
          <span className="font-medium">Will be removed</span>
          <div className="max-h-64 overflow-y-auto rounded-md border">
            {loading ? (
              <p className="flex items-center gap-2 p-3 text-muted-foreground">
                <Spinner className="size-4" /> Checking what is unused…
              </p>
            ) : !plan || plan.items.length === 0 ? (
              <p className="p-3 text-muted-foreground">{any ? 'Nothing to clean: everything chosen is in use or already gone.' : 'Choose something to clean.'}</p>
            ) : (
              <div className="divide-y">
                {plan.items.map((it, i) => (
                  <div key={`${it.kind}:${it.name}:${i}`} className="flex items-center justify-between gap-3 px-3 py-1.5">
                    <span className="min-w-0">
                      <span className="mr-2 text-xs text-muted-foreground">{it.kind}</span>
                      <span className="font-mono text-xs break-all">{it.name}</span>
                    </span>
                    <span className="shrink-0 text-xs tabular-nums text-muted-foreground">{formatBytes(it.size)}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </Dialog>
  )
}
