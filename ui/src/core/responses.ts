import type { AiDetected, AiJobView, AiModel, AiPrompt, AiState, AiTestResult } from './types/ai'
import type { DashboardData, HealthItem, HistoryEntry, LogSource, QuickCommand, QuickEntryDetail, QuickEntryView, QuickPlanResult, RunView, StartupSettings } from './types/apps'
import type { CatalogEntry, CommandHistoryEntry, CustomInstall, CustomServiceDef, DbBackup, DbTool, DeletedItem, EnvironmentManifest, PortStatus, ProcessId, ProcessInfo, Project, ProjectDetail, ServiceStatus, Shortcut, WpProject } from './types/base'
import type { CommandSource, ComposerInfo, EditorInfo, EnvDiffRow, EnvFileInfo, EnvFileView, EnvironmentPlan, Finding, MailCheck, MailEnvPlan, ManifestInfo, MigratedDb, MigrationProgress, MigrationSource, Operation, PackageManagerInfo, PhpExtensions, SetupReport, SiteFolderInspection, SystemStats, VenvInfo, XdebugReport } from './types/environment'
import type { ApiStatus, CatalogView, DoctorReport, GitBranch, GitCommit, GitStatus, LoadOverview, LoadProfile, LoadRun, NetworkStatus, PluginDetection, PluginInfo, RecordedRequest, RepairPlan, RepairReport, SearchHit, ShellMenuStatus, UpdateInfo, UpdaterStatus } from './types/system'
import type { AutoBackupRun, AutoBackupStatus, BundleImportResult, BundlePreview, CloneResult, CpuCapStatus, ImportPreview, ModeResult, ModesView, Profile, ResourceLimits, RestoreResult, SettingsBackup, SnapshotInfo, TaskRun, TaskStatus, TaskView, TunnelProvider, TunnelStatus, WorkerPreset, WorkerStatus } from './types/tasks'
import type { ApplyReport, CaInfo, CertInfo, ConfigFile, ConfigVersion, ConnectionInfo, DbUser, DdnsSyncResult, Domain, DomainSummary, ExternalTool, HealthReport, IntegrityResult, ProcfilePreview, SqliteInfo, WebConfig, WebStatus } from './types/web'

export type CoreResponse =
  | { type: 'pong'; version: string }
  | { type: 'setting'; key: string; value: unknown | null }
  | { type: 'ok' }
  | { type: 'stopped_everything'; still_running: string[] }
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
  | { type: 'site_folder_inspection'; inspection: SiteFolderInspection }
  | { type: 'wp_projects'; projects: WpProject[] }
  | { type: 'wp_sign_in'; url: string; expires_at: number }
  | { type: 'deleted_items'; items: DeletedItem[] }
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
  | { type: 'image_file'; data_url: string }
  | { type: 'text'; text: string }
  | { type: 'applied'; reports: ApplyReport[] }
  | { type: 'ca_info'; info: CaInfo }
  | { type: 'certificates'; certs: CertInfo[] }
  | { type: 'certificate'; cert: CertInfo }
  | { type: 'ssl_verification'; valid: boolean; error: string | null }
  | { type: 'ddns_sync'; result: DdnsSyncResult }
  | { type: 'ddns_sync_all'; results: DdnsSyncResult[] }
  | { type: 'public_ip'; ip: string }
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
  /** What a bulk site edit did. `skipped` names every site the batch refused and why, so
   *  a partly applied edit is reported rather than looking like it fully worked. */
  | { type: 'bulk'; changed: number; skipped: string[] }
  | { type: 'helper_service'; installed: boolean; version: string | null; status: 'development_version' | 'up_to_date' | 'update_available' | null }
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
  | { type: 'procfile'; preview: ProcfilePreview }
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
  | { type: 'resources'; limits: ResourceLimits; cpu: CpuCapStatus }
  | { type: 'auto_backup'; status: AutoBackupStatus }
  | { type: 'auto_backup_run'; run: AutoBackupRun }
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
  | { type: 'ssh_keys'; keys: string[] }
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
  | { type: 'task_started'; id: string }
  | { type: 'tasks'; tasks: TaskView[] }
  | { type: 'site_bundle_preview'; preview: BundlePreview }
  | { type: 'site_bundle_imported'; result: BundleImportResult }
  | { type: 'app_guide'; markdown: string; version: string }
// @@ts-responses-end

