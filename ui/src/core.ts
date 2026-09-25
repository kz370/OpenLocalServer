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

export type ResolutionSource = 'manifest' | 'detected' | 'global' | 'none'

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
