import type { CoreCommand } from '@/core/commands'
import type { Diagnostic } from '@/core/types/system'
import type { TaskView } from '@/core/types/tasks'
import { createPersistentCache } from '@/lib/cache'

const history = createPersistentCache<TaskView[]>('tasks_list', [])
let sequence = 0

export function getTaskHistory(): TaskView[] {
  return history.get()
}

export function saveTaskHistory(tasks: TaskView[]): void {
  history.set(tasks)
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
    type.startsWith('doctor') ||
    type.startsWith('diagnose_') ||
    type.startsWith('load_') ||
    type.startsWith('ai_get_') ||
    type.startsWith('ai_models') ||
    type.startsWith('ai_probe') ||
    type.startsWith('ai_preview') ||
    type.startsWith('git_status') ||
    type.startsWith('git_branches') ||
    type.startsWith('git_diff') ||
    type.startsWith('git_log') ||
    type.startsWith('git_show') ||
    type.startsWith('list_') ||
    type === 'get_dashboard' ||
    type === 'get_environment_health' ||
    type === 'get_system_stats' ||
    type === 'get_resource_limits' ||
    type === 'get_composer_info' ||
    type === 'get_package_managers' ||
    type === 'get_venv' ||
    type === 'get_migration_progress' ||
    type === 'get_setup_progress' ||
    type === 'get_auto_backup' ||
    type === 'get_web_status' ||
    type === 'get_web_config' ||
    type === 'get_ca_info' ||
    type === 'get_xdebug' ||
    type === 'get_app_guide' ||
    type === 'global_search' ||
    type === 'get_api_status' ||
    type === 'get_updater_status' ||
    type === 'get_shell_menu' ||
    type === 'get_startup_settings' ||
    type === 'get_helper_service' ||
    type === 'get_manifest' ||
    type === 'get_project_modes' ||
    type === 'get_connection_info' ||
    type === 'get_quick_app' ||
    type === 'get_quick_run' ||
    type === 'get_findings' ||
    type === 'get_app_guide' ||
    type === 'get_service_aggregate' ||
    type === 'list_processes' ||
    type === 'list_command_history' ||
    type === 'list_history' ||
    type === 'list_database_tables' ||
    type === 'list_php_extensions' ||
    type === 'list_pecl_packages' ||
    type === 'list_foreign_databases' ||
    type === 'list_migration_sources' ||
    type === 'list_tunnel_providers' ||
    type === 'list_project_types' ||
    type === 'take_pending_add_project' ||
    type === 'get_process_output' ||
    type === 'get_domain' ||
    type === 'get_setup_progress' ||
    type === 'get_resource_limits' ||
    type === 'get_web_config' ||
    type === 'get_web_status' ||
    type === 'get_auto_backup' ||
    type === 'get_environment_health' ||
    type === 'get_dashboard' ||
    type === 'get_system_stats' ||
    type === 'get_setting' ||
    type === 'get_secret' ||
    type === 'get_updater_status' ||
    type === 'get_api_status' ||
    type === 'get_startup_settings' ||
    type === 'get_helper_service' ||
    type === 'get_ca_info' ||
    type === 'get_xdebug' ||
    type === 'get_manifest' ||
    type === 'get_project_detail' ||
    type === 'get_quick_app' ||
    type === 'get_quick_run' ||
    type === 'get_composer_info' ||
    type === 'get_package_managers' ||
    type === 'get_venv' ||
    type === 'get_migration_progress' ||
    type === 'get_connection_info' ||
    type === 'get_findings'
  ) {
    return false
  }

  // Keep the requested runtime and service lifecycle actions out of task history.
  if (type === 'start_service' || type === 'restart_service' || type === 'apply_web') return false

  return true
}

export function beginCommandTask(command: CoreCommand): string | null {
  if (!shouldTrackCommand(command)) return null

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
    history.get().map((task) =>
      task.id === id
        ? {
            ...task,
            state: error ? 'failed' : 'succeeded',
            finished_ms: finished,
            error: error ?? null,
            problems: error ? [error.problem] : [],
          }
        : task,
    ),
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
  const direct = value.hostname ?? value.project_id ?? value.projectId ?? value.target ?? value.path ?? value.id
  if (typeof direct === 'string') return direct
  if (command.type === 'bulk_delete_sites' || command.type === 'bulk_remove_domains') {
    const hostnames = (value.hostnames as string[] | undefined) ?? []
    return hostnames.join(', ')
  }
  return ''
}
