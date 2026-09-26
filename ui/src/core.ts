// Thin typed wrapper around the single `run_command` IPC entry point (architecture decision 1).
// Every UI feature should call `runCommand`, never `invoke` directly, so there is one place
// that knows the CoreCommand/CoreResponse shape. Mirrors crates/ols-core/src/command.rs,
// crates/ols-core/src/process.rs and crates/ols-core/src/port.rs.
import { invoke } from '@tauri-apps/api/core'

export type ProcessId = number

export type ProcessState =
  | 'starting'
  | 'running'
  | 'stopping'
  | 'stopped'
  | 'crashed'
  | 'restarting'
  | 'failed'
  | 'unknown'

export interface RestartPolicy {
  max_retries: number
  delay_ms: number
}

export interface ProcessSpec {
  name: string
  executable: string
  args: string[]
  cwd: string | null
  env: [string, string][]
  restart: RestartPolicy | null
}

export interface ProcessInfo {
  id: ProcessId
  name: string
  pid: number | null
  state: ProcessState
  exit_code: number | null
  restarts: number
}

export type OutputStream = 'stdout' | 'stderr'

export type ProcessEvent =
  | { kind: 'state_changed'; id: ProcessId; state: ProcessState }
  | { kind: 'output'; id: ProcessId; stream: OutputStream; line: string }

export interface CommandHistoryEntry {
  command: string
  args: string[]
  exit_code: number | null
  duration_ms: number
  timed_out: boolean
}

export type PortStatus =
  | { status: 'free' }
  | { status: 'in_use'; pid: number | null; process_name: string | null }

export interface SystemInstall {
  path: string
  version: string
}

export type Framework =
  | 'laravel'
  | 'symfony'
  | 'word_press'
  | 'generic_php'
  | 'node'
  | 'django'
  | 'flask'
  | 'fast_api'
  | 'generic_python'
  | 'unknown'

export interface RuntimeRequirement {
  php: string | null
  node: string | null
  python: string | null
}

export interface DetectionResult {
  framework: Framework
  markers: string[]
  requirements: RuntimeRequirement
  doc_root: string | null
}

export type ServiceToggle = boolean | { enabled: boolean }
export interface WorkerManifest {
  command: string
  count: number
  timeout_secs?: number | null
  memory_mb?: number | null
}
export interface ModeManifest {
  xdebug?: boolean | null
  services?: string[]
  workers?: boolean | null
  scheduler?: boolean | null
  env?: Record<string, string>
}
/** `.openlocalserver/environment.yaml` (§71). */
export interface EnvironmentManifest {
  name?: string | null
  profile?: string | null
  runtime: { php?: string | null; node?: string | null; python?: string | null }
  extensions?: string[]
  package_manager?: string | null
  web?: { server?: string | null } | null
  domain?: { hostname: string; https: boolean; wildcard: boolean; root?: string | null; port?: number | null } | null
  database?: { engine: string; version?: string | null; name?: string | null } | null
  services?: Record<string, ServiceToggle>
  workers?: Record<string, boolean | WorkerManifest>
  scheduler?: boolean | { name: string; schedule: string; command: string }[] | null
  tunnel?: { enabled: boolean; provider?: string | null; target?: string | null; autostart: boolean } | null
  modes?: Record<string, ModeManifest>
}

export type ResolutionSource = 'manifest' | 'detected' | 'global' | 'none' | 'custom'

export interface ResolvedRuntime {
  id: string
  requested_version: string | null
  source: ResolutionSource
  installed_version: string | null
  bin_dir: string | null
}

export interface Project {
  id: string
  name: string
  path: string
}

export interface ProjectDetail {
  project: Project
  detection: DetectionResult
  manifest: EnvironmentManifest | null
  resolved: ResolvedRuntime[]
}

export type CustomHealthCheck = { kind: 'none' } | { kind: 'tcp' } | { kind: 'http'; path: string }

/** A program the user runs as a service (§67). `id` is empty when creating. */
export interface CustomServiceDef {
  id: string
  name: string
  executable: string
  args: string[]
  cwd: string | null
  env: [string, string][]
  port: number | null
  health: CustomHealthCheck
  restart_on_crash: boolean
}

export type TerminalEvent = { kind: 'output'; id: number; data: string } | { kind: 'exit'; id: number }

export interface Shortcut {
  id: string
  label: string
  path: string
  is_dir: boolean
}

export interface DbBackup {
  file: string
  database: string
  /** Unix seconds. */
  created: number
  size: number
}

export type PortStatusLite = 'free' | 'in_use'

export interface ServiceStatus {
  id: string
  name: string
  installed: boolean
  running: boolean
  port: number | null
  port_status: PortStatusLite | null
  kind: string
  connection: string | null
  healthy: boolean | null
  version: string | null
}

export interface DbTool {
  id: string
  name: string
  found_path: string | null
  engines: string[]
}

export interface CustomInstall {
  id: string
  label: string
  path: string
}

export interface CatalogEntry {
  id: string
  name: string
  version: string
  installed: boolean
  system: SystemInstall | null
}

export type InstallState = 'downloading' | 'verifying' | 'extracting' | 'installed' | 'failed'

export type RuntimeEvent =
  | { kind: 'progress'; id: string; version: string; state: InstallState; downloaded: number; total: number | null }
  | { kind: 'installed'; id: string; version: string; path: string }
  | { kind: 'failed'; id: string; version: string; message: string }

// ---- Stage 6–10 types (mirror crates/ols-core/src/{domain,certs,health,web,service,sqlite,quickapp,app}.rs)

export type Ownership = 'managed' | 'advanced' | 'manual'
export type SiteKind =
  | { type: 'php'; version: string | null }
  | { type: 'proxy'; upstream_port: number; upstream_host?: string | null; upstream_https?: boolean }
  | { type: 'static' }

