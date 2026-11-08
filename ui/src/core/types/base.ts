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
  | { kind: 'stop_requested'; id: ProcessId; name: string }
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

export type ProjectHealth =
  | { status: 'healthy' }
  | { status: 'missing'; since: number }
  | { status: 'offline'; since: number }

export interface Project {
  id: string
  name: string
  path: string
  icon_image?: string | null
  health?: ProjectHealth
  standalone?: boolean
  auto_discovered?: boolean
  icon?: string | null
  project_type?: string | null
}

export interface ProjectTypePreset {
  id: string
  name: string
  icon: string
}

export interface CustomProjectType {
  id: string
  name: string
}

export interface ProjectFolderInspection {
  exists: boolean
  is_dir: boolean
  path: string
  suggested_name: string
  duplicate_of?: Project | null
  detected_type?: string | null
}

/**
 * A registered project the backend found to be a WordPress install, with the site that
 * serves it. Read on the Sites page to decide which rows get a "WP Admin" action.
 */
export interface WpProject {
  project_id: string
  name: string
  /** Null when the project has no domain yet: there is no URL to hand the browser. */
  hostname: string | null
  url: string | null
  /** Unix seconds a one-time sign-in link still works until; null when none is pending. */
  pending_until: number | null
}

/** One entry of the deletion history shown in Settings. */
export interface DeletedItem {
  /** The skipped project path, or the skipped hostname. */
  value: string
  /** Milliseconds since the Unix epoch; 0 for entries written before this was tracked. */
  deleted_at: number
  /** 'project' or 'domain'. */
  kind: string
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
  category?: string | null
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

/** A database copy saved before a migration (§168). */
export interface GuardSnapshot {
  id: string
  project_id: string
  engine: string
  database: string
  file: string
  /** Unix seconds. */
  created: number
  size: number
  command: string
  kind: string
}

/** Profiler (§174). */
export interface XdebugProfileFile {
  file: string
  php_version: string
  host: string | null
  created_ms: number
  size: number
}

export interface XdebugProfileFn {
  name: string
  file: string
  self_ns: number
  inclusive_ns: number
  calls: number
}

export interface CallNode {
  name: string
  inclusive_ns: number
  calls: number
  children: CallNode[]
}

export interface XdebugProfile {
  total_time_ns: number
  functions: XdebugProfileFn[]
  tree: CallNode
}

/** Windows Defender exclusions (§173). */
export interface DefenderState {
  available: boolean
  realtime_on: boolean
  error?: string | null
}

export interface DevDriveInfo {
  path: string
  file_system: string
  is_dev_drive: boolean
}

/** Import sites from Laragon / XAMPP / WampServer (§172). */
export interface SiteImportCandidate {
  source: string
  hostname: string
  aliases: string[]
  document_root: string
  project_root: string
  exists: boolean
  already_in_ols: boolean
  note: string | null
}

export interface SiteImportChoice {
  hostname: string
  document_root: string
  new_hostname?: string | null
}

/** PHP upgrade check (§171). */
export interface UpgradeItem {
  level: 'blocker' | 'warning' | 'info'
  title: string
  detail: string
  file?: string | null
  line?: number | null
}

export interface PhpUpgradeReport {
  hostname: string
  from: string
  to: string
  checked_ms: number
  items: UpgradeItem[]
  files_checked: number
  truncated: boolean
  can_switch: boolean
}

/** Free up space (§170). */
export interface ReclaimCandidate {
  relative: string
  kind: 'node_modules' | 'vendor' | 'build-cache' | 'rust-target' | 'venv'
  bytes: number
  size_is_minimum: boolean
  rebuild: string
  default_selected: boolean
}

export interface ReclaimProject {
  project_id: string
  name: string
  path: string
  idle_days: number
  busy: boolean
  items: ReclaimCandidate[]
}

export interface ReclaimReport {
  scanned_ms: number
  projects: ReclaimProject[]
  total_bytes: number
}

export interface ReclaimItem {
  project_id: string
  relative: string
}

export type PortStatusLite = 'free' | 'in_use'

export type ServiceLifecycleState = 'starting' | 'stopping' | 'restarting' | 'running' | 'stopped' | 'error'

export interface ServiceStatus {
  id: string
  name: string
  installed: boolean
  running: boolean
  state: ServiceLifecycleState
  port: number | null
  port_status: PortStatusLite | null
  kind: string
  category?: string | null
  connection: string | null
  healthy: boolean | null
  version: string | null
  /** Web servers only: enabled sites it renders. Zero means it opens no listener. */
  sites?: number
  /** Log source holding this service's own output, so Logs can open on it directly. */
  log_source?: string
}

export interface ServiceAggregate {
  running: number
  enabled_total: number
  start_service_ids: string[]
  services: ServiceStatus[]
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
  is_default: boolean
  system: SystemInstall | null
}

export type InstallState = 'downloading' | 'paused' | 'verifying' | 'extracting' | 'installed' | 'cancelled' | 'failed'

export type RuntimeEvent =
  | { kind: 'progress'; id: string; version: string; state: InstallState; downloaded: number; total: number | null }
  | { kind: 'installed'; id: string; version: string; path: string }
  | { kind: 'failed'; id: string; version: string; message: string }
  | { kind: 'cancelled'; id: string; version: string }

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
export type SslProvider = 'local_ca' | 'lets_encrypt' | 'custom'

/** §175 Dump viewer state for one project. */
export interface DumpStatus {
  enabled: boolean
  conflict: boolean
  supported: boolean
  running: ProcessId | null
  port: number | null
}

/** §176 Database per Git branch. */
export interface BranchDbConfig {
  enabled: boolean
  engine: string
  base_branch: string
  base_database: string
  clone_from: string
  map: Record<string, string>
}

export interface BranchDbInfo {
  config: BranchDbConfig | null
  current_branch: string | null
  current_database: string | null
  orphaned: string[]
  pending: string | null
  config_cache: boolean
}

/** §177 A saved server database profile. The password lives in the OS keyring, never here. */
export interface RemoteDb {
  id: string
  name: string
  ssh_target: string
  ssh_port: number | null
  engine: string
  database: string
  db_user: string
  db_host: string
  db_port: number | null
  has_password: boolean
}

/** §169 Package audit and end-of-life. */
export interface AuditFinding {
  package: string
  severity: 'critical' | 'high' | 'moderate' | 'low' | 'unknown' | string
  title: string
  advisory?: string | null
  url?: string | null
  affected?: string | null
  fix?: string | null
}

export interface AuditTool {
  tool: string
  ok: boolean
  error?: string | null
  findings: AuditFinding[]
  info: string[]
}

export interface AuditResult {
  project_id: string
  checked_ms: number
  tools: AuditTool[]
}

export type EolStatus =
  | { state: 'supported' }
  | { state: 'security_only'; until: string }
  | { state: 'ended'; since: string }
  | { state: 'unknown' }

export interface EolItem {
  runtime: string
  version: string
  status: EolStatus
  used_by: string[]
}

/** §178 Open a site on your phone. */
export interface LanInfo {
  on: boolean
  ip: string | null
  url: string | null
  qr_svg: string | null
  warnings: string[]
}
