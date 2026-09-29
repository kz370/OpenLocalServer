import { CalendarClock, Cog, Pencil, Play, Plus, RotateCw, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type ProcfilePreview, type ScheduledTask, type TaskStatus, type Worker, type WorkerPreset, type WorkerStatus, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction, usePoll } from '@/lib/hooks'

const SCHEDULES = [
  ['every_minute', 'Every minute'],
  ['every_5_minutes', 'Every 5 minutes'],
  ['every_15_minutes', 'Every 15 minutes'],
  ['hourly', 'Hourly'],
  ['daily', 'Daily at midnight'],
  ['weekly', 'Weekly'],
] as const

function inWords(ms: number) {
  const s = Math.round((ms - Date.now()) / 1000)
  if (s < 60) return `in ${Math.max(0, s)}s`
  if (s < 3600) return `in ${Math.round(s / 60)} min`
  if (s < 86400) return `in ${Math.round(s / 3600)} h`
  return new Date(ms).toLocaleString()
}

/** §105 queue workers and §106 scheduled tasks for one project. */
export function WorkersPanel({ projectId }: { projectId: string }) {
  const [workers, setWorkers] = useState<WorkerStatus[]>([])
  const [tasks, setTasks] = useState<TaskStatus[]>([])
  const [presets, setPresets] = useState<WorkerPreset[]>([])
  const [editWorker, setEditWorker] = useState<Worker | null>(null)
  const [editTask, setEditTask] = useState<ScheduledTask | null>(null)
  const [procfile, setProcfile] = useState<ProcfilePreview | null>(null)
  const [procfileOpen, setProcfileOpen] = useState(false)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const [w, t] = await Promise.all([runCommand({ type: 'list_workers', project_id: projectId }), runCommand({ type: 'list_schedules', project_id: projectId })])
    if (w.type === 'workers') setWorkers(w.workers)
    if (t.type === 'schedules') setTasks(t.tasks)
  }, [projectId])
  usePoll(load, 3000)
  useEffect(() => {
    runCommand({ type: 'list_worker_presets' }).then((r) => r.type === 'worker_presets' && setPresets(r.presets))
  }, [])
  useEffect(() => {
    runCommand({ type: 'import_procfile', project_id: projectId, dry_run: true }).then((r) => r.type === 'procfile' && setProcfile(r.preview)).catch(() => {})
  }, [projectId])

  const act = (key: string, fn: () => Promise<unknown>) => run(key, async () => {
    await fn()
    await load()
  })

  const newWorker = (): Worker => ({ id: '', project_id: projectId, name: 'queue', command: presets[0]?.command ?? '', count: 1, timeout_secs: null, memory_mb: null, max_retries: 5, restart: true, autostart: true })

  return (
    <div className="flex flex-col gap-5">
      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <section className="flex flex-col gap-2">
        {procfile && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2">
            <p className="text-sm"><span className="font-medium">{procfile.source} found</span><span className="text-muted-foreground"> · Import its processes into OLS</span></p>
            <Button size="sm" variant="secondary" onClick={() => setProcfileOpen(true)}>Preview import</Button>
          </div>
        )}
        <div className="flex items-center justify-between gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            <Cog className="size-4" /> Queue workers
          </h3>
          <div className="flex gap-2">
            {workers.length > 0 && (
              <>
                <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => act('start-all', () => runCommand({ type: 'start_project_workers', project_id: projectId }))}>
                  <Play /> Start all
                </Button>
                <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => act('stop-all', () => runCommand({ type: 'stop_project_workers', project_id: projectId }))}>
                  <StopIcon /> Stop all
                </Button>
              </>
            )}
            <Button size="sm" variant="secondary" onClick={() => setEditWorker(newWorker())}>
              <Plus /> Add worker
            </Button>
          </div>
        </div>
        {workers.length === 0 && <p className="text-sm text-muted-foreground">No workers. Add one to process queued jobs (Laravel queue, Symfony Messenger, Celery, BullMQ).</p>}
        {workers.map(({ worker: w, running, command_line }) => (
          <div key={w.id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
            <div className="min-w-0">
              <p className="flex items-center gap-2 text-sm font-medium">
                {w.name}
                {running > 0 ? <Badge variant="success">{running}/{w.count} running</Badge> : <Badge variant="secondary">stopped</Badge>}
                {!w.autostart && <Badge variant="outline">manual</Badge>}
              </p>
              <p className="truncate font-mono text-xs text-muted-foreground" title={command_line}>
                {command_line}
              </p>
            </div>
            <div className="flex gap-1">
              {running > 0 ? (
                <>
                  <Button size="icon" variant="ghost" className="size-8" title="Restart" disabled={busy !== null} onClick={() => act(`r:${w.id}`, () => runCommand({ type: 'restart_worker', id: w.id }))}>
                    {busy === `r:${w.id}` ? <Spinner /> : <RotateCw />}
                  </Button>
                  <Button size="icon" variant="ghost" className="size-8" title="Stop" disabled={busy !== null} onClick={() => act(`s:${w.id}`, () => runCommand({ type: 'stop_worker', id: w.id }))}>
                    <StopIcon />
                  </Button>
                </>
              ) : (
                <Button size="icon" variant="ghost" className="size-8" title="Start" disabled={busy !== null} onClick={() => act(`g:${w.id}`, () => runCommand({ type: 'start_worker', id: w.id }))}>
                  {busy === `g:${w.id}` ? <Spinner /> : <Play />}
                </Button>
              )}
              <Button size="icon" variant="ghost" className="size-8" title="Edit" onClick={() => setEditWorker(w)}>
                <Pencil />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="size-8"
                title="Delete"
                onClick={async () => {
                  if (await confirmAction(`Delete the worker "${w.name}"? It is stopped first.`)) await act(`d:${w.id}`, () => runCommand({ type: 'remove_worker', id: w.id }))
                }}
              >
                <Trash2 />
              </Button>
            </div>
          </div>
        ))}
      </section>

      {procfileOpen && procfile && (
        <Dialog open={procfileOpen} onClose={() => setProcfileOpen(false)} title={`Import ${procfile.source}`} description="Review the processes before adding them to this project.">
          <div className="flex flex-col gap-3 text-sm">
            {procfile.web && <p><span className="font-medium">Web · port {procfile.web_port}</span><span className="block font-mono text-xs text-muted-foreground">{[procfile.web.executable, ...procfile.web.args].join(' ')}</span></p>}
            {procfile.workers.map((w) => <p key={w.id}><span className="font-medium">{w.name}</span><span className="block font-mono text-xs text-muted-foreground">{w.command}</span></p>)}
            {procfile.warnings.map((warning) => <p key={warning} className="text-amber-700 dark:text-amber-300">{warning}</p>)}
            <div className="flex justify-end gap-2"><Button variant="ghost" onClick={() => setProcfileOpen(false)}>Cancel</Button><Button disabled={busy !== null} onClick={() => run('procfile', async () => { await runCommand({ type: 'import_procfile', project_id: projectId, dry_run: false }); setProcfileOpen(false); setProcfile(null); await load() })}>Import processes</Button></div>
          </div>
        </Dialog>
      )}

      <section className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            <CalendarClock className="size-4" /> Scheduled tasks
          </h3>
          <Button size="sm" variant="secondary" onClick={() => setEditTask({ id: '', project_id: projectId, name: '', schedule: 'every_minute', command: '', enabled: true })}>
            <Plus /> Add task
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">Tasks run while OLS is open. A run that is still going is never started twice.</p>
        {tasks.map((t) => (
          <div key={t.task.id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
            <div className="min-w-0">
              <p className="flex items-center gap-2 text-sm font-medium">
                {t.task.name}
                <Badge variant="secondary">{t.description}</Badge>
                {t.running && <Badge variant="success">running</Badge>}
                {!t.task.enabled && <Badge variant="outline">paused</Badge>}
              </p>
              <p className="truncate font-mono text-xs text-muted-foreground">{t.task.command}</p>
              <p className="text-xs text-muted-foreground">
                {t.next_run_ms && t.task.enabled ? `Next ${inWords(t.next_run_ms)}` : 'Not scheduled'}
                {t.last_run &&
                  ` · last ${timeAgo(t.last_run.started_ms)}${t.last_run.skipped ? ' (skipped: still running)' : t.last_run.error ? `: ${t.last_run.error}` : t.last_run.exit_code !== null ? ` (exit ${t.last_run.exit_code})` : ''}`}
              </p>
            </div>
            <div className="flex items-center gap-1">
              <Toggle
                checked={t.task.enabled}
                onChange={(v) => act(`e:${t.task.id}`, () => runCommand({ type: 'save_schedule', task: { ...t.task, enabled: v } }))}
                label="On"
              />
              <Button size="icon" variant="ghost" className="size-8" title="Run now" disabled={busy !== null} onClick={() => act(`n:${t.task.id}`, () => runCommand({ type: 'run_schedule_now', id: t.task.id }))}>
                <Play />
              </Button>
              <Button size="icon" variant="ghost" className="size-8" title="Edit" onClick={() => setEditTask(t.task)}>
                <Pencil />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="size-8"
                title="Delete"
                onClick={async () => {
                  if (await confirmAction(`Delete the scheduled task "${t.task.name}"?`)) await act(`x:${t.task.id}`, () => runCommand({ type: 'remove_schedule', id: t.task.id }))
                }}
              >
                <Trash2 />
              </Button>
            </div>
          </div>
        ))}
      </section>

      {editWorker && <WorkerEditor initial={editWorker} presets={presets} onClose={() => setEditWorker(null)} onSaved={() => { setEditWorker(null); void load() }} />}
      {editTask && <TaskEditor initial={editTask} onClose={() => setEditTask(null)} onSaved={() => { setEditTask(null); void load() }} />}
    </div>
  )
}