export interface AppSpec {
  executable: string
  args: string[]
  cwd: string
  runtime: string | null
}
export interface HeaderRule {
  name: string
  value: string
}
export interface RedirectRule {
  from: string
  to: string
  code: number
}
export interface ProxyMapping {
  path: string
  upstream: string
}
export interface UpstreamGroup {
  name: string
  servers: string[]
}
export interface SiteBlocks {
  headers: HeaderRule[]
  redirects: RedirectRule[]
  mappings: ProxyMapping[]
  upstreams: UpstreamGroup[]
  includes: string[]
}
export interface Domain {
  hostname: string
  project_id: string | null
  root: string
  kind: SiteKind
  https: boolean
  redirect_https: boolean
  wildcard: boolean
  enabled: boolean
  ownership: Ownership
  app: AppSpec | null
  blocks: SiteBlocks
  generated_hashes: Record<string, string>
}
export interface DomainSummary {
  hostname: string
  url: string
  https: boolean
  kind: 'php' | 'proxy' | 'static'
  enabled: boolean
  project_id: string | null
  has_app: boolean
  folder: string
  /** Website type the site is listed under. */
  group: 'php' | 'nodejs' | 'python' | 'static' | 'proxy'
}

export interface WebConfig {
  server: string
  http_port: number
  https_port: number
  php_workers: number
  dns_port: number
}
export interface PoolStatus {
  version: string
  ports: number[]
  running: boolean
}
export interface ServerAvailability {
  id: string
  name: string
  installed: boolean
  active: boolean
}
export interface WebStatus {
  server: string
  servers: ServerAvailability[]
  running: boolean
  http_port: number
  https_port: number
  port_conflicts: string[]
  php_pools: PoolStatus[]
  apps: { hostname: string; running: boolean }[]
  dns_running: boolean
  dns_port: number
  error_log: string | null
}
export interface ApplyReport {
  server: string
  written: string[]
  unchanged: string[]
  drifted: string[]
  started: boolean
  reloaded: boolean
  hosts_updated: boolean
  validator_output: string
  warnings: string[]
}

export interface CaInfo {
  exists: boolean
  trusted: boolean
  common_name: string
  cert_path: string
}
export interface CertInfo {
  hostname: string
  sans: string[]
  issuer: string
  issued_at: number
  expires_at: number
  days_left: number
  status: 'valid' | 'expiring' | 'expired'
  trusted: boolean
  project_id: string | null
  cert_path: string
  key_path: string
}
export interface HealthStep {
  name: string
  ok: boolean
  skipped: boolean
  detail: string
}
export interface HealthReport {
  hostname: string
  ok: boolean
  steps: HealthStep[]
}

export type ConfigPart = 'main' | 'site' | 'custom'
export interface ConfigFile {
  hostname: string | null
  part: ConfigPart
  path: string
  ownership: Ownership | null
  drifted: boolean
  editable: boolean
}
export interface ConfigVersion {
  id: string
  part: ConfigPart
  timestamp_ms: number
  bytes: number
}

export interface DbUser {
  user: string
  host: string
}
export interface ConnectionInfo {
  engine: string
  host: string
  port: number | null
  user: string | null
  database: string | null
  path: string | null
  uri: string
}
export interface SqliteInfo {
  path: string
  name: string
  project_id: string | null
  exists: boolean
  size_bytes: number
  backups: string[]
}
export interface IntegrityResult {
  ok: boolean
  detail: string
}
export interface ExternalTool {
  id: string
  name: string
  engines: string[]
  executable: string
  args: string[]
}

export type Scalar = string | number | boolean
export interface QuickVariable {
  name: string
  label: string
  type: string
  required: boolean
  default: Scalar | null
  options: Scalar[]
  validation: string | null
  show_if: string | null
  runtime: string | null
  help: string | null
}
export interface QuickApp {
  id: string
  name: string
  description: string
  category: string
  variables: QuickVariable[]
}
export type EntrySource = 'builtin' | 'local' | 'imported'
export interface QuickEntryView {
  id: string
  name: string
  description: string
  category: string
  source: EntrySource
  trusted: boolean
  favorite: boolean
  origin: string | null
  overrides_builtin: boolean
}
export interface QuickEntryDetail {
  view: QuickEntryView
  app: QuickApp
  yaml: string
}
export interface FieldError {
  field: string
  message: string
}
export interface PlannedStep {
  stage: string
  name: string
  display: string
  elevated: boolean
  allow_failure: boolean
}
export interface Permission {
  id: string
  label: string
}
export interface RunPlan {
  app_id: string
  app_name: string
  display_values: Record<string, string>
  project_path: string | null
  hostname: string | null
  https: boolean
  steps: PlannedStep[]
  permissions: Permission[]
  warnings: string[]
}
export interface RequirementView {
  id: string
  label: string
  wanted: string | null
  status: 'installed' | 'installable' | 'unavailable'
  detail: string | null
}
export interface QuickPlanResult {
  ok: boolean
  errors: FieldError[]
  plan: RunPlan | null
  requirements: RequirementView[]
  trusted: boolean
  source: string
  values: Record<string, string>
}
export type StepStatus = 'pending' | 'running' | 'done' | 'failed' | 'skipped'
export interface RunView {
  id: string
  app_id: string
  app_name: string
  state: 'running' | 'succeeded' | 'failed' | 'cancelled'
  steps: { name: string; stage: string; display: string; status: StepStatus }[]
  log: string[]
  results: { label: string; ok: boolean; detail: string | null }[]
  open_url: string | null
  project_id: string | null
  error: string | null
  warnings: string[]
  started_ms: number
  finished_ms: number | null
}
export interface QuickCommand {
  id: string
  name: string
  description: string
  category: string
  applies_to: string[]
  working_directory: string | null
  command: { executable: string; arguments: string[] } | null
  environment: { use_project_runtime: boolean }
  action: string | null
  with: Record<string, Scalar>
  builtin: boolean
}
export interface HistoryEntry {
  id: number
  line: string
  cwd: string | null
  project_id: string | null
  timestamp_ms: number
}

