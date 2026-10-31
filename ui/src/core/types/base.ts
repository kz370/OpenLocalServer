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
  /** Web servers only: enabled sites it renders. Zero means it opens no listener. */
  sites?: number
  /** Log source holding this service's own output, so Logs can open on it directly. */
  log_source?: string
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

