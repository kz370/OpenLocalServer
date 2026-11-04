import { AlertTriangle, CheckCircle2, CircleSlash, X } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Pagination } from '@/components/ui/pagination'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type TaskEvent, type TaskView, runCommand } from '@/core'
import { formatBytes, timeAgo, usePoll } from '@/lib/hooks'
import { getTaskHistory, mergeTaskHistory, saveTaskHistory, type HistoryTask } from '@/lib/taskHistory'
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
  const [tasks, setTasks] = useState<TaskView[]>(getTaskHistory)
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(10)

  const refresh = useCallback(async () => {
    const r = await runCommand({ type: 'list_tasks' })
    if (r.type === 'tasks') {
      setTasks(mergeTaskHistory(r.tasks))
    }
  }, [])

  useEffect(() => {
    void refresh().catch(() => undefined)
    let alive = true
    let unlisten: (() => void) | undefined
    void listen<TaskEvent>('task-event', (event) => {
      if (!alive) return
      const task = event.payload.task
      setTasks((current) => {
        const next = mergeTaskHistory([...current.filter((t) => t.id !== task.id), task])
        saveTaskHistory(next)
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
  const pageCount = Math.max(1, Math.ceil(tasks.length / perPage))
  const currentPage = Math.min(page, pageCount)
  const pageTasks = useMemo(() => {
    const start = (currentPage - 1) * perPage
    return tasks.slice(start, start + perPage)
  }, [currentPage, perPage, tasks])

  return (
    <Card>
        <CardHeader className="flex-row items-start justify-between gap-4">
          <div className="space-y-1.5">
          <CardTitle className="text-sm">
            Background tasks · {running.length} running, {finished.length} finished
          </CardTitle>
          <CardDescription>
            Site operations and other background work, newest first. Finished tasks stay here after restart until cleared.
          </CardDescription>
          </div>
          {finished.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              className="h-7 shrink-0 text-xs"
              onClick={() =>
                void runCommand({ type: 'clear_finished_tasks' }).then(() => {
                  setTasks((current) => {
                    const next = current.filter((task) => task.state === 'running')
                    saveTaskHistory(next)
                    return next
                  })
                  setPage(1)
                  return refresh()
                })
              }
            >
              Clear finished
            </Button>
          )}
        </CardHeader>
        <CardContent className="p-0">
          {tasks.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">No background work yet.</p>
          ) : (
            <>
              <Table className="min-w-[860px]">
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="w-[22%]">Task</TableHead>
                    <TableHead className="w-[18%]">Site / target</TableHead>
                    <TableHead>Status and details</TableHead>
                    <TableHead className="w-48 whitespace-nowrap">Updated</TableHead>
                    <TableHead className="w-24 text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pageTasks.map((task) => <TaskRow key={task.id} task={task} />)}
                </TableBody>
              </Table>
              <Pagination
                label="tasks"
                total={tasks.length}
                page={currentPage}
                perPage={perPage}
                onPage={setPage}
                onPerPage={(size) => {
                  setPerPage(size)
                  setPage(1)
                }}
                className="border-t border-border"
              />
            </>
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

function TaskRow({ task }: { task: HistoryTask }) {
  const [busy, setBusy] = useState(false)
  const live = task.state === 'running'
  const cancellable = live && task.id.startsWith('task-')
  const { done, total, bytes, bytes_total } = task.progress
  const fraction = total > 0 ? Math.min(1, done / total) : null

  const act = async (command: 'cancel_task' | 'dismiss_task') => {
    setBusy(true)
    try {
      const ids = command === 'dismiss_task' ? (task.repeat_ids ?? [task.id]) : [task.id]
      await Promise.all(
        ids
          .filter((id) => id.startsWith('task-'))
          .map((id) => runCommand({ type: command, id })),
      )
      if (command === 'dismiss_task') {
        // TaskManager cannot dismiss synthetic command-history rows; remove either kind here.
        const next = getTaskHistory().filter((item) => item.id !== task.id)
        saveTaskHistory(next)
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <TableRow>
      <TableCell className="align-top">
        <div className="flex items-start gap-2">
          {live ? <Spinner className="mt-0.5 size-3.5 shrink-0" /> : <StateIcon state={task.state} />}
          <span className="min-w-0 truncate text-sm font-medium" title={task.title}>
            {task.title}{(task.repeat_count ?? 1) > 1 && ` ×${task.repeat_count}`}
          </span>
        </div>
      </TableCell>
      <TableCell className="align-top">
        <span className="block truncate font-mono text-xs text-muted-foreground" title={task.target || 'No target'}>
          {task.target || '—'}
        </span>
      </TableCell>
      <TableCell className="align-top">
        <div className="flex flex-col items-start gap-1.5">
          <Badge variant={STATE_BADGE[task.state].variant}>{STATE_BADGE[task.state].label}</Badge>

          {/* A bar with no total is indeterminate, not full: a task that has not said how much
              work it has must not look finished. */}
          {live && (fraction !== null || bytes_total !== null) && (
            <div className="h-1.5 w-full max-w-64 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}>
              <div
                className={cn('h-full rounded-full transition-[width] duration-200', fraction === null ? 'w-1/3 animate-pulse bg-ring' : 'bg-ring')}
                style={fraction === null ? undefined : { width: `${Math.max(2, fraction * 100)}%` }}
              />
            </div>
          )}
          {live && (
            <p className="text-xs text-muted-foreground">
              {total > 0 ? `${done} of ${total} done` : 'working…'}
              {bytes > 0 && ` · ${formatBytes(bytes)}${bytes_total ? ` of ${formatBytes(bytes_total)}` : ''}`}
              {task.cancel_requested && ' · stopping after this item'}
            </p>
          )}
          {task.steps.length > 0 && (
            <ul className="flex flex-col gap-0.5">
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
            <div className="rounded-md border border-destructive/40 bg-destructive/5 p-2 text-xs">
              <p className="font-medium text-destructive">{task.error.problem}</p>
              <p className="text-muted-foreground">{task.error.cause}</p>
              {task.error.fix && <p className="text-success">{task.error.fix}</p>}
            </div>
          )}
          {task.problems.length > 0 && (
            <ul className="flex flex-col gap-0.5 text-xs text-warning">
              {task.problems.map((p, i) => <li key={i}>⚠ {p}</li>)}
            </ul>
          )}
          {task.results.length > 0 && (
            <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
              {task.results.map((r, i) => <li key={i} className="truncate" title={r}>✓ {r}</li>)}
            </ul>
          )}
        </div>
      </TableCell>
      <TableCell className="align-top">
        <time
          className="whitespace-nowrap text-xs text-muted-foreground"
          dateTime={new Date(task.finished_ms ?? task.started_ms).toISOString()}
          title={timeAgo(task.finished_ms ?? task.started_ms)}
        >
          {new Date(task.finished_ms ?? task.started_ms).toLocaleString()}
        </time>
      </TableCell>
      <TableCell className="align-top text-right">
        {cancellable ? (
          <Button size="sm" variant="outline" className="h-7 text-xs" disabled={busy} onClick={() => void act('cancel_task')}>
            Stop
          </Button>
        ) : live ? null : (
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
      </TableCell>
    </TableRow>
  )
}

function StateIcon({ state }: { state: TaskView['state'] }) {
  if (state === 'succeeded') return <CheckCircle2 className="size-3.5 shrink-0 text-success" />
  if (state === 'failed') return <AlertTriangle className="size-3.5 shrink-0 text-destructive" />
  return <CircleSlash className="size-3.5 shrink-0 text-warning" />
}