export interface HealthItem {
  id: string
  label: string
  status: 'ok' | 'warn' | 'error'
  detail: string
  fix: string | null
}
export interface DashboardData {
  services: ServiceStatus[]
  web: WebStatus
  domains: DomainSummary[]
  project_count: number
  health: HealthItem[]
}
export interface LogSource {
  id: string
  name: string
  kind: string
}
export interface StartupSettings {
  with_windows: boolean
  start_minimized: boolean
  autostart_web: boolean
  autostart_services: string[]
  notifications: boolean
  close_to_tray: boolean
}

export type CoreCommand =
  | { type: 'ping' }
  | { type: 'get_setting'; key: string }
  | { type: 'set_setting'; key: string; value: unknown }
  | { type: 'start_process'; spec: ProcessSpec }
  | { type: 'stop_process'; id: ProcessId }
  | { type: 'list_processes' }
  | { type: 'get_process_output'; id: ProcessId }
  | { type: 'run_command'; executable: string; args: string[]; cwd: string | null; timeout_ms: number }
  | { type: 'list_command_history' }
  | { type: 'check_port'; port: number }
  | { type: 'list_runtime_catalog' }
  | { type: 'install_runtime'; id: string; version: string }
  | { type: 'register_project'; path: string }
  | { type: 'scan_and_register_projects'; path: string }
  | { type: 'list_projects' }
  | { type: 'sync_auto_domains' }
  | { type: 'remove_project'; id: string }
  | { type: 'get_project_detail'; id: string }
  | { type: 'run_in_project'; project_id: string; runtime_id: string; args: string[] }
  | { type: 'list_services' }
  | { type: 'start_service'; id: string }
  | { type: 'stop_service'; id: string }
  | { type: 'list_db_tools' }
  | { type: 'open_db_tool'; id: string }
  | { type: 'set_custom_install'; id: string; label: string; path: string }
  | { type: 'remove_custom_install'; id: string; label: string }
  | { type: 'list_custom_installs' }
  | { type: 'scan_php_folder'; dir: string }
  | { type: 'list_php_extensions'; version: string }
  | { type: 'set_php_extension'; version: string; name: string; enabled: boolean }
  | { type: 'install_php_extension'; version: string; name: string }
  | { type: 'list_pecl_packages' }
  | { type: 'get_web_status' }
  | { type: 'get_web_config' }
  | { type: 'list_domains' }
  | { type: 'get_domain'; hostname: string }
  | { type: 'add_domain'; domain: Domain }
  | { type: 'update_domain'; domain: Domain }
  | { type: 'remove_domain'; hostname: string }
  | { type: 'set_domain_enabled'; hostname: string; enabled: boolean }
  | { type: 'duplicate_domain'; hostname: string; new_hostname: string }
  | { type: 'rename_domain'; hostname: string; new_hostname: string }
  | { type: 'suggest_domain'; project_id: string; template: string }
  | { type: 'apply_web'; overwrite: string[] }
  | { type: 'stop_web' }
  | { type: 'validate_web' }
  | { type: 'restart_site_app'; hostname: string }
  | { type: 'sync_hosts' }
  | { type: 'get_ca_info' }
  | { type: 'trust_ca' }
  | { type: 'untrust_ca' }
  | { type: 'list_certificates' }
  | { type: 'regenerate_certificate'; hostname: string }
  | { type: 'revoke_certificate'; hostname: string }
  | { type: 'health_check'; hostname: string }
  | { type: 'list_web_configs' }
  | { type: 'read_web_config'; hostname: string | null; part: ConfigPart }
  | { type: 'write_web_config'; hostname: string; part: ConfigPart; content: string }
  | { type: 'set_ownership'; hostname: string; ownership: Ownership }
  | { type: 'list_config_history'; hostname: string }
  | { type: 'read_config_history'; hostname: string; id: string }
  | { type: 'restore_config_history'; hostname: string; id: string }
  | { type: 'export_web_config'; hostname: string; part: ConfigPart; dest: string }
  | { type: 'create_database'; engine: string; name: string }
  | { type: 'list_databases'; engine: string }
  | { type: 'list_custom_services' }
  | { type: 'save_custom_service'; service: CustomServiceDef }
  | { type: 'remove_custom_service'; id: string }
  | { type: 'backup_database'; engine: string; database: string }
  | { type: 'list_db_backups'; engine: string; database: string | null }
  | { type: 'restore_database'; engine: string; database: string; file: string }
  | { type: 'delete_db_backup'; engine: string; file: string }
  | { type: 'create_db_user'; engine: string; user: string; password: string; database: string }
  | { type: 'list_db_users'; engine: string }
  | { type: 'get_connection_info'; engine: string; database: string | null; path: string | null }
  | { type: 'list_sqlite' }
  | { type: 'detect_sqlite'; project_id: string }
  | { type: 'create_sqlite'; path: string; project_id: string | null }
  | { type: 'associate_sqlite'; path: string; project_id: string | null }
  | { type: 'forget_sqlite'; path: string }
  | { type: 'backup_sqlite'; path: string }
  | { type: 'restore_sqlite'; path: string; backup: string }
  | { type: 'check_sqlite'; path: string }
  | { type: 'list_external_tools' }
  | { type: 'save_external_tool'; tool: ExternalTool }
  | { type: 'remove_external_tool'; id: string }
  | { type: 'open_database'; engine: string; database: string | null; path: string | null; tool_id: string | null }
  | { type: 'list_quick_apps' }
  | { type: 'get_quick_app'; id: string }
  | { type: 'save_quick_app'; yaml: string }
  | { type: 'duplicate_quick_app'; id: string; new_id: string; new_name: string }
  | { type: 'delete_quick_app'; id: string }
  | { type: 'favorite_quick_app'; id: string; favorite: boolean }
  | { type: 'export_quick_app'; id: string; dest: string }
  | { type: 'import_quick_app'; source: string }
  | { type: 'trust_quick_app_source'; origin: string }
  | { type: 'plan_quick_app'; id: string; values: Record<string, string> }
  | {
      type: 'start_quick_app'
      id: string
      values: Record<string, string>
      approval: 'once' | 'source' | null
      allow_elevated: boolean
    }
  | { type: 'get_quick_run'; id: string }
  | { type: 'list_quick_runs' }
  | { type: 'cancel_quick_run'; id: string }
  | { type: 'list_quick_commands' }
  | { type: 'save_quick_command'; command: QuickCommand }
  | { type: 'delete_quick_command'; id: string }
  | { type: 'run_quick_command'; id: string; project_id: string | null }
  | { type: 'run_command_line'; line: string; cwd: string | null; project_id: string | null }
  | { type: 'list_history' }
  | { type: 'delete_history'; id: number }
  | { type: 'clear_history' }
  | { type: 'save_history_as_quick_command'; id: number; command_id: string; name: string }
  | { type: 'get_dashboard' }
  | { type: 'get_environment_health' }
  | { type: 'list_log_sources' }
  | { type: 'read_log'; source: string; max_lines: number }
  | { type: 'export_log'; source: string; dest: string }
  | { type: 'clear_log'; source: string }
  | { type: 'get_startup_settings' }
  | { type: 'set_startup_settings'; settings: StartupSettings }
  | { type: 'open_path'; path: string }
  | { type: 'open_url'; url: string }
  | { type: 'open_in_editor'; path: string }
  | { type: 'open_with'; path: string; app: string }
  | { type: 'open_terminal'; project_id: string | null; shell: string | null; rows: number; cols: number }
  | { type: 'terminal_input'; id: number; data: string }
  | { type: 'resize_terminal'; id: number; rows: number; cols: number }
  | { type: 'close_terminal'; id: number }
  | { type: 'list_project_shortcuts'; project_id: string }
  | { type: 'list_editors' }
  | { type: 'get_system_stats' }
  | { type: 'list_migration_sources' }
  | { type: 'list_foreign_databases'; source_id: string; password: string }
  | { type: 'migrate_databases'; source_id: string; password: string; databases: string[]; target: string }
  | { type: 'get_migration_progress' }
  | { type: 'get_helper_service' }
  | { type: 'install_helper_service' }
  | { type: 'uninstall_helper_service' }
  | { type: 'get_xdebug'; version: string }
  | { type: 'set_xdebug'; version: string; settings: XdebugSettings }
  | { type: 'xdebug_ide_config'; project_id: string; ide: string; version: string }
  | { type: 'discover_commands'; project_id: string }
  | { type: 'get_composer_info'; project_id: string }
  | { type: 'run_composer'; project_id: string; action: string; target: string | null }
  | { type: 'get_package_managers'; project_id: string }
  | { type: 'enable_package_manager'; project_id: string; manager: string }
  | { type: 'get_venv'; project_id: string }
  | { type: 'create_venv'; project_id: string; recreate: boolean }
  | { type: 'install_venv_requirements'; project_id: string; what: string }
  | { type: 'run_diagnostics' }
  | { type: 'ignore_diagnostic'; id: string; ignore: boolean }
  | { type: 'restart_service'; id: string }
  | { type: 'list_env_files'; project_id: string }
  | { type: 'read_env_file'; project_id: string; file: string }
  | { type: 'save_env_file'; project_id: string; file: string; content: string }
  | { type: 'set_env_value'; project_id: string; file: string; key: string; value: string }
  | { type: 'delete_env_key'; project_id: string; file: string; key: string }
  | { type: 'compare_env_files'; project_id: string; a: string; b: string }
  | { type: 'import_env_file'; project_id: string; file: string; source: string; mode: 'merge' | 'replace' }
  | { type: 'export_env_file'; project_id: string; file: string; dest: string }
  | { type: 'create_env_file'; project_id: string; file: string; from: string | null }
  | { type: 'list_operations' }
  | { type: 'dismiss_operation'; id: number }
  | { type: 'mailpit_env_plan'; project_id: string; file: string }
  | { type: 'apply_mailpit_env'; project_id: string; file: string }
  | { type: 'mail_diagnostics'; project_id: string | null }
  | { type: 'send_test_mail'; to: string }
  // Stage 12
  | { type: 'get_manifest'; project_id: string }
  | { type: 'save_manifest'; project_id: string; manifest: EnvironmentManifest | null }
  | { type: 'save_manifest_text'; project_id: string; text: string }
  | { type: 'plan_setup'; project_id: string }
  | { type: 'apply_setup'; project_id: string; dry_run: boolean }
  | { type: 'get_setup_progress' }
  // Stage 13
  | { type: 'list_profiles' }
  | { type: 'save_profile'; profile: Profile }
  | { type: 'delete_profile'; id: string }
  | { type: 'export_profile'; id: string; dest: string }
  | { type: 'read_profile_file'; source: string }
  | { type: 'import_profile'; source: string }
  | { type: 'apply_profile'; project_id: string; profile_id: string }
  | { type: 'profile_from_project'; project_id: string; name: string }
  | { type: 'profile_yaml'; id: string }
  | { type: 'save_profile_yaml'; yaml: string }
  | { type: 'get_project_modes'; project_id: string }
  | { type: 'set_project_mode'; project_id: string; mode: string }
  | { type: 'list_workers'; project_id: string | null }
  | { type: 'list_worker_presets' }
  | { type: 'save_worker'; worker: Worker }
  | { type: 'remove_worker'; id: string }
  | { type: 'start_worker'; id: string }
  | { type: 'stop_worker'; id: string }
  | { type: 'restart_worker'; id: string }
  | { type: 'start_project_workers'; project_id: string }
  | { type: 'stop_project_workers'; project_id: string }
  | { type: 'list_schedules'; project_id: string | null }
  | { type: 'save_schedule'; task: ScheduledTask }
  | { type: 'remove_schedule'; id: string }
  | { type: 'run_schedule_now'; id: string }
  | { type: 'describe_schedule'; schedule: string }
  | { type: 'list_snapshots'; project_id: string }
  | { type: 'create_snapshot'; project_id: string; label: string; options: SnapshotOptions }
  | { type: 'delete_snapshot'; project_id: string; id: string }
  | { type: 'restore_snapshot'; project_id: string; id: string; options: RestoreOptions }
  | { type: 'export_snapshot'; project_id: string; id: string; dest: string }
  | { type: 'preview_import'; source: string }
  | { type: 'import_environment'; source: string; target: string; name: string }
  | { type: 'clone_environment'; project_id: string; target: string; name: string; what: 'full' | 'infrastructure' | 'configuration' }
  | { type: 'backup_settings' }
  | { type: 'list_settings_backups' }
  | { type: 'restore_settings'; id: string }
  | { type: 'get_resource_limits' }
  | { type: 'set_resource_limits'; limits: ResourceLimits }
  // Stage 14
  | { type: 'list_tunnel_providers' }
  | { type: 'list_tunnels' }
  | { type: 'save_tunnel'; tunnel: TunnelConfig }
  | { type: 'remove_tunnel'; id: string }
  | { type: 'start_tunnel'; id: string; confirm_exposure: boolean }
  | { type: 'stop_tunnel'; id: string }
  | { type: 'check_tunnel'; id: string }
  | { type: 'tunnel_log'; id: string }
  | { type: 'set_tunnel_token'; provider: string; token: string | null }
  | { type: 'set_tunnel_password'; id: string; password: string | null }
  | { type: 'list_tunnel_requests'; id: string }
  | { type: 'clear_tunnel_requests'; id: string }
  | { type: 'replay_tunnel_request'; id: string; request_id: number }
  | { type: 'send_tunnel_test_request'; id: string; method: string; path: string; headers: [string, string][]; body: string }
  // Stage 15
  | { type: 'global_search'; query: string }
  | { type: 'doctor' }
  | { type: 'diagnose_project'; project_id: string }
  | { type: 'plan_repair'; project_id: string | null }
  | { type: 'apply_repair'; project_id: string | null; ids: string[]; confirm_destructive: boolean }
  | { type: 'git_status'; project_id: string }
  | { type: 'git_init'; project_id: string }
  | { type: 'git_branches'; project_id: string }
  | { type: 'git_create_branch'; project_id: string; name: string; checkout: boolean }
  | { type: 'git_switch_branch'; project_id: string; name: string }
  | { type: 'git_delete_branch'; project_id: string; name: string; force: boolean }
  | { type: 'git_stage'; project_id: string; paths: string[] }
  | { type: 'git_unstage'; project_id: string; paths: string[] }
  | { type: 'git_discard'; project_id: string; paths: string[] }
  | { type: 'git_commit'; project_id: string; message: string; amend: boolean }
  | { type: 'git_sync'; project_id: string; action: 'pull' | 'push' | 'fetch'; remote: string | null }
  | { type: 'git_diff'; project_id: string; path: string; staged: boolean }
  | { type: 'git_log'; project_id: string; limit: number }
  | { type: 'git_show'; project_id: string; hash: string }
  | { type: 'git_add_remote'; project_id: string; name: string; url: string }
  | { type: 'git_remove_remote'; project_id: string; name: string }
  | { type: 'git_stash'; project_id: string; action: 'push' | 'pop' | 'apply' | 'drop'; message: string | null; index: number | null }
  | { type: 'git_add_ignore'; project_id: string; template: string }
  | { type: 'git_set_credentials'; host: string; username: string; token: string | null }
  | { type: 'git_clone'; url: string; target: string; branch: string | null; auth: { type: 'https'; username: string; password: string; remember: boolean } | { type: 'ssh'; key_path: string; remember: boolean; passphrase: string | null } | null }
  | { type: 'list_plugins' }
  | { type: 'install_plugin'; source: string }
  | { type: 'set_plugin_enabled'; id: string; enabled: boolean; approve: string[] }
  | { type: 'remove_plugin'; id: string }
  | { type: 'plugin_detect'; project_id: string }
  | { type: 'list_catalog_sources' }
  | { type: 'add_catalog_source'; name: string; url: string; public_key: string }
  | { type: 'remove_catalog_source'; id: string }
  | { type: 'refresh_catalogs'; id: string | null }
  | { type: 'install_catalog_plugin'; source_id: string; plugin_id: string }
  | { type: 'get_api_status' }
  | { type: 'set_api_settings'; enabled: boolean; port: number; mode: 'read_only' | 'operate' }
  | { type: 'rotate_api_token' }
  | { type: 'clear_api_token' }
  | { type: 'get_updater_status' }
  | { type: 'set_updater_settings'; endpoint: string; public_key: string }
  | { type: 'check_update' }
  | { type: 'download_update' }
  | { type: 'install_update' }
  | { type: 'get_shell_menu' }
  | { type: 'install_shell_menu' }
  | { type: 'remove_shell_menu' }
  | { type: 'check_network'; force: boolean }
  | { type: 'export_support_bundle'; dest: string }
  | { type: 'load_overview'; project_id: string }
  | { type: 'load_read_script'; project_id: string; name: string }
  | { type: 'load_save_script'; project_id: string; name: string; content: string }
  | { type: 'load_delete_script'; project_id: string; name: string }
  | { type: 'load_list_profiles' }
  | { type: 'load_save_profile'; profile: LoadProfile }
  | { type: 'load_delete_profile'; id: string }
  | { type: 'load_generate'; project_id: string; profile: LoadProfile; name: string | null }
  | { type: 'load_run'; project_id: string; script: string; target: string | null; confirm_public: boolean; env: [string, string][] }
  | { type: 'load_status'; run_id: string }
  | { type: 'load_stop'; run_id: string }
  | { type: 'load_runs'; project_id: string }
  | { type: 'load_delete_run'; project_id: string; run_id: string }
  | { type: 'ai_get_state' }
  | { type: 'ai_save_settings'; enabled: boolean; features: Record<string, string> }
  | { type: 'ai_save_provider'; provider: AiProvider; api_key: string | null }
  | { type: 'ai_remove_provider'; id: string }
  | { type: 'ai_detect_local' }
  | { type: 'ai_test'; provider_id: string }
  | { type: 'ai_models'; provider_id: string }
  | { type: 'ai_probe'; provider: AiProvider; api_key: string | null }
  | { type: 'ai_preview'; request: AiRequest }
  | { type: 'ai_start'; request: AiRequest; confirm_remote: boolean }
  | { type: 'ai_job'; job_id: string }
  | { type: 'ai_cancel'; job_id: string }
  | { type: 'ai_apply'; actions: CoreCommand[]; confirm_destructive: boolean }
  // @@ts-commands-end

