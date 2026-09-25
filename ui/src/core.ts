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
}

export interface EnvironmentManifest {
  name: string | null
  runtime: RuntimeRequirement
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
  | { type: 'proxy'; upstream_port: number }
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
  | { type: 'remove_project'; id: string }
  | { type: 'get_project_detail'; id: string }
  | { type: 'run_in_project'; project_id: string; runtime_id: string; args: string[] }
  | { type: 'list_services' }
  | { type: 'start_service'; id: string }
  | { type: 'stop_service'; id: string }
  | { type: 'create_mysql_database'; name: string }
  | { type: 'list_db_tools' }
  | { type: 'open_db_tool'; id: string }
  | { type: 'set_custom_install'; id: string; label: string; path: string }
  | { type: 'remove_custom_install'; id: string; label: string }
  | { type: 'list_custom_installs' }
  | { type: 'get_web_status' }
  | { type: 'get_web_config' }
  | { type: 'list_domains' }
  | { type: 'get_domain'; hostname: string }
  | { type: 'add_domain'; domain: Domain }
  | { type: 'update_domain'; domain: Domain }
  | { type: 'remove_domain'; hostname: string }
  | { type: 'set_domain_enabled'; hostname: string; enabled: boolean }
  | { type: 'duplicate_domain'; hostname: string; new_hostname: string }
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
  | { type: 'get_startup_settings' }
  | { type: 'set_startup_settings'; settings: StartupSettings }
  | { type: 'open_path'; path: string }
  | { type: 'open_url'; url: string }
  | { type: 'open_in_editor'; path: string }

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
  | { type: 'secret'; key: string; value: string | null }
  | { type: 'db_tools'; tools: DbTool[] }
  | { type: 'custom_installs'; entries: CustomInstall[] }
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
