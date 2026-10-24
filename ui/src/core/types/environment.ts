import type { EnvironmentManifest } from './base'
import type { CoreCommand } from '../commands'
import type { HealthReport } from './web'

export interface MigrationSource {
  id: string
  label: string
  engine: string
  bin_dir: string
  data_dir: string
  size_bytes: number
  running_port: number | null
}

export interface MigrationProgress {
  running: boolean
  kind: 'scan' | 'import' | ''
  step: string
  db_index: number
  db_total: number
  current_db: string | null
  bytes: number
  bytes_total: number | null
  done: MigratedDb[]
  started_ms: number
}

export interface MigratedDb {
  name: string
  ok: boolean
  detail: string
}

export interface SystemStats {
  cpu_percent: number
  cpu_cores: number
  memory_used: number
  memory_total: number
  /** Drives holding OLS's data or your projects. */
  disks: { mount: string; used: number; total: number; holds: string[] }[]
  /** Keyed by the managed process's PID; includes its child processes. */
  processes: Record<number, { cpu_percent: number; memory: number; count: number }>
  sites: SiteUsage[]
}

export interface SiteUsage {
  hostname: string
  via: string
  cpu_percent: number
  memory: number
  shared_by: number
  measured: boolean
  /** Size of the site's folder; null until the first count finishes. */
  disk: number | null
}

export interface EditorInfo {
  id: string
  name: string
  path: string | null
}

export interface XdebugSettings {
  modes: string[]
  start_with_request: 'yes' | 'trigger' | 'default' | 'no'
  client_host: string
  client_port: number
  idekey: string
}

export interface XdebugReport {
  version: string
  installed: boolean
  enabled: boolean
  settings: XdebugSettings
}

export interface ComposerPackage {
  name: string
  constraint: string
  locked: string | null
  dev: boolean
}

export interface ComposerInfo {
  has_composer_json: boolean
  has_lock: boolean
  vendor_installed: boolean
  name: string | null
  packages: ComposerPackage[]
  scripts: string[]
}

export interface CommandArgument {
  name: string
  description: string
  required: boolean
  multiple: boolean
  default: string | null
}
export interface CommandOption {
  name: string
  shortcut: string | null
  description: string
  accepts_value: boolean
  value_required: boolean
  multiple: boolean
  default: string | null
}
export interface DiscoveredCommand {
  name: string
  description: string
  help: string
  arguments: CommandArgument[]
  options: CommandOption[]
}

export interface DetectedVenv {
  name: string
  path: string
  python_exe: string
  version: string | null
}

export interface SiteFolderInspection {
  framework: string
  package_scripts: DiscoveredCommand[]
  detected_package_manager: string | null
  detected_venv: string | null
  available_venvs: DetectedVenv[]
  requirements_files: string[]
  suggested_command: string | null
  suggested_port: number
  entry_files: string[]
}
export interface CommandSource {
  id: string
  label: string
  prefix: string[]
  commands: DiscoveredCommand[]
  error: string | null
  /** The tool failed; the list was read from source files instead. Says why. */
  warning: string | null
}

export interface PackageManagerInfo {
  detected: string | null
  detected_from: string | null
  pinned_version: string | null
  npm: boolean
  pnpm: boolean
  yarn: boolean
  corepack: boolean
}

export interface VenvInfo {
  exists: boolean
  dir_name: string | null
  python_version: string | null
  base_home: string | null
  base_missing: boolean
  requirements: string[]
  has_pyproject: boolean
  activate_command: string | null
}

export interface Finding {
  id: string
  severity: 'error' | 'warning' | 'info'
  problem: string
  cause: string
  fix: string
  /** Run this to apply the fix; null when only the user can. */
  fix_command: CoreCommand | null
  auto_fixable: boolean
  details: string[]
  ignored: boolean
}

export interface EnvEntry {
  key: string
  value: string
  line: number
  secret: boolean
}

export interface EnvIssue {
  line: number
  severity: 'error' | 'warning'
  message: string
}

export interface EnvFileView {
  name: string
  content: string
  entries: EnvEntry[]
  issues: EnvIssue[]
}

export interface EnvFileInfo {
  name: string
  size: number
  entries: number
}

export interface Operation {
  id: number
  kind: string
  title: string
  started_ms: number
  finished_ms: number | null
  status: 'running' | 'done' | 'failed' | 'interrupted'
  detail: string | null
  undo: string | null
}

export interface MailChange {
  key: string
  current: string | null
  new: string
  changed: boolean
}

export interface MailEnvPlan {
  file: string
  framework: string
  changes: MailChange[]
  up_to_date: boolean
  note: string | null
}

export interface MailCheck {
  id: string
  label: string
  ok: boolean
  detail: string
  fix: string | null
}

export interface EnvDiffRow {
  key: string
  a: string | null
  b: string | null
  status: 'same' | 'different' | 'only_a' | 'only_b'
  secret: boolean
}

export interface PhpExtensions {
  version: string
  thread_safe: boolean
  extensions: { name: string; enabled: boolean; downloaded: boolean }[]
}

// ---- Stage 12: manifests and setup ------------------------------------------------

export interface ManifestInfo {
  path: string
  found: boolean
  text: string | null
  manifest: EnvironmentManifest | null
  error: string | null
  derived: EnvironmentManifest
  lock: Record<string, string> | null
  has_commands: boolean
  has_services: boolean
}

export interface PlanStep {
  group: 'install' | 'create' | 'configure' | 'start' | 'tunnel' | 'check'
  label: string
  action: { kind: string }
  done: boolean
  note: string | null
}

export interface SetupConflict {
  kind: string
  blocking: boolean
  message: string
  resolution: string
}

export interface EnvironmentPlan {
  project_id: string
  project_name: string
  project_path: string
  manifest_found: boolean
  manifest: EnvironmentManifest
  lock_found: boolean
  steps: PlanStep[]
  conflicts: SetupConflict[]
  ok: boolean
}

export type SetupStepStatus = 'pending' | 'running' | 'done' | 'skipped' | 'failed' | 'rolled_back' | 'not_run'

export interface SetupReport {
  project_id: string
  running: boolean
  dry_run: boolean
  ok: boolean
  steps: { group: string; label: string; status: SetupStepStatus; detail: string | null }[]
  conflicts: SetupConflict[]
  rolled_back: string[]
  lock_written: string | null
  health: HealthReport | null
  error: string | null
}

// ---- Stage 13: profiles, modes, workers, scheduler, snapshots ------------------------