export type CoreResponse =
  | { type: 'pong'; version: string }
  | { type: 'setting'; key: string; value: unknown | null }
  | { type: 'ok' }
  | { type: 'process_started'; id: ProcessId }
  | { type: 'processes'; processes: ProcessInfo[] }
  | { type: 'process_output'; id: ProcessId; lines: string[] }
  | { type: 'command_result'; entry: CommandHistoryEntry }
  | { type: 'command_history'; entries: CommandHistoryEntry[] }
  | { type: 'port'; port: number; status: PortStatus }
  | { type: 'runtime_catalog'; entries: CatalogEntry[] }
  | { type: 'project'; project: Project }
  | { type: 'projects'; projects: Project[] }
  | { type: 'project_detail'; detail: ProjectDetail }
  | { type: 'services'; services: ServiceStatus[] }
  | { type: 'db_backups'; backups: DbBackup[] }
  | { type: 'shortcuts'; shortcuts: Shortcut[] }
  | { type: 'terminal'; id: number }
  | { type: 'custom_services'; services: CustomServiceDef[] }
  | { type: 'custom_service'; service: CustomServiceDef }
  | { type: 'secret'; key: string; value: string | null }
  | { type: 'db_tools'; tools: DbTool[] }
  | { type: 'custom_installs'; entries: CustomInstall[] }
  | { type: 'php_scan'; found: { version: string; php_exe: string }[] }
  | { type: 'php_extensions'; report: PhpExtensions }
  | { type: 'web_status'; status: WebStatus }
  | { type: 'web_config'; config: WebConfig }
  | { type: 'domains'; domains: DomainSummary[] }
  | { type: 'domain'; domain: Domain }
  | { type: 'text'; text: string }
  | { type: 'applied'; report: ApplyReport }
  | { type: 'ca_info'; info: CaInfo }
  | { type: 'certificates'; certs: CertInfo[] }
  | { type: 'health'; report: HealthReport }
  | { type: 'configs'; files: ConfigFile[] }
  | { type: 'config_versions'; versions: ConfigVersion[] }
  | { type: 'names'; names: string[] }
  | { type: 'db_users'; users: DbUser[] }
  | { type: 'connection'; info: ConnectionInfo }
  | { type: 'sqlite_list'; databases: SqliteInfo[] }
  | { type: 'sqlite_info'; info: SqliteInfo }
  | { type: 'integrity'; result: IntegrityResult }
  | { type: 'external_tools'; tools: ExternalTool[] }
  | { type: 'quick_apps'; apps: QuickEntryView[] }
  | { type: 'quick_app'; detail: QuickEntryDetail }
  | { type: 'quick_plan'; result: QuickPlanResult }
  | { type: 'quick_run_started'; run_id: string }
  | { type: 'quick_run'; run: RunView }
  | { type: 'quick_runs'; runs: RunView[] }
  | { type: 'quick_commands'; commands: QuickCommand[] }
  | { type: 'history'; entries: HistoryEntry[] }
  | { type: 'maybe_process'; id: ProcessId | null }
  | { type: 'dashboard'; data: DashboardData }
  | { type: 'environment_health'; items: HealthItem[] }
  | { type: 'log_sources'; sources: LogSource[] }
  | { type: 'log_lines'; source: string; lines: string[] }
  | { type: 'startup'; settings: StartupSettings }
  | { type: 'editors'; editors: EditorInfo[] }
  | { type: 'count'; count: number }
  | { type: 'helper_service'; installed: boolean }
  | { type: 'system_stats'; stats: SystemStats }
  | { type: 'migration_sources'; sources: MigrationSource[] }
  | { type: 'migrated'; results: MigratedDb[] }
  | { type: 'migration_progress'; progress: MigrationProgress }
  | { type: 'xdebug'; report: XdebugReport }
  | { type: 'composer'; info: ComposerInfo }
  | { type: 'package_managers'; info: PackageManagerInfo }
  | { type: 'command_sources'; sources: CommandSource[] }
  | { type: 'venv'; info: VenvInfo }
  | { type: 'diagnostics'; findings: Finding[] }
  | { type: 'env_files'; files: EnvFileInfo[] }
  | { type: 'env_file'; view: EnvFileView }
  | { type: 'env_compare'; rows: EnvDiffRow[] }
  | { type: 'mail_env_plan'; plan: MailEnvPlan }
  | { type: 'mail_checks'; checks: MailCheck[] }
  | { type: 'manifest_info'; info: ManifestInfo }
  | { type: 'manifest'; manifest: EnvironmentManifest }
  | { type: 'setup_plan'; plan: EnvironmentPlan }
  | { type: 'setup'; report: SetupReport }
  | { type: 'setup_progress'; report: SetupReport | null }
  | { type: 'profiles'; profiles: Profile[] }
  | { type: 'profile'; profile: Profile }
  | { type: 'modes'; view: ModesView }
  | { type: 'mode_result'; result: ModeResult }
  | { type: 'workers'; workers: WorkerStatus[] }
  | { type: 'worker_presets'; presets: WorkerPreset[] }
  | { type: 'schedules'; tasks: TaskStatus[] }
  | { type: 'task_run'; run: TaskRun }
  | { type: 'snapshots'; snapshots: SnapshotInfo[] }
  | { type: 'snapshot'; snapshot: SnapshotInfo }
  | { type: 'restored'; result: RestoreResult }
  | { type: 'import_preview'; preview: ImportPreview }
  | { type: 'cloned'; result: CloneResult }
  | { type: 'settings_backups'; backups: SettingsBackup[] }
  | { type: 'settings_backup'; backup: SettingsBackup }
  | { type: 'resources'; limits: ResourceLimits }
  | { type: 'tunnel_providers'; providers: TunnelProvider[] }
  | { type: 'tunnels'; tunnels: TunnelStatus[] }
  | { type: 'tunnel'; tunnel: TunnelStatus }
  | { type: 'lines'; lines: string[] }
  | { type: 'tunnel_requests'; requests: RecordedRequest[] }
  | { type: 'tunnel_request'; request: RecordedRequest }
  | { type: 'search_results'; hits: SearchHit[] }
  | { type: 'doctor_report'; report: DoctorReport }
  | { type: 'repair_plan'; plan: RepairPlan }
  | { type: 'repair_report'; report: RepairReport }
  | { type: 'git_status'; status: GitStatus }
  | { type: 'git_branches'; branches: GitBranch[] }
  | { type: 'git_commits'; commits: GitCommit[] }
  | { type: 'git_commit'; commit: GitCommit }
  | { type: 'git_result'; result: { ok: boolean; output: string } }
  | { type: 'operations'; operations: Operation[] }
  | { type: 'plugins'; plugins: PluginInfo[] }
  | { type: 'plugin'; plugin: PluginInfo }
  | { type: 'plugin_detections'; detections: PluginDetection[] }
  | { type: 'catalog_sources'; catalogs: CatalogView[] }
  | { type: 'api_status'; status: ApiStatus }
  | { type: 'updater_status'; status: UpdaterStatus }
  | { type: 'update'; update: UpdateInfo }
  | { type: 'shell_menu'; status: ShellMenuStatus }
  | { type: 'network'; status: NetworkStatus }
  | { type: 'load_overview'; overview: LoadOverview }
  | { type: 'load_run'; run: LoadRun }
  | { type: 'load_runs'; runs: LoadRun[] }
  | { type: 'load_profiles'; profiles: LoadProfile[] }
  | { type: 'ai_state'; state: AiState }
  | { type: 'ai_detected'; servers: AiDetected[] }
  | { type: 'ai_test'; result: AiTestResult }
  | { type: 'ai_models'; models: AiModel[] }
  | { type: 'ai_prompt'; prompt: AiPrompt }
  | { type: 'ai_job'; job: AiJobView }
  | { type: 'ai_applied'; steps: { label: string; ok: boolean; detail: string }[] }
  // @@ts-responses-end

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
  /** Drives holding OpenLocalServer's data or your projects. */
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

