import { listen } from '@tauri-apps/api/event'
import { CheckCircle2, CircleX, Square } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type TaskEvent, type TaskView, runCommand } from '@/core'

/**
 * §166: live output of one background task (a `docker compose` action). Follows
 * `task-event` only while open; the task keeps running if the dialog is closed and stays
 * visible on the Background tasks page.
 */
export function TaskOutputDialog({
  taskId,
  onClose,
  onFinished,
}: {
  taskId: string | null
  onClose: () => void
  onFinished?: (task: TaskView) => void
}) {
  const [task, setTask] = useState<TaskView | null>(null)
  const box = useRef<HTMLDivElement>(null)
  const finishedRef = useRef(onFinished)
  useEffect(() => {
    finishedRef.current = onFinished
  }, [onFinished])

  useEffect(() => {
    if (!taskId) return
    let alive = true
    let unlisten: (() => void) | undefined
    let reported = false
    const take = (t: TaskView) => {
      if (!alive || t.id !== taskId) return
      setTask(t)
      if (t.state !== 'running' && !reported) {
        reported = true
        finishedRef.current?.(t)
      }
    }
    void listen<TaskEvent>('task-event', (e) => take(e.payload.task)).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    // The task may have produced output (or finished) before the listener was ready.
    void runCommand({ type: 'list_tasks' })
      .then((r) => {
        if (r.type === 'tasks') {
          const t = r.tasks.find((x) => x.id === taskId)
          if (t) take(t)
        }
      })
      .catch(() => undefined)
    return () => {
      alive = false
      unlisten?.()
    }
  }, [taskId])

  // Keep the newest line in view.
  useEffect(() => {
    const el = box.current
    if (el) el.scrollTop = el.scrollHeight
  }, [task?.results.length])

  const running = !task || task.state === 'running'
  // Rendered on <body>: the Dashboard page is a CSS container, which traps `position: fixed`
  // children, so the dialog opened inside the page flow instead of over it.
  return createPortal(
    <Dialog
      open={!!taskId}
      onClose={onClose}
      size="wide"
      title={task?.title ?? 'Starting…'}
      description={task?.steps.at(-1)?.label}
      footer={
        <div className="flex w-full items-center justify-between gap-2">
          <span className="flex items-center gap-2 text-sm">
            {running ? (
              <>
                <Spinner className="size-4" /> Running…
              </>
            ) : task?.state === 'succeeded' ? (
              <>
                <CheckCircle2 className="size-4 text-emerald-500" /> Done
              </>
            ) : (
              <>
                <CircleX className="size-4 text-destructive" /> {task?.state === 'cancelled' ? 'Stopped' : 'Failed'}
              </>
            )}
          </span>
          <span className="flex gap-2">
            {running && task && (
              <Button
                size="sm"
                variant="outline"
                className="cursor-pointer"
                disabled={task.cancel_requested}
                onClick={() => void runCommand({ type: 'cancel_task', id: task.id })}
              >
                <Square className="size-3.5" /> Stop
              </Button>
            )}
            <Button size="sm" className="cursor-pointer" onClick={onClose}>
              {running ? 'Hide (keeps running)' : 'Close'}
            </Button>
          </span>
        </div>
      }
    >
      <div
        ref={box}
        className="h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all"
      >
        {task?.results.length ? task.results.join('\n') : running ? 'Waiting for output…' : 'No output.'}
      </div>
      {task?.error && (
        <div className="mt-3 rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm">
          <p className="font-medium">{task.error.problem}</p>
          <p className="mt-1 text-muted-foreground">{task.error.cause}</p>
          {task.error.fix && <p className="mt-1">{task.error.fix}</p>}
        </div>
      )}
    </Dialog>,
    document.body,
  )
}
