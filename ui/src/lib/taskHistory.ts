import type { CoreCommand } from '@/core/commands'
import type { Diagnostic } from '@/core/types/system'
import type { TaskView } from '@/core/types/tasks'
import { createPersistentCache } from '@/lib/cache'

const history = createPersistentCache<TaskView[]>('tasks_list', [])
const taskTargets = new Map<string, string>()
let sequence = 0
let recovered = false

export type HistoryTask = TaskView & { repeat_count?: number; repeat_ids?: string[] }

export function getTaskHistory(): HistoryTask[] {
  recoverInterruptedTasks()
  return history.get() as HistoryTask[]
}

export function saveTaskHistory(tasks: TaskView[]): void {
  history.set(tasks)
}

export function registerTaskTarget(id: string, label: string): void {
  taskTargets.set(id, label)
}

export function commandTaskKey(command: CoreCommand): string | null {
  return shouldTrackCommand(command) ? `${command.type}:${commandTarget(command)}` : null
}

export function mergeTaskHistory(tasks: TaskView[]): HistoryTask[] {
  recoverInterruptedTasks()
  const merged = new Map<string, HistoryTask>()
  for (const task of history.get() as HistoryTask[]) merged.set(task.id, task)
  for (const task of tasks) {
    const previous = merged.get(task.id)
    merged.set(
      task.id,
      previous?.repeat_count
        ? { ...task, repeat_count: previous.repeat_count, repeat_ids: previous.repeat_ids }
        : task as HistoryTask,
    )
  }
  const next = collapseFinished([...merged.values()].sort((a, b) => b.started_ms - a.started_ms))
  history.set(next)
  return next
}

export function shouldTrackCommand(command: CoreCommand): boolean {
  const { type } = command

  // These commands only read state, report live progress, or manage this history.
  if (
    type === 'ping' ||
    type === 'check_port' ||
    type === 'list_tasks' ||
    type === 'cancel_task' ||
    type === 'dismiss_task' ||
    type === 'clear_finished_tasks' ||
    type === 'take_pending_add_project' ||
    type === 'global_search' ||
    type === 'doctor' ||
    type === 'run_diagnostics' ||
    type === 'git_status' ||
    type === 'git_branches' ||
    type === 'git_diff' ||
    type === 'git_log' ||
    type === 'git_show' ||
    type === 'list_processes' ||
    type === 'list_command_history' ||
    type === 'list_history' ||
    type === 'list_database_tables' ||
    type === 'list_php_extensions' ||
    type === 'list_pecl_packages' ||
    type === 'list_foreign_databases' ||
    type === 'list_migration_sources' ||
    type === 'list_tunnel_providers' ||
    type === 'get_process_output' ||
    type === 'get_domain' ||
    type.startsWith('list_') ||
    type.startsWith('get_') ||
    type.startsWith('read_') ||
    type.startsWith('inspect_') ||
    type.startsWith('check_') ||
    type.startsWith('preview_') ||
    type.startsWith('compare_') ||
    type.startsWith('validate_') ||
    type.startsWith('detect_') ||
    type.startsWith('suggest_') ||
    type.startsWith('describe_') ||
    type.startsWith('load_') ||
    type.startsWith('ai_get_') ||
    type.startsWith('ai_models') ||
    type.startsWith('ai_probe') ||
    type.startsWith('ai_preview')
  ) {
    return false
  }

  // Keep the requested runtime and service lifecycle actions out of task history.
  if (type === 'start_service' || type === 'restart_service' || type === 'apply_web') return false

  return true
}

export function beginCommandTask(command: CoreCommand): string | null {
  if (!shouldTrackCommand(command)) return null
  recoverInterruptedTasks()

  const now = Date.now()
  const id = `command-${now}-${sequence++}`
  const task: TaskView = {
    id,
    kind: command.type,
    title: commandTitle(command),
    target: commandTarget(command),
    state: 'running',
    steps: [],
    progress: { done: 0, total: 0, bytes: 0, bytes_total: null },
    results: [],
    problems: [],
    error: null,
    started_ms: now,
    finished_ms: null,
    cancel_requested: false,
  }
  history.set([task, ...history.get()])
  return id
}

export function finishCommandTask(id: string | null, error?: Diagnostic): void {
  if (!id) return
  const finished = Date.now()
  history.set(
    collapseFinished(history.get().map((task) =>
      task.id === id
        ? {
            ...task,
            state: error ? 'failed' : 'succeeded',
            finished_ms: finished,
            error: error ?? null,
            problems: error ? [error.problem] : [],
          }
        : task,
    )),
  )
}

export function discardCommandTask(id: string | null): void {
  if (!id) return
  history.set(history.get().filter((task) => task.id !== id))
}

function commandTitle(command: CoreCommand): string {
  const titles: Partial<Record<CoreCommand['type'], string>> = {
    delete_site: 'Delete site',
    bulk_delete_sites: 'Delete sites',
    wp_core_repair: 'Repair WordPress site',
    plan_repair: 'Plan site repair',
    apply_repair: 'Repair site',
    export_sites: 'Export sites',
    import_sites: 'Import sites',
    create_snapshot: 'Create snapshot',
    restore_snapshot: 'Restore snapshot',
  }
  return titles[command.type] ?? command.type.replaceAll('_', ' ').replace(/^\w/, (char) => char.toUpperCase())
}

function commandTarget(command: CoreCommand): string {
  const value = command as CoreCommand & Record<string, unknown>
  if (typeof value.project_id === 'string' && taskTargets.has(value.project_id)) {
    return taskTargets.get(value.project_id)!
  }
  const direct = value.hostname ?? value.project_id ?? value.projectId ?? value.target ?? value.path ?? value.id
  if (typeof direct === 'string') return direct
  if (command.type === 'bulk_delete_sites' || command.type === 'bulk_remove_domains') {
    const hostnames = (value.hostnames as string[] | undefined) ?? []
    return hostnames.join(', ')
  }
  return ''
}

function recoverInterruptedTasks(): void {
  if (recovered) return
  recovered = true
  const now = Date.now()
  history.set(
    history.get().map((task) =>
      task.state === 'running'
        ? {
            ...task,
            state: 'failed',
            finished_ms: now,
            error: {
              problem: 'The task was interrupted.',
              cause: 'OpenLocalServer closed before this task finished.',
              fix: 'Check the site state, then run the operation again if it is still needed.',
            },
            problems: ['The operation was interrupted when OpenLocalServer closed.'],
          }
        : task,
    ),
  )
}

function collapseFinished(tasks: TaskView[]): HistoryTask[] {
  const collapsed: HistoryTask[] = []
  for (const task of tasks as HistoryTask[]) {
    const previous = collapsed.at(-1)
    if (
      previous &&
      previous.state !== 'running' &&
      task.state !== 'running' &&
      previous.kind === task.kind &&
      previous.title === task.title &&
      previous.target === task.target
    ) {
      previous.repeat_count = (previous.repeat_count ?? 1) + (task.repeat_count ?? 1)
      previous.repeat_ids = [...(previous.repeat_ids ?? [previous.id]), ...(task.repeat_ids ?? [task.id])]
      continue
    }
    collapsed.push({
      ...task,
      repeat_count: task.repeat_count ?? 1,
      repeat_ids: task.repeat_ids ?? [task.id],
    })
  }
  return collapsed
}