export interface ResourceLimits {
  mariadb_buffer_pool_mb: number | null
  postgres_shared_buffers_mb: number | null
  redis_maxmemory_mb: number | null
  mongodb_cache_mb: number | null
  node_max_old_space_mb: number | null
  max_worker_count: number | null
  max_processes: number | null
  k6_max_vus: number | null
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

export interface RecordedRequest {
  id: number
  time_ms: number
  method: string
  path: string
  status: number
  duration_ms: number
  request_headers: [string, string][]
  response_headers: [string, string][]
  request_size: number
  response_size: number
  request_body: string | null
  response_body: string | null
  client: string | null
  replay: boolean
  error: string | null
}

// ---- Stage 15: search, doctor, repair, Git -------------------------------------------

export interface SearchHit {
  kind: 'project' | 'service' | 'site' | 'quick_app' | 'quick_command' | 'runtime' | 'tunnel' | 'config' | 'log'
  target: string
  title: string
  subtitle: string
  excerpt: string | null
}

export interface DoctorReport {
  checks: { label: string; status: 'ok' | 'warning' | 'error' | 'info'; detail: string }[]
  findings: Finding[]
  warnings: number
  errors: number
}

export interface RepairPlan {
  project_id: string | null
  findings: Finding[]
  actions: { finding_id: string; label: string; command: CoreCommand; destructive: boolean }[]
  manual: string[]
}

export interface RepairReport {
  steps: { label: string; ok: boolean; detail: string }[]
  after: Finding[]
  fixed: number
}

export interface GitFile {
  path: string
  from: string | null
  index: string
  worktree: string
  staged: boolean
  unstaged: boolean
  untracked: boolean
  conflicted: boolean
  kind: 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'conflicted'
}

export interface GitCommit {
  hash: string
  short: string
  author: string
  email: string
  time: number
  subject: string
}

export interface GitBranch {
  name: string
  current: boolean
  remote: boolean
  upstream: string | null
  commit: string
  subject: string
  time: number
}

export interface GitStatus {
  available: boolean
  git_path: string | null
  version: string | null
  is_repo: boolean
  branch: string | null
  detached: boolean
  upstream: string | null
  ahead: number
  behind: number
  files: GitFile[]
  last_commit: GitCommit | null
  remotes: { name: string; url: string; has_credentials: boolean }[]
  stashes: string[]
  has_gitignore: boolean
  operation: string | null
}

export interface Diagnostic {
  problem: string
  cause: string
  fix: string | null
}

export async function runCommand(command: CoreCommand): Promise<CoreResponse> {
  try {
    return await invoke<CoreResponse>('run_command', { command })
  } catch (err) {
    // Tauri surfaces our Diagnostic as the rejection value.
    throw err as Diagnostic
  }
}


// ---- Stage 16: plugins and signed catalogs ---------------------------------------------

export interface PluginManifest {
  id: string
  name: string
  version: string
  description: string
  author: string
  homepage: string
  kind: 'declarative' | 'wasm'
  permissions: string[]
  contributes: {
    runtimes: { id: string; name: string; version: string }[]
    quick_apps: string | null
    detections: { id: string; name: string; markers: string[] }[]
    health_checks: { id: string; name: string; kind: string; target: string }[]
  }
}

export interface PluginInfo {
  manifest: PluginManifest
  builtin: boolean
  enabled: boolean
  approved: boolean
  permissions: { id: string; description: string; used: boolean }[]
  problem: string | null
  runtimes: number
  quick_apps: number
  detections: number
  health_checks: number
  folder: string | null
}

export interface PluginDetection {
  plugin: string
  id: string
  name: string
  matched: string[]
}

export interface CatalogView {
  source: { id: string; name: string; url: string; public_key: string }
  verified: boolean
  note: string | null
  refreshed_ms: number | null
  doc: {
    name: string
    runtimes: { id: string; name: string; version: string }[]
    plugins: { id: string; name: string; version: string; description: string; url: string; sha256: string }[]
    quick_app_sources: { name: string; url: string; description: string }[]
  } | null
  error: string | null
}

// ---- Stage 17: release hardening --------------------------------------------------------

export interface ApiStatus {
  settings: { enabled: boolean; port: number; mode: 'read_only' | 'operate' }
  token_set: boolean
  running: boolean
  error: string | null
  url: string
}

export interface UpdateInfo {
  current: string
  latest: string
  available: boolean
  notes: string
  date: string
  url: string
  sha256: string
  size: number
  downloaded: string | null
}

export interface UpdaterStatus {
  settings: { endpoint: string; public_key: string }
  current: string
  key_configured: boolean
  last: UpdateInfo | null
}

export interface ShellMenuStatus {
  installed: boolean
  cli_path: string | null
  supported: boolean
}

export interface NetworkStatus {
  online: boolean
  probes: { name: string; ok: boolean; ms: number | null }[]
  needs_internet: string[]
}

// ---- Stage 18: load testing with k6 -----------------------------------------------------

export interface LoadOverview {
  k6: { installed: boolean; path: string | null; version: string | null; managed: boolean }
  scripts: { name: string; size: number }[]
  sites: { host: string; url: string; public: boolean }[]
  max_vus: number
}

export interface LoadProfile {
  id: string
  name: string
  description: string
  builtin: boolean
  icon: string
  stages: { duration_s: number; target: number }[]
  think_time_s: number
  requests: { method: string; path: string; body: string | null }[]
  headers: { name: string; value: string }[]
  variables: { name: string; value: string; secret: boolean }[]
  thresholds: { p95_ms: number | null; p99_ms: number | null; error_rate_pct: number | null }
}

export interface LoadMetrics {
  requests: number
  failed: number
  error_rate: number
  rps: number
  avg_ms: number
  p50_ms: number
  p95_ms: number
  p99_ms: number
  max_ms: number
  vus: number
  checks_passed: number
  checks_failed: number
  iterations: number
}

export interface LoadRun {
  id: string
  project_id: string
  script: string
  target: string
  state: 'running' | 'passed' | 'failed' | 'error' | 'stopped'
  started_ms: number
  finished_ms: number | null
  exit_code: number | null
  metrics: LoadMetrics
  series: { t: number; rps: number; p95_ms: number; vus: number; errors: number }[]
  output: string[]
  message: string | null
}
// ---- Stage 19: AI assistant ---------------------------------------------------------------

export type AiKind = 'lmstudio' | 'ollama' | 'huggingface' | 'openrouter' | 'custom'

export interface AiProvider {
  id: string
  name: string
  kind: AiKind
  base_url: string
  model: string
  tools: boolean
  /** Runs on this computer. Computed by the core. */
  local: boolean
  /** A key is stored for it. Computed by the core. */
  has_key: boolean
}

export interface AiState {
  settings: { enabled: boolean; providers: AiProvider[]; features: Record<string, string> }
  features: { id: string; label: string }[]
}

export interface AiModel {
  id: string
  name: string
  context: number | null
  prompt_per_m: number | null
  completion_per_m: number | null
}

export interface AiTestResult {
  ok: boolean
  message: string
  ms: number
  models: AiModel[]
}

export interface AiDetected {
  kind: AiKind
  name: string
  base_url: string
  models: string[]
}

export interface AiRequest {
  feature: 'explain' | 'config' | 'logs' | 'traffic' | 'commit' | 'palette'
  kind?: string
  project_id?: string | null
  question?: string | null
  title?: string | null
  text?: string | null
  tunnel_id?: string | null
  request_ids?: number[]
  log_sources?: string[]
}

export interface AiPrompt {
  provider_id: string
  provider_name: string
  model: string
  local: boolean
  host: string
  messages: { role: string; content: string }[]
  attachments: string[]
  tools: string[]
}

export interface AiAction {
  label: string
  command: CoreCommand
  destructive: boolean
}

export interface AiAnswer {
  text: string
  actions: AiAction[]
  rejected: string[]
  manifest: string | null
  script: string | null
  commit_message: string | null
  provider: string
  model: string
  local: boolean
  tokens_in: number | null
  tokens_out: number | null
  cost_usd: number | null
  used: string[]
}

export interface AiJobView {
  id: string
  feature: string
  state: 'running' | 'done' | 'failed' | 'cancelled'
  partial: string
  activity: string[]
  answer: AiAnswer | null
  error: string | null
  provider: string
  local: boolean
}

// @@ts-types
