import type { EnvironmentManifest, ModeManifest, Project } from './base'
import type { Diagnostic } from './system'

export interface Profile {
  id: string
  name: string
  description: string
  environment: EnvironmentManifest
  builtin: boolean
}

export interface ModesView {
  current: string | null
  modes: { name: string; mode: ModeManifest; custom: boolean }[]
}

export interface ModeResult {
  mode: string
  changes: string[]
  problems: string[]
}

export interface Worker {
  id: string
  project_id: string
  name: string
  command: string
  count: number
  timeout_secs: number | null
  memory_mb: number | null
  max_retries: number
  restart: boolean
  autostart: boolean
}

export interface WorkerStatus {
  worker: Worker
  running: number
  processes: number[]
  command_line: string
}

export interface WorkerPreset {
  id: string
  label: string
  command: string
}

export interface ScheduledTask {
  id: string
  project_id: string | null
  name: string
  schedule: string
  command: string
  enabled: boolean
}

export interface TaskRun {
  started_ms: number
  process: number | null
  exit_code: number | null
  skipped: boolean
  error: string | null
}

export interface TaskStatus {
  task: ScheduledTask
  description: string
  next_run_ms: number | null
  last_run: TaskRun | null
  running: boolean
}

export interface SnapshotOptions {
  env: boolean
  databases: boolean
  files: boolean
}

export interface RestoreOptions {
  config: boolean
  env: boolean
  databases: boolean
  files: boolean
}

export interface SnapshotInfo {
  id: string
  project_id: string
  project_name: string
  label: string
  created_ms: number
  size_bytes: number
  options: SnapshotOptions
  path: string
  summary: string[]
}

export interface RestoreResult {
  safety_snapshot: string | null
  restored: string[]
  problems: string[]
}

export interface ImportPreview {
  source: string
  summary: string[]
  suggested_name: string
  adjustments: string[]
  conflicts: string[]
  content: { project: Project; label: string; created_ms: number; options: SnapshotOptions; file_count: number }
}

export interface CloneResult {
  project: Project
  changes: string[]
  problems: string[]
}

export interface SettingsBackup {
  id: string
  path: string
  created_ms: number
  size_bytes: number
}

/** §165: what a site bundle carries. Data is asked for, not assumed. */
export interface BundleOptions {
  /** The site record, project manifest, workers, scheduled tasks, tunnels and mode. */
  settings: boolean
  /** `.env` files — where the application's passwords and keys live. */
  env: boolean
  /** SQL dumps of the site's MariaDB / PostgreSQL database. */
  databases: boolean
  /** The project's own files, without node_modules, vendor and the like. */
  files: boolean
}

/** What to do about a site name that already exists. */
export type OnConflict = { type: 'update' } | { type: 'rename' }

export interface BundlePreviewSite {
  project_name: string
  hostnames: string[]
  /** Hostnames that already exist — the ones the update / rename choice is about. */
  existing: string[]
  /** A free hostname for each, which is what a rename import would use. */
  suggested: string[]
  summary: string[]
}

export interface BundlePreview {
  source: string
  label: string
  created_ms: number
  encrypted: boolean
  options: BundleOptions
  sites: BundlePreviewSite[]
  problems: string[]
}

export interface BundleSiteResult {
  project_name: string
  action: 'created' | 'updated' | 'skipped'
  hostnames: string[]
  /** The snapshot taken before an update, so it can be put back. */
  safety_snapshot: string | null
  changes: string[]
  problems: string[]
}

export interface BundleImportResult {
  sites: BundleSiteResult[]
  unlocked: boolean
}

/** §164: one background task's state, published whole on every change. */
export type TaskState = 'running' | 'succeeded' | 'failed' | 'cancelled'

export type StepState = 'running' | 'done' | 'failed' | 'skipped'

export interface TaskStep {
  label: string
  state: StepState
  detail: string | null
}

export interface TaskProgress {
  done: number
  /** 0 when the total is not known, which is an indeterminate bar rather than a full one. */
  total: number
  bytes: number
  bytes_total: number | null
}

export interface TaskView {
  id: string
  kind: string
  title: string
  target: string
  state: TaskState
  steps: TaskStep[]
  progress: TaskProgress
  results: string[]
  /** Collected, never raised: one bad database does not cost the rest of the export. */
  problems: string[]
  error: Diagnostic | null
  started_ms: number
  finished_ms: number | null
  cancel_requested: boolean
}

