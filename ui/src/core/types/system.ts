import type { CoreCommand } from '../commands'
import type { Finding } from './environment'

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
  status: 'up_to_date' | 'update_available' | 'development_version'
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

