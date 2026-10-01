import { TasksPanel } from '@/components/TasksPanel'

/**
 * §164: background work has a page of its own.
 *
 * It used to be a tab on Processes, which put it below the system monitor and the process
 * list — so starting an import and being sent to watch it landed the user on a page where the
 * thing they came for was below the fold, and scrolling to it was a workaround for a layout
 * rather than a fix. A page that holds the task list and nothing else cannot have the task
 * list off the screen.
 */
export function TasksPage() {
  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Background tasks</h1>
        <p className="text-sm text-muted-foreground">
          Imports, exports and snapshots the app runs for you. Each one reports its own steps and progress, and a finished
          task keeps its report until you clear it.
        </p>
      </div>

      <TasksPanel />
    </div>
  )
}