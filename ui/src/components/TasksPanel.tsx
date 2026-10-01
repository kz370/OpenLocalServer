import { AlertTriangle, CheckCircle2, CircleSlash, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type TaskEvent, type TaskView, runCommand } from '@/core'
import { formatBytes, timeAgo, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'

/**
 * §164: the background work the user started, newest first, with each task's own progress.
 *
 * The list is read once and then patched by `task-event`, which carries the whole task every
 * time, so there is no delta to reconcile. A poll backs it up: if the webview misses events
 * (it was reloading, or it fell behind the broadcast), the list catches up within a second
 * rather than sitting on a stale row.
 */
export function TasksPanel() {
  const [tasks, setTasks] = useState<TaskView[]>([])

  const refresh = useCallback(async () => {
    const r = await runCommand({ type: 'list_tasks' })
    if (r.type === 'tasks') setTasks(r.tasks)
  }, [])

  useEffect(() => {
    void refresh().catch(() => undefined)
    let alive = true
    let unlisten: (() => void) | undefined
    void listen<TaskEvent>('task-event', (event) => {
      if (!alive) return
      const task = event.payload.task
      setTasks((current) => {
        const next = current.filter((t) => t.id !== task.id)
        next.unshift(task)
        return next
      })
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    return () => {
      alive = false
      unlisten?.()
    }
  }, [refresh])

  usePoll(() => void refresh().catch(() => undefined), 1000)

  const running = tasks.filter((t) => t.state === 'running')
  const finished = tasks.filter((t) => t.state !== 'running')

  return (
    <Card>
        <CardHeader>
          <CardTitle className="text-sm">
            Background tasks · {running.length} running, {finished.length} finished
          </CardTitle>
          <CardDescription>
            Imports, exports and snapshots the app runs for you. Each row shows its own steps; a finished task keeps its
            report until you clear it.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {tasks.length === 0 ? (
            <p className="text-sm text-muted-foreground">No background work yet.</p>
          ) : (
            tasks.map((t) => <TaskRow key={t.id} task={t} />)
          )}
          {finished.length > 0 && (
            <div className="flex justify-end">
              <Button
                size="sm"
                variant="ghost"
                className="h-7 text-xs"
                onClick={() => void runCommand({ type: 'clear_finished_tasks' }).then(() => refresh())}
              >
                Clear finished
              </Button>
            </div>
          )}
        </CardContent>
    </Card>
  )
}

const STATE_BADGE: Record<TaskView['state'], { label: string; variant: 'default' | 'success' | 'warning' | 'destructive' | 'secondary' }> = {
  running: { label: 'Running', variant: 'default' },
  succeeded: { label: 'Done', variant: 'success' },
  failed: { label: 'Failed', variant: 'destructive' },
  // A stop the user asked for is never dressed up as a failure.
  cancelled: { label: 'Stopped', variant: 'warning' },
}

const STEP_MARK: Record<string, string> = {
  running: 'text-ring',
  done: 'text-success',
  failed: 'text-destructive',
  skipped: 'text-muted-foreground',
}

function TaskRow({ task }: { task: TaskView }) {
  const [busy, setBusy] = useState(false)
  const live = task.state === 'running'
  const { done, total, bytes, bytes_total } = task.progress
  const fraction = total > 0 ? Math.min(1, done / total) : null

  const act = async (command: 'cancel_task' | 'dismiss_task') => {
    setBusy(true)
    try {
      await runCommand({ type: command, id: task.id })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-border p-3">
      <div className="flex flex-wrap items-center gap-2">
        {live ? <Spinner className="size-3.5" /> : <StateIcon state={task.state} />}
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={task.title}>
          {task.title}
        </span>
        <Badge variant={STATE_BADGE[task.state].variant}>{STATE_BADGE[task.state].label}</Badge>
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          {timeAgo(task.finished_ms ?? task.started_ms)}
        </span>
        {live ? (
          <Button size="sm" variant="outline" className="h-7 text-xs" disabled={busy} onClick={() => void act('cancel_task')}>
            Stop
          </Button>
        ) : (
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            aria-label={`Remove ${task.title} from the list`}
            title="Remove from the list"
            disabled={busy}
            onClick={() => void act('dismiss_task')}
          >
            <X className="size-3.5" />
          </Button>
        )}
      </div>

      {task.target && (
        <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground" title={task.target}>
          {task.target}
        </p>
      )}

      {/* A bar with no total is indeterminate, not full: a task that has not said how much
          work it has must not look finished. */}
      {live && (fraction !== null || bytes_total !== null) && (
        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}>
          <div
            className={cn('h-full rounded-full transition-[width] duration-200', fraction === null ? 'w-1/3 animate-pulse bg-ring' : 'bg-ring')}
            style={fraction === null ? undefined : { width: `${Math.max(2, fraction * 100)}%` }}
          />
        </div>
      )}
      {live && (
        <p className="mt-1 text-xs text-muted-foreground">
          {total > 0 ? `${done} of ${total} done` : 'working…'}
          {bytes > 0 && ` · ${formatBytes(bytes)}${bytes_total ? ` of ${formatBytes(bytes_total)}` : ''}`}
          {task.cancel_requested && ' · stopping after this item'}
        </p>
      )}

      {task.steps.length > 0 && (
        <ul className="mt-2 flex flex-col gap-0.5">
          {task.steps.map((s, i) => (
            <li key={`${s.label}-${i}`} className="flex items-start gap-1.5 text-xs">
              <span className={cn('mt-[3px] size-1.5 shrink-0 rounded-full bg-current', STEP_MARK[s.state])} />
              <span className={cn(s.state === 'running' ? 'text-foreground' : 'text-muted-foreground')}>
                {s.label}
                {s.detail && <span className="text-muted-foreground"> — {s.detail}</span>}
              </span>
            </li>
          ))}
        </ul>
      )}

      {task.error && (
        <div className="mt-2 rounded-md border border-destructive/40 bg-destructive/5 p-2 text-xs">
          <p className="font-medium text-destructive">{task.error.problem}</p>
          <p className="text-muted-foreground">{task.error.cause}</p>
          {task.error.fix && <p className="text-success">{task.error.fix}</p>}
        </div>
      )}

      {task.problems.length > 0 && (
        <ul className="mt-2 flex flex-col gap-0.5 text-xs text-warning">
          {task.problems.map((p, i) => (
            <li key={i}>⚠ {p}</li>
          ))}
        </ul>
      )}

      {task.results.length > 0 && (
        <ul className="mt-2 flex flex-col gap-0.5 text-xs text-muted-foreground">
          {task.results.map((r, i) => (
            <li key={i} className="truncate" title={r}>
              ✓ {r}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function StateIcon({ state }: { state: TaskView['state'] }) {
  if (state === 'succeeded') return <CheckCircle2 className="size-3.5 shrink-0 text-success" />
  if (state === 'failed') return <AlertTriangle className="size-3.5 shrink-0 text-destructive" />
  return <CircleSlash className="size-3.5 shrink-0 text-warning" />
}