function num(v: string): number | null {
  const n = parseInt(v, 10)
  return Number.isFinite(n) && n > 0 ? n : null
}

function WorkerEditor({ initial, presets, onClose, onSaved }: { initial: Worker; presets: WorkerPreset[]; onClose: () => void; onSaved: () => void }) {
  const [w, setW] = useState(initial)
  const { busy, error, setError, run } = useAction()
  const set = (p: Partial<Worker>) => setW({ ...w, ...p })
  return (
    <Dialog
      open
      onClose={onClose}
      title={initial.id ? `Worker · ${initial.name}` : 'Add a worker'}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={busy !== null || !w.command.trim()}
            onClick={() =>
              run('save', async () => {
                await runCommand({ type: 'save_worker', worker: w })
                onSaved()
              })
            }
          >
            {busy ? <Spinner /> : null} Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {!initial.id && (
          <Field label="Kind">
            <Select onChange={(e) => set({ command: presets.find((p) => p.id === e.target.value)?.command ?? w.command })} defaultValue={presets[0]?.id}>
              {presets.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </Select>
          </Field>
        )}
        <div className="grid gap-4 sm:grid-cols-[10rem_minmax(0,1fr)]">
          <Field label="Name">
            <Input value={w.name} onChange={(e) => set({ name: e.target.value })} disabled={!!initial.id} />
          </Field>
          <Field label="Command" hint="Runs in the project folder with its PHP / Node / Python.">
            <Input value={w.command} onChange={(e) => set({ command: e.target.value })} className="font-mono" />
          </Field>
        </div>
        <div className="grid gap-4 sm:grid-cols-4">
          <Field label="Copies">
            <Input type="number" min={1} max={16} value={w.count} onChange={(e) => set({ count: num(e.target.value) ?? 1 })} />
          </Field>
          <Field label="Job timeout (s)">
            <Input type="number" min={1} value={w.timeout_secs ?? ''} onChange={(e) => set({ timeout_secs: num(e.target.value) })} placeholder="default" />
          </Field>
          <Field label="Memory (MB)">
            <Input type="number" min={16} value={w.memory_mb ?? ''} onChange={(e) => set({ memory_mb: num(e.target.value) })} placeholder="default" />
          </Field>
          <Field label="Retries">
            <Input type="number" min={0} value={w.max_retries} onChange={(e) => set({ max_retries: Math.max(0, parseInt(e.target.value, 10) || 0) })} />
          </Field>
        </div>
        <Toggle checked={w.restart} onChange={(v) => set({ restart: v })} label="Restart after a crash" hint="Up to the number of retries, three seconds apart." />
        <Toggle checked={w.autostart} onChange={(v) => set({ autostart: v })} label="Start with the project" hint="With setup, Start all and modes that turn workers on." />
      </div>
    </Dialog>
  )
}

function TaskEditor({ initial, onClose, onSaved }: { initial: ScheduledTask; onClose: () => void; onSaved: () => void }) {
  const [t, setT] = useState(initial)
  const [custom, setCustom] = useState(!SCHEDULES.some(([id]) => id === initial.schedule))
  const [meaning, setMeaning] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const set = (p: Partial<ScheduledTask>) => setT({ ...t, ...p })

  useEffect(() => {
    const id = setTimeout(() => {
      runCommand({ type: 'describe_schedule', schedule: t.schedule })
        .then((r) => r.type === 'text' && setMeaning(r.text))
        .catch((e) => setMeaning(`⚠ ${e.cause ?? e}`))
    }, 250)
    return () => clearTimeout(id)
  }, [t.schedule])

  return (
    <Dialog
      open
      onClose={onClose}
      title={initial.id ? `Scheduled task · ${initial.name}` : 'Add a scheduled task'}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={busy !== null || !t.command.trim() || !t.name.trim()}
            onClick={() =>
              run('save', async () => {
                await runCommand({ type: 'save_schedule', task: t })
                onSaved()
              })
            }
          >
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="Name">
          <Input value={t.name} onChange={(e) => set({ name: e.target.value })} disabled={!!initial.id} placeholder="Laravel scheduler" />
        </Field>
        <Field label="Command">
          <Input value={t.command} onChange={(e) => set({ command: e.target.value })} className="font-mono" placeholder="php artisan schedule:run" />
        </Field>
        <Field label="When" hint={meaning ?? undefined}>
          <div className="flex flex-wrap gap-2">
            <Select
              value={custom ? '__custom' : t.schedule}
              onChange={(e) => {
                if (e.target.value === '__custom') {
                  setCustom(true)
                  set({ schedule: '*/10 * * * *' })
                } else {
                  setCustom(false)
                  set({ schedule: e.target.value })
                }
              }}
              className="w-52"
            >
              {SCHEDULES.map(([id, label]) => (
                <option key={id} value={id}>
                  {label}
                </option>
              ))}
              <option value="__custom">Custom cron…</option>
            </Select>
            {custom && <Input value={t.schedule} onChange={(e) => set({ schedule: e.target.value })} className="w-40 font-mono" placeholder="*/10 * * * *" aria-label="Cron expression" />}
          </div>
        </Field>
      </div>
    </Dialog>
  )
}
