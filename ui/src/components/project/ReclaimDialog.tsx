import { Eraser } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Select } from '@/components/ui/form'
import { type Diagnostic, type ReclaimReport, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { asDiagnostic } from '@/lib/diagnostic'
import { formatBytes } from '@/lib/hooks'

/** The key a folder is picked by. */
const keyOf = (projectId: string, relative: string) => `${projectId}\u0000${relative}`

/**
 * §170 Free up space: scans registered projects for node_modules, vendor and build caches,
 * lists them with sizes and how to rebuild them, and deletes the ones the user picks. Deletion
 * is permanent; the core re-checks every path against a closed list before removing it.
 */
export function ReclaimDialog({ onClose }: { onClose: () => void }) {
  const [report, setReport] = useState<ReclaimReport | null>(null)
  const [days, setDays] = useState(90)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [taskId, setTaskId] = useState<string | null>(null)
  const [mode, setMode] = useState<'scan' | 'delete' | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [loading, setLoading] = useState(true)

  const visible = useMemo(() => (report?.projects ?? []).filter((p) => p.idle_days >= days), [report, days])

  const selectDefaults = useCallback((r: ReclaimReport, minDays: number) => {
    const next = new Set<string>()
    for (const p of r.projects) {
      if (p.busy || p.idle_days < minDays) continue
      for (const it of p.items) if (it.default_selected) next.add(keyOf(p.project_id, it.relative))
    }
    setPicked(next)
  }, [])

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_reclaim_report' })
    if (r.type === 'reclaim_report') {
      setReport(r.report)
      if (r.report) selectDefaults(r.report, days)
    }
    setLoading(false)
    // `days` only seeds the default picks; changing it re-seeds below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectDefaults])

  useEffect(() => {
    load().catch((e) => setError(asDiagnostic(e)))
  }, [load])

  const pickedBytes = useMemo(() => {
    let n = 0
    for (const p of visible) for (const it of p.items) if (picked.has(keyOf(p.project_id, it.relative))) n += it.bytes
    return n
  }, [visible, picked])
  const pickedCount = useMemo(() => {
    let n = 0
    for (const p of visible) for (const it of p.items) if (picked.has(keyOf(p.project_id, it.relative))) n++
    return n
  }, [visible, picked])

  async function scan() {
    setError(null)
    try {
      const r = await runCommand({ type: 'scan_reclaim' })
      if (r.type === 'task_started') {
        setMode('scan')
        setTaskId(r.id)
      }
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  async function remove() {
    const items = visible.flatMap((p) => p.items.filter((it) => picked.has(keyOf(p.project_id, it.relative))).map((it) => ({ project_id: p.project_id, relative: it.relative })))
    if (items.length === 0) return
    const ok = await confirmAction(
      `Delete ${items.length} folder${items.length === 1 ? '' : 's'} (${formatBytes(pickedBytes)})? They are removed permanently, not moved to the Recycle Bin. Run the rebuild command shown next to each one to get them back.`,
      'Delete these folders?',
      'Delete',
    )
    if (!ok) return
    try {
      const r = await runCommand({ type: 'reclaim_folders', items })
      if (r.type === 'task_started') {
        setMode('delete')
        setTaskId(r.id)
      }
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  return (
    <>
      <Dialog
        open
        onClose={onClose}
        size="wide"
        icon={<Eraser className="size-5 text-emerald-500" />}
        title="Free up space"
        description="Finds folders that can be rebuilt (node_modules, vendor, build caches) in projects you have not used for a while."
        footer={
          <div className="flex w-full flex-wrap items-center justify-between gap-2">
            <span className="text-sm text-muted-foreground">{pickedCount > 0 ? `${pickedCount} folder${pickedCount === 1 ? '' : 's'} · ${formatBytes(pickedBytes)}` : 'Nothing chosen'}</span>
            <span className="flex gap-2">
              <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
                Close
              </Button>
              <Button size="sm" variant="destructive" className="cursor-pointer" disabled={pickedCount === 0} onClick={() => void remove()}>
                Delete selected
              </Button>
            </span>
          </div>
        }
      >
        <div className="@container flex flex-col gap-4 text-sm">
          <ErrorCard error={error} onDismiss={() => setError(null)} />
          <div className="flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-2">
              Unused for at least
              <Select
                className="w-36"
                value={String(days)}
                onChange={(e) => {
                  const d = Number(e.target.value)
                  setDays(d)
                  if (report) selectDefaults(report, d)
                }}
              >
                <option value="0">any time</option>
                <option value="30">30 days</option>
                <option value="60">60 days</option>
                <option value="90">90 days</option>
                <option value="180">180 days</option>
              </Select>
            </label>
            <Button size="sm" variant="secondary" className="cursor-pointer" onClick={() => void scan()}>
              {report ? 'Scan again' : 'Scan'}
            </Button>
          </div>
          {loading ? (
            <p className="flex items-center gap-2 text-muted-foreground">
              <Spinner className="size-4" /> Loading…
            </p>
          ) : !report ? (
            <p className="text-muted-foreground">Press Scan to look. Nothing is deleted until you pick folders and confirm.</p>
          ) : visible.length === 0 ? (
            <p className="text-muted-foreground">No project unused for {days} days or more has folders to clear.</p>
          ) : (
            <div className="flex max-h-96 flex-col gap-3 overflow-y-auto">
              {visible.map((p) => (
                <div key={p.project_id} className="rounded-md border">
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-1.5">
                    <span className="font-medium">{p.name}</span>
                    <span className="text-xs text-muted-foreground">
                      unused for {p.idle_days} day{p.idle_days === 1 ? '' : 's'}
                      {p.busy && <Badge variant="secondary" className="ml-2">running</Badge>}
                    </span>
                  </div>
                  <div className="divide-y">
                    {p.items.map((it) => {
                      const k = keyOf(p.project_id, it.relative)
                      return (
                        <label key={k} className={`flex items-start gap-2 px-3 py-1.5 ${p.busy ? 'opacity-60' : 'cursor-pointer'}`}>
                          <Checkbox
                            checked={picked.has(k)}
                            disabled={p.busy}
                            onChange={(v) => {
                              const next = new Set(picked)
                              if (v) next.add(k)
                              else next.delete(k)
                              setPicked(next)
                            }}
                          />
                          <span className="min-w-0 flex-1">
                            <span className="font-mono text-xs break-all">{it.relative}</span>
                            <span className="block text-xs text-muted-foreground">
                              {p.busy ? 'Stop its processes first.' : `Rebuild with: ${it.rebuild}`}
                              {it.kind === 'venv' && ' The site stops working until the environment is recreated.'}
                            </span>
                          </span>
                          <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                            {it.size_is_minimum ? 'at least ' : ''}
                            {formatBytes(it.bytes)}
                          </span>
                        </label>
                      )
                    })}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </Dialog>
      <TaskOutputDialog
        taskId={taskId}
        onClose={() => setTaskId(null)}
        onFinished={() => {
          if (mode === 'scan') void load()
          else {
            setReport(null)
            setPicked(new Set())
          }
        }}
      />
    </>
  )
}