export type TaskEvent =
  | { kind: 'started'; task: TaskView }
  | { kind: 'progress'; task: TaskView }
  | { kind: 'finished'; task: TaskView }

/** Which projects an automatic backup covers: every project, or only the listed ones. */
export type AutoBackupScope = 'app' | 'site'

/** §130: what automatic backups take, how often, and how many they keep. */
export interface AutoBackupSettings {
  enabled: boolean
  scope: AutoBackupScope
  /** Cron expression, or a name the scheduler knows: `daily`, `hourly`, `weekly`, … */
  schedule: string
  keep: number
  snapshots: boolean
  include_env: boolean
  include_files: boolean
  databases: boolean
}

/** One site's own backup settings, as its Backups page leaves them. */
export interface AutoBackupSiteSettings {
  enabled: boolean
  /** This site's own period; null means it follows the app-wide plan. */
  schedule: string | null
  /** This site's own keep count; null means it follows the app-wide plan. */
  keep: number | null
  /** This site's own "copy the project's files" answer; null follows the plan. */
  include_files: boolean | null
  /** This site's own "copy the project's .env files" answer; null follows the plan. */
  include_env: boolean | null
  /** This site's own "dump the project's databases" answer; null follows the plan. */
  databases: boolean | null
}

/** One site's backup settings, and what they mean right now. */
export interface AutoBackupSite {
  hostname: string
  /** The project a snapshot would record, when the site belongs to one. */
  project_id: string | null
  project_name: string | null
  enabled: boolean
  schedule: string | null
  keep: number | null
  include_files: boolean | null
  include_env: boolean | null
  databases: boolean | null
  /** The period that runs now: the site's own, or the plan's. */
  effective_schedule: string
  effective_keep: number
  /** What actually gets copied for this site right now. */
  effective_include_files: boolean
  effective_include_env: boolean
  effective_databases: boolean
  /** The plan covers the whole app, so the site's own period, keep and contents are not in force. */
  managed_by_plan: boolean
}

/** What one automatic backup pass wrote, deleted and could not do. */
export interface AutoBackupRun {
  started_ms: number
  finished_ms: number
  created: string[]
  removed: string[]
  problems: string[]
}

export interface AutoBackupStatus {
  settings: AutoBackupSettings
  /** "daily at 00:00", "every 15 minutes", … */
  description: string
  next_run_ms: number | null
  running: boolean
  last_run: AutoBackupRun | null
  /** How many automatic backups are actually kept per project or database. */
  kept: Record<string, number>
  /** Every known site and whether it asked to be backed up. */
  sites: AutoBackupSite[]
}

export interface ResourceLimits {
  mariadb_buffer_pool_mb: number | null
  postgres_shared_buffers_mb: number | null
  redis_maxmemory_mb: number | null
  memcached_max_memory_mb: number | null
  mongodb_cache_mb: number | null
  /** Total CPU as a share of every core (1–100). Mutually exclusive with cpu_threads. */
  cpu_percent: number | null
  /** The same cap as a thread count, converted against this machine's logical cores. */
  cpu_threads: number | null
  /** An explicit cpulimit.exe path, for a build the search does not find. */
  cpu_limiter_path: string | null
  node_max_old_space_mb: number | null
  max_worker_count: number | null
  max_processes: number | null
  k6_max_vus: number | null
}

/** Whether a wanted CPU cap can actually be applied, so the UI never claims a limit that is not in force. */
export interface CpuCapStatus {
  wanted: boolean
  /** cpulimit.exe when found; null means a wanted cap cannot be applied. */
  limiter_path: string | null
  logical_cores: number
  /** The percentage the cap will be applied at, or null when unset. */
  effective_percent: number | null
  hint: string
}

// ---- Stage 14: tunnels and traffic ---------------------------------------------------

export interface TunnelConfig {
  id: string
  project_id: string | null
  name: string
  provider: string
  target: string
  auth_user: string | null
  allow_internal: boolean
  public_hostname: string | null
  acknowledged: boolean
  autostart?: boolean
}

export interface TunnelProvider {
  id: string
  name: string
  path: string | null
  install_hint: string
  uses_token: boolean
  token_saved: boolean
  note: string
}

export interface TunnelStatus {
  config: TunnelConfig
  state: 'stopped' | 'needs_confirmation' | 'starting' | 'connected' | 'failed'
  public_url: string | null
  started_ms: number | null
  requests: number
  last_request_ms: number | null
  latency_ms: number | null
  error: string | null
  inspector_port: number | null
  has_password: boolean
  exposure: string
}

