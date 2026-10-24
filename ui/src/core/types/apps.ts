import type { ServiceStatus } from './base'
import type { DomainSummary, WebStatus } from './web'

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
  autostart_services: string[]
  notifications: boolean
  close_to_tray: boolean
}

/**
 * The resident administrator helper. It runs a *copy* of `ols-helper` in an admin-only
 * folder, so it can drift from the app it serves after an update or a move.
 * `status: null` means the service is not answering, which is "not installed" rather
 * than "out of date" — the two need different words on screen. `status` compares the
 * service copy against this app with SemVer rules: `development_version` when the
 * service is newer (dev build), `up_to_date` when they match, `update_available` when
 * the service is older.
 */
export interface HelperService {
  installed: boolean
  version: string | null
  status: 'development_version' | 'up_to_date' | 'update_available' | null
}

