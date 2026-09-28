//! The `CoreCommand` dispatcher (architecture decision 1): every front door — Tauri IPC,
//! the CLI, and later the local HTTP API — routes through this single enum + `dispatch` fn.
//! The UI/CLI never talks to individual managers directly.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{
    DomainSummary, HealthItem, Host, Inner, LogSource, QuickPlanResult, StartupSettings,
};
use crate::certs::{CaInfo, CertInfo};
use crate::custom_install::CustomInstall;
use crate::dbtools::{DbTool, ExternalTool};
use crate::domain::{Domain, Ownership};
use crate::error::{CoreError, Diagnostic};
use crate::health::HealthReport;
use crate::paths::AppPaths;
use crate::port::{self, PortStatus};
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo, ProcessSpec, ProcessSupervisor};
use crate::project::{Project, ProjectDetail};
use crate::quickapp::commands::{quick_command_from_line, HistoryEntry, QuickCommand};
use crate::quickapp::{EntryDetail, EntryView, RunView};
use crate::resolver::ResolutionSource;
use crate::runtime::{CatalogEntry, RuntimeManager};
use crate::service::{ConnectionInfo, DbUser, ServiceManager, ServiceStatus};
use crate::settings::SettingsService;
use crate::sqlite::{IntegrityResult, SqliteInfo};
use crate::web::manager::{ApplyReport, ConfigFile, ConfigPart, ConfigVersion, WebStatus};
use crate::web::WebConfig;
use crate::xdebug::{XdebugReport, XdebugSettings};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreCommand {
    /// Trivial liveness check the UI calls on startup.
    Ping,
    GetSetting {
        key: String,
    },
    SetSetting {
        key: String,
        value: Value,
    },

    // Process Supervisor (§107, Stage 2)
    StartProcess {
        spec: ProcessSpec,
    },
    StopProcess {
        id: ProcessId,
    },
    ListProcesses,
    GetProcessOutput {
        id: ProcessId,
    },

    // Command Runner (§90–91, Stage 2)
    RunCommand {
        executable: String,
        args: Vec<String>,
        cwd: Option<String>,
        timeout_ms: u64,
    },
    ListCommandHistory,

    // Port Manager (§109, Stage 2)
    CheckPort {
        port: u16,
    },

    // Runtime Manager (§9–10, §20–21, Stage 3)
    ListRuntimeCatalog,
    RefreshRuntimeCatalog {
        id: String,
    },
    InstallRuntime {
        id: String,
        version: String,
    },
    RemoveRuntime {
        id: String,
        version: String,
    },

    // Project Manager + Environment Resolver (§18, §40, §42–43, Stage 4)
    RegisterProject {
        path: String,
    },
    /// Registers every project folder found as an immediate child of `path` (not
    /// recursive) — for a workspace directory holding several unrelated projects,
    /// which don't need to share a parent beyond that one scan point.
    ScanAndRegisterProjects {
        path: String,
    },
    ListProjects,
    /// Laragon-style: give every project in the remembered folders a `<name>.test` domain.
    SyncAutoDomains,
    RemoveProject {
        id: String,
    },
    GetProjectDetail {
        id: String,
    },
    /// Runtime-aware terminal (§19): runs `runtime_id`'s resolved binary for this
    /// project, with its bin dir first on PATH, and streams output like any other
    /// managed process (reuses the Process Supervisor — same live-output UI).
    RunInProject {
        project_id: String,
        runtime_id: String,
        args: Vec<String>,
    },

    // Service Manager (§22, §31, §61–68, Stage 5)
    ListServices,
    /// Custom services (§67): the user's own programs, managed like the built-in ones.
    ListCustomServices,
    SaveCustomService {
        service: crate::custom_service::CustomService,
    },
    RemoveCustomService {
        id: String,
    },
    StartService {
        id: String,
    },
    StopService {
        id: String,
    },

    // Secrets Manager (§104, §141, Stage 5)
    SetSecret {
        key: String,
        value: String,
    },
    GetSecret {
        key: String,
    },
    DeleteSecret {
        key: String,
    },

    // External DB GUI tools (§102, Stage 5)
    ListDbTools,
    OpenDbTool {
        id: String,
    },

    // Custom install locations (Stage 5, user-requested): point OpenLocalServer at a tool or
    // runtime version it didn't find/install itself, instead of only ever offering a
    // download. `label` is a version string for runtimes ("8.1"), empty for single-path
    // tools (heidisql/pgadmin).
    SetCustomInstall {
        id: String,
        label: String,
        path: String,
    },
    RemoveCustomInstall {
        id: String,
        label: String,
    },
    ListCustomInstalls,
    /// Register every PHP install found under `dir` (see `php::scan_folder`).
    ScanPhpFolder {
        dir: String,
    },
    /// Extensions for one installed PHP version (managed "8.4.26" or a custom label).
    ListPhpExtensions {
        version: String,
    },
    SetPhpExtension {
        version: String,
        name: String,
        enabled: bool,
    },
    /// Download a PECL extension build matching that PHP and enable it.
    InstallPhpExtension {
        version: String,
        name: String,
    },
    ListPeclPackages,

    // ---- Stage 6: domains, HTTPS, web server -------------------------------------
    GetWebStatus,
    GetWebConfig,
    ListDomains,
    GetDomain {
        hostname: String,
    },
    AddDomain {
        domain: Domain,
    },
    UpdateDomain {
        domain: Domain,
    },
    RemoveDomain {
        hostname: String,
    },
    SetDomainEnabled {
        hostname: String,
        enabled: bool,
    },
    DuplicateDomain {
        hostname: String,
        new_hostname: String,
    },
    RenameDomain {
        hostname: String,
        new_hostname: String,
    },
    /// §48 templates: `{project}.test`, `api.{project}.test`, ...
    SuggestDomain {
        project_id: String,
        template: String,
    },
    /// §28 apply pipeline. `overwrite` lists hosts whose hand-edited file may be replaced (§27).
    ApplyWeb {
        overwrite: Vec<String>,
    },
    StopWeb,
    ValidateWeb,
    RestartSiteApp {
        hostname: String,
    },
    SyncHosts,
    GetCaInfo,
    TrustCa,
    UntrustCa,
    ListCertificates,
    RegenerateCertificate {
        hostname: String,
    },
    RevokeCertificate {
        hostname: String,
    },
    HealthCheck {
        hostname: String,
    },

    // ---- Stage 9: config management ---------------------------------------------
    ListWebConfigs,
    ReadWebConfig {
        hostname: Option<String>,
        part: ConfigPart,
    },
    WriteWebConfig {
        hostname: String,
        part: ConfigPart,
        content: String,
    },
    SetOwnership {
        hostname: String,
        ownership: Ownership,
    },
    ListConfigHistory {
        hostname: String,
    },
    ReadConfigHistory {
        hostname: String,
        id: String,
    },
    RestoreConfigHistory {
        hostname: String,
        id: String,
    },
    ExportWebConfig {
        hostname: String,
        part: ConfigPart,
        dest: String,
    },

    // ---- Stage 10: more databases ------------------------------------------------
    CreateDatabase {
        engine: String,
        name: String,
    },
    ListDatabases {
        engine: String,
    },
    CreateDbUser {
        engine: String,
        user: String,
        password: String,
        database: String,
    },
    ListDbUsers {
        engine: String,
    },
    GetConnectionInfo {
        engine: String,
        database: Option<String>,
        path: Option<String>,
    },
    /// SQL dump backups (§32, §34): MariaDB and PostgreSQL.
    BackupDatabase {
        engine: String,
        database: String,
    },
    ListDbBackups {
        engine: String,
        database: Option<String>,
    },
    /// Loads a backup into `database`, after a safety backup of what is there.
    RestoreDatabase {
        engine: String,
        database: String,
        file: String,
    },
    DeleteDbBackup {
        engine: String,
        file: String,
    },
    ListSqlite,
    DetectSqlite {
        project_id: String,
    },
    CreateSqlite {
        path: String,
        project_id: Option<String>,
    },
    AssociateSqlite {
        path: String,
        project_id: Option<String>,
    },
    ForgetSqlite {
        path: String,
    },
    BackupSqlite {
        path: String,
    },
    RestoreSqlite {
        path: String,
        backup: String,
    },
    CheckSqlite {
        path: String,
    },
    ListExternalTools,
    SaveExternalTool {
        tool: ExternalTool,
    },
    RemoveExternalTool {
        id: String,
    },
    OpenDatabase {
        engine: String,
        database: Option<String>,
        path: Option<String>,
        tool_id: Option<String>,
    },

    // ---- Stage 7: Quick Apps + Quick Commands -----------------------------------
    ListQuickApps,
    GetQuickApp {
        id: String,
    },
    SaveQuickApp {
        yaml: String,
    },
    DuplicateQuickApp {
        id: String,
        new_id: String,
        new_name: String,
    },
    DeleteQuickApp {
        id: String,
    },
    FavoriteQuickApp {
        id: String,
        favorite: bool,
    },
    ExportQuickApp {
        id: String,
        dest: String,
    },
    ImportQuickApp {
        source: String,
    },
    TrustQuickAppSource {
        origin: String,
    },
    PlanQuickApp {
        id: String,
        values: BTreeMap<String, String>,
    },
    /// `approval`: "once" or "source" — required for untrusted (imported) apps (§139).
    /// `allow_elevated`: the separate confirmation for administrator steps (§92).
    StartQuickApp {
        id: String,
        values: BTreeMap<String, String>,
        approval: Option<String>,
        allow_elevated: bool,
    },
    GetQuickRun {
        id: String,
    },
    ListQuickRuns,
    CancelQuickRun {
        id: String,
    },
    ListQuickCommands,
    SaveQuickCommand {
        command: QuickCommand,
    },
    DeleteQuickCommand {
        id: String,
    },
    RunQuickCommand {
        id: String,
        project_id: Option<String>,
    },
    RunCommandLine {
        line: String,
        cwd: Option<String>,
        project_id: Option<String>,
    },
    ListHistory,
    DeleteHistory {
        id: u64,
    },
    ClearHistory,
    SaveHistoryAsQuickCommand {
        id: u64,
        command_id: String,
        name: String,
    },

    // ---- Stage 8: dashboard, logs, startup, project actions ----------------------
    GetDashboard,
    GetEnvironmentHealth,
    ListLogSources,
    ReadLog {
        source: String,
        max_lines: usize,
    },
    ExportLog {
        source: String,
        dest: String,
    },
    ClearLog {
        source: String,
    },
    GetStartupSettings,
    SetStartupSettings {
        settings: StartupSettings,
    },
    OpenPath {
        path: String,
    },
    OpenUrl {
        url: String,
    },
    OpenInEditor {
        path: String,
    },
    /// "Open with" (§97): `app` is `editor`, an editor id like `vscode`, `explorer`, `terminal` or `default`.
    OpenWith {
        path: String,
        app: String,
    },
    /// The project's usual places: folder, public/, config/, .env, logs, site server config (§100).
    ListProjectShortcuts {
        project_id: String,
    },

    // ---- Interactive terminal (§19) ------------------------------------------------
    /// A shell in the project's folder with its runtimes on PATH. `shell`: `powershell` or `cmd`.
    OpenTerminal {
        project_id: Option<String>,
        shell: Option<String>,
        rows: u16,
        cols: u16,
    },
    TerminalInput {
        id: u32,
        data: String,
    },
    ResizeTerminal {
        id: u32,
        rows: u16,
        cols: u16,
    },
    CloseTerminal {
        id: u32,
    },
    /// Known code editors and where each is installed (Settings picker).
    ListEditors,
    /// CPU / memory / disk for the machine and for every managed process tree.
    GetSystemStats,
    /// Old Laragon / XAMPP / WampServer database servers found on this PC.
    ListMigrationSources,
    ListForeignDatabases {
        source_id: String,
        password: String,
    },
    /// Copy databases (all when `databases` is empty) into our `target` engine.
    MigrateDatabases {
        source_id: String,
        password: String,
        databases: Vec<String>,
        target: String,
    },
    /// Where the running scan or import is up to (poll while it runs).
    GetMigrationProgress,
    /// The admin helper service: status, install (one UAC prompt), remove.
    GetHelperService,
    InstallHelperService,
    UninstallHelperService,

    // ---- Stage 11: runtime depth + diagnostics -----------------------------------
    /// Xdebug state and settings for one PHP version (§13).
    GetXdebug {
        version: String,
    },
    SetXdebug {
        version: String,
        settings: XdebugSettings,
    },
    /// IDE setup text for a project: `ide` is "vscode", "phpstorm" or "other".
    XdebugIdeConfig {
        project_id: String,
        ide: String,
        version: String,
    },
    /// Every command the project's artisan / bin/console / composer / package.json /
    /// manage.py offers, with arguments and options where the tool describes them.
    DiscoverCommands {
        project_id: String,
    },
    /// composer.json / composer.lock read for a project (§14).
    GetComposerInfo {
        project_id: String,
    },
    /// One of the named Composer actions (see `composer::command_args`).
    RunComposer {
        project_id: String,
        action: String,
        target: Option<String>,
    },
    /// Which Node package managers a project uses and can run (§15).
    GetPackageManagers {
        project_id: String,
    },
    /// Switch on pnpm or yarn through corepack.
    EnablePackageManager {
        project_id: String,
        manager: String,
    },
    /// Python virtual environment of a project (§17).
    GetVenv {
        project_id: String,
    },
    CreateVenv {
        project_id: String,
        recreate: bool,
    },
    InstallVenvRequirements {
        project_id: String,
        what: String,
    },
    /// DiagnosticEngine v1 (§112).
    RunDiagnostics,
    IgnoreDiagnostic {
        id: String,
        ignore: bool,
    },
    RestartService {
        id: String,
    },

    // ---- .env editor (§103) ------------------------------------------------------
    ListEnvFiles {
        project_id: String,
    },
    ReadEnvFile {
        project_id: String,
        file: String,
    },
    SaveEnvFile {
        project_id: String,
        file: String,
        content: String,
    },
    SetEnvValue {
        project_id: String,
        file: String,
        key: String,
        value: String,
    },
    DeleteEnvKey {
        project_id: String,
        file: String,
        key: String,
    },
    CompareEnvFiles {
        project_id: String,
        a: String,
        b: String,
    },
    /// `mode` is "merge" (keep this file's other keys) or "replace".
    ImportEnvFile {
        project_id: String,
        file: String,
        source: String,
        mode: String,
    },
    ExportEnvFile {
        project_id: String,
        file: String,
        dest: String,
    },
    /// A new env file, copied from `from` when given (e.g. `.env` from `.env.example`).
    CreateEnvFile {
        project_id: String,
        file: String,
        from: Option<String>,
    },

    // ---- Operation journal (§78, §163) ---------------------------------------------
    ListOperations,
    /// Forget an interrupted operation once it has been dealt with.
    DismissOperation {
        id: u64,
    },

    // ---- Mailpit integration (§63, §66) ------------------------------------------
    /// What pointing `file` at Mailpit would change, before anything is written.
    MailpitEnvPlan {
        project_id: String,
        file: String,
    },
    ApplyMailpitEnv {
        project_id: String,
        file: String,
    },
    /// The mail checklist; with a project it also checks that project's `.env`.
    MailDiagnostics {
        project_id: Option<String>,
    },
    SendTestMail {
        to: String,
    },

    // ---- Stage 12: manifests, reproducible setup (§71–78) -------------------------
    /// The project's manifest files (or, without one, what detection suggests).
    GetManifest {
        project_id: String,
    },
    /// Writes `.openlocalserver/environment.yaml`; the derived manifest when `manifest` is null.
    SaveManifest {
        project_id: String,
        manifest: Option<crate::manifest::EnvironmentManifest>,
    },
    /// Writes hand-edited manifest YAML after checking that it parses.
    SaveManifestText {
        project_id: String,
        text: String,
    },
    PlanSetup {
        project_id: String,
    },
    /// §73; `dry_run` changes nothing (§77).
    ApplySetup {
        project_id: String,
        dry_run: bool,
    },
    GetSetupProgress,
    /// Preview or import a project's Procfile/Procfile.dev.
    ImportProcfile {
        project_id: String,
        dry_run: bool,
    },
    ReadSiteFile {
        hostname: String,
        name: String,
    },
    WriteSiteFile {
        hostname: String,
        name: String,
        content: String,
    },

    // ---- Stage 13: profiles, modes, workers, scheduler, snapshots ------------------
    ListProfiles,
    SaveProfile {
        profile: crate::profiles::Profile,
    },
    DeleteProfile {
        id: String,
    },
    ExportProfile {
        id: String,
        dest: String,
    },
    /// Reads a profile file for review; `ImportProfile` then saves it.
    ReadProfileFile {
        source: String,
    },
    ImportProfile {
        source: String,
    },
    ApplyProfile {
        project_id: String,
        profile_id: String,
    },
    ProfileFromProject {
        project_id: String,
        name: String,
    },
    /// A profile as YAML, for the editor; `SaveProfileYaml` checks and saves it.
    ProfileYaml {
        id: String,
    },
    SaveProfileYaml {
        yaml: String,
    },
    GetProjectModes {
        project_id: String,
    },
    SetProjectMode {
        project_id: String,
        mode: String,
    },
    ListWorkers {
        project_id: Option<String>,
    },
    ListWorkerPresets,
    SaveWorker {
        worker: crate::workers::Worker,
    },
    RemoveWorker {
        id: String,
    },
    StartWorker {
        id: String,
    },
    StopWorker {
        id: String,
    },
    RestartWorker {
        id: String,
    },
    StartProjectWorkers {
        project_id: String,
    },
    StopProjectWorkers {
        project_id: String,
    },
    ListSchedules {
        project_id: Option<String>,
    },
    SaveSchedule {
        task: crate::scheduler::ScheduledTask,
    },
    RemoveSchedule {
        id: String,
    },
    RunScheduleNow {
        id: String,
    },
    /// Checks a schedule and says it in words ("every 5 minutes").
    DescribeSchedule {
        schedule: String,
    },
    ListSnapshots {
        project_id: String,
    },
    CreateSnapshot {
        project_id: String,
        label: String,
        options: crate::snapshots::SnapshotOptions,
    },
    DeleteSnapshot {
        project_id: String,
        id: String,
    },
    RestoreSnapshot {
        project_id: String,
        id: String,
        options: crate::snapshots::RestoreOptions,
    },
    ExportSnapshot {
        project_id: String,
        id: String,
        dest: String,
    },
    PreviewImport {
        source: String,
    },
    ImportEnvironment {
        source: String,
        target: String,
        name: String,
    },
    /// `what`: full, infrastructure or configuration (§158).
    CloneEnvironment {
        project_id: String,
        target: String,
        name: String,
        what: String,
    },
    BackupSettings,
    ListSettingsBackups,
    RestoreSettings {
        id: String,
    },
    GetResourceLimits,
    SetResourceLimits {
        limits: crate::resources::ResourceLimits,
    },

    // ---- Stage 14: tunnels and traffic (§56–60, §110–111) --------------------------
    ListTunnelProviders,
    ListTunnels,
    SaveTunnel {
        tunnel: crate::tunnel::TunnelConfig,
    },
    RemoveTunnel {
        id: String,
    },
    /// The first start needs `confirm_exposure` (§59).
    StartTunnel {
        id: String,
        confirm_exposure: bool,
    },
    StopTunnel {
        id: String,
    },
    CheckTunnel {
        id: String,
    },
    TunnelLog {
        id: String,
    },
    SetTunnelToken {
        provider: String,
        token: Option<String>,
    },
    SetTunnelPassword {
        id: String,
        password: Option<String>,
    },
    ListTunnelRequests {
        id: String,
    },
    ClearTunnelRequests {
        id: String,
    },
    ReplayTunnelRequest {
        id: String,
        request_id: u64,
    },
    SendTunnelTestRequest {
        id: String,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    },

    // ---- Stage 15: search, doctor, repair, Git (§113–115, §123, §125) ---------------
    GlobalSearch {
        query: String,
    },
    Doctor,
    DiagnoseProject {
        project_id: String,
    },
    /// §114: what would be fixed, before anything is.
    PlanRepair {
        project_id: Option<String>,
    },
    /// Applies the chosen fixes (all when `ids` is empty); destructive ones need `confirm_destructive`.
    ApplyRepair {
        project_id: Option<String>,
        ids: Vec<String>,
        confirm_destructive: bool,
    },
    GitStatus {
        project_id: String,
    },
    GitInit {
        project_id: String,
    },
    GitBranches {
        project_id: String,
    },
    GitCreateBranch {
        project_id: String,
        name: String,
        checkout: bool,
    },
    GitSwitchBranch {
        project_id: String,
        name: String,
    },
    GitDeleteBranch {
        project_id: String,
        name: String,
        force: bool,
    },
    /// All changes when `paths` is empty.
    GitStage {
        project_id: String,
        paths: Vec<String>,
    },
    GitUnstage {
        project_id: String,
        paths: Vec<String>,
    },
    GitDiscard {
        project_id: String,
        paths: Vec<String>,
    },
    GitCommit {
        project_id: String,
        message: String,
        amend: bool,
    },
    /// `action`: pull, push or fetch.
    GitSync {
        project_id: String,
        action: String,
        remote: Option<String>,
    },
    GitDiff {
        project_id: String,
        path: String,
        staged: bool,
    },
    GitLog {
        project_id: String,
        limit: usize,
    },
    GitShow {
        project_id: String,
        hash: String,
    },
    GitAddRemote {
        project_id: String,
        name: String,
        url: String,
    },
    GitRemoveRemote {
        project_id: String,
        name: String,
    },
    /// `action`: push, pop, apply or drop.
    GitStash {
        project_id: String,
        action: String,
        message: Option<String>,
        index: Option<u32>,
    },
    GitAddIgnore {
        project_id: String,
        template: String,
    },
    /// Saves (or with no token, forgets) HTTPS credentials for a Git host.
    GitSetCredentials {
        host: String,
        username: String,
        token: Option<String>,
    },
    /// Returns the file paths of SSH private keys found in the current user's `~/.ssh` directory.
    ListSshKeys,
    GitClone {
        url: String,
        target: String,
        branch: Option<String>,
        auth: Option<crate::git::GitAuth>,
    },

    // ---- Stage 16: plugins and signed catalogs (§133–135, §87–88) ----------------------
    ListPlugins,
    /// From a folder or a .zip. It arrives switched off.
    InstallPlugin {
        source: String,
    },
    /// Turning on needs `approve` to list exactly the permissions the plugin declares.
    SetPluginEnabled {
        id: String,
        enabled: bool,
        approve: Vec<String>,
    },
    RemovePlugin {
        id: String,
    },
    PluginDetect {
        project_id: String,
    },
    ListCatalogSources,
    AddCatalogSource {
        name: String,
        url: String,
        public_key: String,
    },
    RemoveCatalogSource {
        id: String,
    },
    RefreshCatalogs {
        id: Option<String>,
    },
    InstallCatalogPlugin {
        source_id: String,
        plugin_id: String,
    },

    // ---- Stage 17: release hardening (§124, §128, §137, §145, diagnostics) ---------------
    GetApiStatus,
    SetApiSettings {
        enabled: bool,
        port: u16,
        mode: String,
    },
    /// Returns the new token once; only its hash is kept.
    RotateApiToken,
    ClearApiToken,
    GetUpdaterStatus,
    SetUpdaterSettings {
        endpoint: String,
        public_key: String,
    },
    CheckUpdate,
    DownloadUpdate,
    InstallUpdate,
    GetShellMenu,
    InstallShellMenu,
    RemoveShellMenu,
    CheckNetwork {
        force: bool,
    },
    ExportSupportBundle {
        dest: String,
    },

    // ---- Stage 18: load testing with k6 ----------------------------------------------
    LoadOverview {
        project_id: String,
    },
    LoadReadScript {
        project_id: String,
        name: String,
    },
    LoadSaveScript {
        project_id: String,
        name: String,
        content: String,
    },
    LoadDeleteScript {
        project_id: String,
        name: String,
    },
    /// `kind`: smoke, load or spike. Returns the new script's file name.
    LoadListProfiles,
    LoadSaveProfile {
        profile: crate::loadtest::LoadProfile,
    },
    LoadDeleteProfile {
        id: String,
    },
    /// Writes the plan's script into the project (`name` defaults to the plan's id); returns the file name.
    LoadGenerate {
        project_id: String,
        profile: crate::loadtest::LoadProfile,
        name: Option<String>,
    },
    /// `target` is a site's hostname (default: the project's first site). A public tunnel needs `confirm_public`.
    LoadRun {
        project_id: String,
        script: String,
        target: Option<String>,
        confirm_public: bool,
        env: Vec<(String, String)>,
    },
    LoadStatus {
        run_id: String,
    },
    LoadStop {
        run_id: String,
    },
    LoadRuns {
        project_id: String,
    },
    LoadDeleteRun {
        project_id: String,
        run_id: String,
    },

    // ---- Stage 19: AI assistant (bring your own model) ---------------------------------
    AiGetState,
    AiSaveSettings {
        enabled: bool,
        features: BTreeMap<String, String>,
    },
    /// `api_key`: none keeps the stored key, an empty string removes it. The key goes to the Secrets Manager.
    AiSaveProvider {
        provider: crate::ai::AiProvider,
        api_key: Option<String>,
    },
    AiRemoveProvider {
        id: String,
    },
    AiDetectLocal,
    AiTest {
        provider_id: String,
    },
    AiModels {
        provider_id: String,
    },
    /// The models a provider as typed in the form offers (not saved yet); no prompt is sent.
    AiProbe {
        provider: crate::ai::AiProvider,
        api_key: Option<String>,
    },
    /// "Show what will be sent": the redacted prompt; nothing is sent.
    AiPreview {
        request: crate::ai::AiRequest,
    },
    /// Starts a request. A provider outside this computer needs `confirm_remote`.
    AiStart {
        request: crate::ai::AiRequest,
        confirm_remote: bool,
    },
    AiJob {
        job_id: String,
    },
    AiCancel {
        job_id: String,
    },
    /// Runs the steps of a plan the user approved (each re-checked against the allowlist).
    AiApply {
        actions: Vec<CoreCommand>,
        confirm_destructive: bool,
    },
    // @@commands-end
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub services: Vec<ServiceStatus>,
    pub web: WebStatus,
    pub domains: Vec<DomainSummary>,
    pub project_count: usize,
    pub health: Vec<HealthItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse {
    Pong {
        version: String,
    },
    Setting {
        key: String,
        value: Option<Value>,
    },
    Ok,
    ProcessStarted {
        id: ProcessId,
    },
    Processes {
        processes: Vec<ProcessInfo>,
    },
    ProcessOutput {
        id: ProcessId,
        lines: Vec<String>,
    },
    CommandResult {
        entry: CommandHistoryEntry,
    },
    CommandHistory {
        entries: Vec<CommandHistoryEntry>,
    },
    Port {
        port: u16,
        status: PortStatus,
    },
    RuntimeCatalog {
        entries: Vec<CatalogEntry>,
    },
    Project {
        project: Project,
    },
    Projects {
        projects: Vec<Project>,
    },
    ProjectDetail {
        detail: Box<ProjectDetail>,
    },
    Services {
        services: Vec<ServiceStatus>,
    },
    CustomServices {
        services: Vec<crate::custom_service::CustomService>,
    },
    CustomService {
        service: Box<crate::custom_service::CustomService>,
    },
    Secret {
        key: String,
        value: Option<String>,
    },
    DbTools {
        tools: Vec<DbTool>,
    },
    CustomInstalls {
        entries: Vec<CustomInstall>,
    },
    PhpScan {
        found: Vec<crate::php::ScannedPhp>,
    },
    PhpExtensions {
        report: crate::php::PhpExtensions,
    },

    WebStatus {
        status: Box<WebStatus>,
    },
    WebConfig {
        config: WebConfig,
    },
    Domains {
        domains: Vec<DomainSummary>,
    },
    Domain {
        domain: Box<Domain>,
    },
    Text {
        text: String,
    },
    /// One entry per web server that was applied; each renders only its own sites.
    Applied {
        reports: Vec<ApplyReport>,
    },
    CaInfo {
        info: CaInfo,
    },
    Certificates {
        certs: Vec<CertInfo>,
    },
    Health {
        report: HealthReport,
    },
    Configs {
        files: Vec<ConfigFile>,
    },
    ConfigVersions {
        versions: Vec<ConfigVersion>,
    },

    Names {
        names: Vec<String>,
    },
    DbUsers {
        users: Vec<DbUser>,
    },
    Connection {
        info: ConnectionInfo,
    },
    DbBackups {
        backups: Vec<crate::dbbackup::DbBackup>,
    },
    SqliteList {
        databases: Vec<SqliteInfo>,
    },
    SqliteInfo {
        info: SqliteInfo,
    },
    Integrity {
        result: IntegrityResult,
    },
    ExternalTools {
        tools: Vec<ExternalTool>,
    },

    QuickApps {
        apps: Vec<EntryView>,
    },
    QuickApp {
        detail: Box<EntryDetail>,
    },
    QuickPlan {
        result: Box<QuickPlanResult>,
    },
    QuickRunStarted {
        run_id: String,
    },
    QuickRun {
        run: Box<RunView>,
    },
    QuickRuns {
        runs: Vec<RunView>,
    },
    QuickCommands {
        commands: Vec<QuickCommand>,
    },
    History {
        entries: Vec<HistoryEntry>,
    },
    MaybeProcess {
        id: Option<ProcessId>,
    },

    Dashboard {
        data: Box<DashboardData>,
    },
    EnvironmentHealth {
        items: Vec<HealthItem>,
    },
    LogSources {
        sources: Vec<LogSource>,
    },
    LogLines {
        source: String,
        lines: Vec<String>,
    },
    Shortcuts {
        shortcuts: Vec<crate::shortcuts::Shortcut>,
    },
    Terminal {
        id: u32,
    },
    Startup {
        settings: StartupSettings,
    },
    Count {
        count: usize,
    },
    HelperService {
        installed: bool,
    },
    SystemStats {
        stats: Box<crate::monitor::SystemStats>,
    },
    MigrationSources {
        sources: Vec<crate::migrate::MigrationSource>,
    },
    Migrated {
        results: Vec<crate::migrate::MigratedDb>,
    },
    MigrationProgress {
        progress: crate::migrate::MigrationProgress,
    },
    Editors {
        editors: Vec<crate::editors::EditorInfo>,
    },
    Xdebug {
        report: XdebugReport,
    },
    Composer {
        info: Box<crate::composer::ComposerInfo>,
    },
    PackageManagers {
        info: crate::nodepm::PackageManagerInfo,
    },
    CommandSources {
        sources: Vec<crate::command_catalog::CommandSource>,
    },
    Venv {
        info: crate::venv::VenvInfo,
    },
    Diagnostics {
        findings: Vec<crate::diagnostics::Finding>,
    },
    EnvFiles {
        files: Vec<crate::envfile::EnvFileInfo>,
    },
    EnvFile {
        view: Box<crate::envfile::EnvFileView>,
    },
    EnvCompare {
        rows: Vec<crate::envfile::EnvDiffRow>,
    },
    MailEnvPlan {
        plan: Box<crate::mail::MailEnvPlan>,
    },
    Operations {
        operations: Vec<crate::journal::Operation>,
    },
    MailChecks {
        checks: Vec<crate::mail::MailCheck>,
    },

    ManifestInfo {
        info: Box<crate::setup::ManifestInfo>,
    },
    Manifest {
        manifest: Box<crate::manifest::EnvironmentManifest>,
    },
    SetupPlan {
        plan: Box<crate::setup::EnvironmentPlan>,
    },
    Setup {
        report: Box<crate::setup::SetupReport>,
    },
    SetupProgress {
        report: Option<Box<crate::setup::SetupReport>>,
    },
    Procfile {
        preview: Box<crate::procfile::ProcfilePreview>,
    },
    Profiles {
        profiles: Vec<crate::profiles::Profile>,
    },
    Profile {
        profile: Box<crate::profiles::Profile>,
    },
    Modes {
        view: crate::profiles::ModesView,
    },
    ModeResult {
        result: crate::profiles::ModeResult,
    },
    Workers {
        workers: Vec<crate::workers::WorkerStatus>,
    },
    WorkerPresets {
        presets: Vec<crate::workers::WorkerPreset>,
    },
    Schedules {
        tasks: Vec<crate::scheduler::TaskStatus>,
    },
    TaskRun {
        run: crate::scheduler::TaskRun,
    },
    Snapshots {
        snapshots: Vec<crate::snapshots::SnapshotInfo>,
    },
    Snapshot {
        snapshot: Box<crate::snapshots::SnapshotInfo>,
    },
    Restored {
        result: crate::snapshots::RestoreResult,
    },
    ImportPreview {
        preview: Box<crate::snapshots::ImportPreview>,
    },
    Cloned {
        result: Box<crate::snapshots::CloneResult>,
    },
    SettingsBackups {
        backups: Vec<crate::snapshots::SettingsBackup>,
    },
    SettingsBackup {
        backup: crate::snapshots::SettingsBackup,
    },
    Resources {
        limits: crate::resources::ResourceLimits,
    },
    TunnelProviders {
        providers: Vec<crate::tunnel::ProviderInfo>,
    },
    Tunnels {
        tunnels: Vec<crate::tunnel::TunnelStatus>,
    },
    Tunnel {
        tunnel: Box<crate::tunnel::TunnelStatus>,
    },
    Lines {
        lines: Vec<String>,
    },
    TunnelRequests {
        requests: Vec<crate::inspector::RecordedRequest>,
    },
    TunnelRequest {
        request: Box<crate::inspector::RecordedRequest>,
    },

    SearchResults {
        hits: Vec<crate::search::SearchHit>,
    },
    DoctorReport {
        report: Box<crate::repair::DoctorReport>,
    },
    RepairPlan {
        plan: Box<crate::repair::RepairPlan>,
    },
    RepairReport {
        report: Box<crate::repair::RepairReport>,
    },
    GitStatus {
        status: Box<crate::git::GitStatus>,
    },
    GitBranches {
        branches: Vec<crate::git::Branch>,
    },
    GitCommits {
        commits: Vec<crate::git::Commit>,
    },
    GitCommit {
        commit: crate::git::Commit,
    },
    GitResult {
        result: crate::git::GitResult,
    },
    /// Paths to SSH private key files found in `~/.ssh`.
    SshKeys {
        keys: Vec<String>,
    },

    Plugins {
        plugins: Vec<crate::plugin::PluginInfo>,
    },
    Plugin {
        plugin: Box<crate::plugin::PluginInfo>,
    },
    PluginDetections {
        detections: Vec<crate::plugin::PluginDetection>,
    },
    CatalogSources {
        catalogs: Vec<crate::catalogs::CatalogView>,
    },

    ApiStatus {
        status: Box<crate::api::ApiStatus>,
    },
    UpdaterStatus {
        status: Box<crate::updater::UpdaterStatus>,
    },
    Update {
        update: Box<crate::updater::UpdateInfo>,
    },
    ShellMenu {
        status: crate::shell_menu::ShellMenuStatus,
    },
    Network {
        status: Box<crate::network::NetworkStatus>,
    },

    LoadOverview {
        overview: Box<crate::loadtest::LoadOverview>,
    },
    LoadRun {
        run: Box<crate::loadtest::LoadRun>,
    },
    LoadRuns {
        runs: Vec<crate::loadtest::LoadRun>,
    },
    LoadProfiles {
        profiles: Vec<crate::loadtest::LoadProfile>,
    },

    AiState {
        state: Box<crate::ai::AiState>,
    },
    AiDetected {
        servers: Vec<crate::ai::AiDetected>,
    },
    AiTest {
        result: crate::ai::AiTestResult,
    },
    AiModels {
        models: Vec<crate::ai::AiModel>,
    },
    AiPrompt {
        prompt: Box<crate::ai::AiPrompt>,
    },
    AiJob {
        job: Box<crate::ai::AiJobView>,
    },
    AiApplied {
        steps: Vec<crate::repair::RepairStep>,
    },
    // @@responses-end
}

/// A cheap handle onto the shared application state. Cloning shares everything.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

impl Core {
    pub fn new(settings: SettingsService, paths: AppPaths) -> Self {
        let inner = Inner::new(settings, paths).expect("failed to start the application core");
        inner.apply_api();
        Self { inner }
    }

    /// Used by the Tauri shell so it can share the supervisor/runtime managers it forwards
    /// events from with the core.
    pub fn with_parts(
        settings: SettingsService,
        paths: AppPaths,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
    ) -> Self {
        let inner = Inner::with_parts(settings, paths, supervisor, runtimes)
            .expect("failed to start the application core");
        inner.apply_api();
        Self { inner }
    }

    /// A handle onto an existing core, for the local API's server thread.
    pub fn from_inner(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }
    pub fn supervisor(&self) -> Arc<ProcessSupervisor> {
        self.inner.supervisor.clone()
    }
    pub fn runtimes(&self) -> Arc<RuntimeManager> {
        self.inner.runtimes.clone()
    }
    pub fn services(&self) -> Arc<ServiceManager> {
        self.inner.services.clone()
    }

    /// Route a `CoreCommand` to the right manager and return its result.
    /// Every failure is converted to a `Diagnostic` before it reaches the caller.
    pub fn dispatch(&self, command: CoreCommand) -> Result<CoreResponse, Diagnostic> {
        self.dispatch_inner(command)
            .map_err(|e| Diagnostic::from(&e))
    }

    fn dispatch_inner(&self, command: CoreCommand) -> Result<CoreResponse, CoreError> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        match command {
            C::Ping => Ok(R::Pong {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            C::GetSetting { key } => {
                let value = if key == "paths.sites_dir" {
                    Some(serde_json::Value::String(
                        i.sites_dir().display().to_string(),
                    ))
                } else {
                    i.settings.lock().unwrap().get(&key).cloned()
                };
                Ok(R::Setting { value, key })
            }
            C::SetSetting { key, value } => {
                // Redact before it ever reaches the log, per §141 — settings can hold secrets.
                let logged_value = crate::logging::redact_value(&key, &value.to_string());
                tracing::info!(command = "set_setting", key = %key, value = %logged_value);
                if let Some(id) = key
                    .strip_prefix("runtime.")
                    .and_then(|k| k.strip_suffix(".global"))
                {
                    if matches!(id, "nginx" | "apache" | "caddy") && i.web.is_running() {
                        return Err(CoreError::ServiceError(
                            "Stop the web server before changing its runtime version.".into(),
                        ));
                    }
                    if matches!(
                        id,
                        "mariadb" | "postgres" | "mongodb" | "redis" | "memcached" | "mailpit"
                    ) && i.services.is_running(id)
                    {
                        return Err(CoreError::ServiceError(format!(
                            "Stop {id} before changing its runtime version."
                        )));
                    }
                }
                i.settings.lock().unwrap().set(key.clone(), value.clone())?;
                if let (Some(id), Some(version)) = (
                    key.strip_prefix("runtime.")
                        .and_then(|k| k.strip_suffix(".global")),
                    value.as_str(),
                ) {
                    // Keep unmanaged/custom PHP and Node preferences in settings, while only
                    // applying managed catalog versions to the runtime manager.
                    let _ = i.runtimes.set_preferred(id, version);
                }
                Ok(R::Ok)
            }

            C::StartProcess { spec } => {
                tracing::info!(command = "start_process", name = %spec.name);
                Ok(R::ProcessStarted {
                    id: i.supervisor.start(spec),
                })
            }
            C::StopProcess { id } => {
                i.supervisor.stop(id);
                Ok(R::Ok)
            }
            C::ListProcesses => Ok(R::Processes {
                processes: i.supervisor.snapshot(),
            }),
            C::GetProcessOutput { id } => Ok(R::ProcessOutput {
                id,
                lines: i.supervisor.recent_output(id),
            }),

            C::RunCommand {
                executable,
                args,
                cwd,
                timeout_ms,
            } => {
                tracing::info!(command = "run_command", executable = %executable);
                let entry = i.supervisor.run_to_completion(
                    &executable,
                    &args,
                    cwd.as_deref(),
                    Duration::from_millis(timeout_ms),
                );
                Ok(R::CommandResult { entry })
            }
            C::ListCommandHistory => Ok(R::CommandHistory {
                entries: i.supervisor.history(),
            }),

            C::CheckPort { port: p } => Ok(R::Port {
                port: p,
                status: port::check_port(p),
            }),

            C::ListRuntimeCatalog => Ok(R::RuntimeCatalog {
                entries: i.runtimes.catalog(),
            }),
            C::RefreshRuntimeCatalog { id } => {
                i.runtimes
                    .refresh_online_catalog(&id)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::RuntimeCatalog {
                    entries: i.runtimes.catalog(),
                })
            }
            C::InstallRuntime { id, version } => {
                tracing::info!(command = "install_runtime", id = %id, version = %version);
                i.runtimes.install(&id, &version);
                Ok(R::Ok)
            }
            C::RemoveRuntime { id, version } => {
                if i.services.is_running(&id) {
                    return Err(CoreError::ServiceError(format!(
                        "Stop {id} before removing a version."
                    )));
                }
                if matches!(id.as_str(), "nginx" | "apache" | "caddy") && i.web.is_running() {
                    return Err(CoreError::ServiceError(
                        "Stop the web server before removing a server version.".into(),
                    ));
                }
                if id == "php" && i.web.is_running() {
                    return Err(CoreError::ServiceError(
                        "Stop the web server before removing a PHP version.".into(),
                    ));
                }
                let was_default =
                    i.runtimes.catalog().iter().any(|entry| {
                        entry.id == id && entry.version == version && entry.is_default
                    });
                let last_version = i.runtimes.installed_versions(&id).len() == 1;
                let pref_key = format!("runtime.{id}.global");
                let old_pref = if was_default && last_version {
                    let mut settings = i.settings.lock().unwrap();
                    let old = settings.get(&pref_key).cloned();
                    settings.set(pref_key.clone(), serde_json::Value::Null)?;
                    old
                } else {
                    None
                };
                if let Err(error) = i.runtimes.remove(&id, &version) {
                    if was_default && last_version {
                        let mut settings = i.settings.lock().unwrap();
                        if let Some(old) = old_pref {
                            let _ = settings.set(pref_key, old);
                        } else {
                            let _ = settings.set(pref_key, serde_json::Value::Null);
                        }
                    }
                    return Err(CoreError::ServiceError(error));
                }
                Ok(R::Ok)
            }

            C::RegisterProject { path } => {
                tracing::info!(command = "register_project", path = %path);
                let project = i.projects.lock().unwrap().register(&path)?;
                let _ = i.clear_project_skip(&project.path);
                if let Err(e) = i.sync_auto_domains() {
                    tracing::warn!(error = %e, "automatic domains could not be synced");
                }
                Ok(R::Project { project })
            }
            C::ScanAndRegisterProjects { path } => {
                tracing::info!(command = "scan_and_register_projects", path = %path);
                let root = std::path::PathBuf::from(&path);
                if !root.is_dir() {
                    return Err(CoreError::InvalidProjectPath(path));
                }
                let candidates = crate::detection::scan_for_projects(&root);
                let mut projects = i.projects.lock().unwrap();
                let mut registered = Vec::with_capacity(candidates.len());
                for candidate in candidates {
                    if let Some(s) = candidate.to_str() {
                        // Bulk scans don't resurrect folders removed from the list.
                        if i.is_project_skipped(s) {
                            continue;
                        }
                        registered.push(projects.register(s)?);
                    }
                }
                drop(projects);
                i.remember_projects_root(&path)?;
                if let Err(e) = i.sync_auto_domains() {
                    tracing::warn!(error = %e, "automatic domains could not be synced");
                }
                Ok(R::Projects {
                    projects: registered,
                })
            }
            C::SyncAutoDomains => Ok(R::Count {
                count: i.sync_auto_domains()?,
            }),
            C::ListProjects => Ok(R::Projects {
                projects: i.projects.lock().unwrap().list(),
            }),
            C::RemoveProject { id } => {
                i.remove_project(&id)?;
                Ok(R::Ok)
            }
            C::GetProjectDetail { id } => {
                let detail = i
                    .project_detail(&id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(id.clone()))?;
                Ok(R::ProjectDetail {
                    detail: Box::new(detail),
                })
            }
            C::RunInProject {
                project_id,
                runtime_id,
                args,
            } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let detail = i
                    .project_detail(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let resolved = detail.resolved.iter().find(|r| r.id == runtime_id);

                let Some(r) = resolved.filter(|r| r.installed_version.is_some()) else {
                    return Err(CoreError::InvalidProjectPath(format!(
                        "{runtime_id} is not resolved to an installed version for this project"
                    )));
                };
                let bin_dir = r.bin_dir.clone();

                let binary = if r.source == ResolutionSource::Custom {
                    // A custom install's own path IS the binary — bin_dir above is just
                    // its parent directory (for PATH), not something to re-derive from.
                    i.custom_installs
                        .lock()
                        .unwrap()
                        .resolve(&runtime_id, r.requested_version.as_deref())
                        .map(|c| std::path::PathBuf::from(&c.path))
                        .ok_or_else(|| {
                            CoreError::InvalidProjectPath(format!(
                                "{runtime_id} custom install vanished"
                            ))
                        })?
                } else {
                    let version = r.installed_version.clone().unwrap();
                    i.runtimes
                        .binary_path(&runtime_id, &version)
                        .ok_or_else(|| {
                            CoreError::InvalidProjectPath(format!(
                                "{runtime_id} {version} binary missing on disk"
                            ))
                        })?
                };

                // Prepend the resolved runtime's own directory to PATH so a command this
                // process shells out to (e.g. `npm` calling back into `node`) also finds
                // the project-selected version, not whatever's on the system PATH (§19).
                let system_path = std::env::var("PATH").unwrap_or_default();
                let new_path = match bin_dir {
                    Some(dir) => format!("{dir};{system_path}"),
                    None => system_path,
                };
                let mut env = vec![("PATH".to_string(), new_path)];
                if runtime_id == "php" {
                    if let Some(v) = &r.installed_version {
                        if let Ok(ini) = i.php.write_ini(v) {
                            env.push(("PHPRC".into(), ini.display().to_string()));
                        }
                    }
                }
                let version_for_log = r.installed_version.clone().unwrap_or_default();
                tracing::info!(command = "run_in_project", project = %project.name, runtime = %runtime_id, version = %version_for_log);
                let id = i.supervisor.start(ProcessSpec {
                    name: format!("{}: {} {}", project.name, runtime_id, args.join(" ")),
                    executable: binary.display().to_string(),
                    args,
                    cwd: Some(project.path.clone()),
                    env,
                    restart: None,
                });
                Ok(R::ProcessStarted { id })
            }

            C::ListServices => Ok(R::Services {
                services: i.services.list(),
            }),
            C::ListCustomServices => Ok(R::CustomServices {
                services: i.services.list_custom(),
            }),
            C::SaveCustomService { service } => {
                tracing::info!(command = "save_custom_service", name = %service.name);
                Ok(R::CustomService {
                    service: Box::new(
                        i.services
                            .save_custom(service)
                            .map_err(CoreError::ServiceError)?,
                    ),
                })
            }
            C::RemoveCustomService { id } => {
                i.services
                    .remove_custom(&id)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::StartService { id } => {
                tracing::info!(command = "start_service", id = %id);
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    // A web server renders the sites assigned to it, then binds its ports.
                    i.apply_web_server(&id, &[]).map(|_| R::Ok)?;
                } else {
                    i.services.start(&id).map_err(CoreError::ServiceError)?;
                }
                Ok(R::Ok)
            }
            C::StopService { id } => {
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    i.web.stop_server(&id);
                } else {
                    i.services.stop(&id);
                }
                Ok(R::Ok)
            }

            C::SetSecret { key, value } => {
                // Never log the value itself, only that a secret was set (§141).
                tracing::info!(command = "set_secret", key = %key);
                crate::secrets::set_secret(&key, &value).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::GetSecret { key } => {
                let value = crate::secrets::get_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(R::Secret { key, value })
            }
            C::DeleteSecret { key } => {
                crate::secrets::delete_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            C::ListDbTools => {
                // A user-pinned custom path always wins over auto-detection (§126).
                let mut tools = crate::dbtools::detect_db_tools();
                let custom = i.custom_installs.lock().unwrap();
                for tool in &mut tools {
                    if let Some(c) = custom.resolve(&tool.id, None) {
                        tool.found_path = Some(c.path.clone());
                    }
                }
                Ok(R::DbTools { tools })
            }
            C::OpenDbTool { id } => {
                let path = i
                    .custom_installs
                    .lock()
                    .unwrap()
                    .resolve(&id, None)
                    .map(|c| c.path.clone())
                    .or_else(|| {
                        crate::dbtools::detect_db_tools()
                            .into_iter()
                            .find(|t| t.id == id)
                            .and_then(|t| t.found_path)
                    })
                    .ok_or_else(|| {
                        CoreError::ServiceError(format!("{id} was not found on this system"))
                    })?;
                // A standalone GUI app the user drives themselves — not managed/supervised.
                crate::dbtools::launch(&path, &[], false).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            C::SetCustomInstall { id, label, path } => {
                i.custom_installs.lock().unwrap().set(&id, &label, &path)?;
                i.sync_php_external();
                Ok(R::Ok)
            }
            C::RemoveCustomInstall { id, label } => {
                i.custom_installs.lock().unwrap().remove(&id, &label)?;
                i.sync_php_external();
                Ok(R::Ok)
            }
            C::ScanPhpFolder { dir } => {
                let found = crate::php::scan_folder(std::path::Path::new(&dir));
                {
                    let mut store = i.custom_installs.lock().unwrap();
                    for f in &found {
                        store.set("php", &f.version, &f.php_exe)?;
                    }
                }
                i.sync_php_external();
                Ok(R::PhpScan { found })
            }
            C::ListPhpExtensions { version } => Ok(R::PhpExtensions {
                report: i.php.extensions_report(&version),
            }),
            C::SetPhpExtension {
                version,
                name,
                enabled,
            } => {
                tracing::info!(command = "set_php_extension", version = %version, name = %name, enabled);
                i.php
                    .set_extension(&version, &name, enabled)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions {
                    report: i.php.extensions_report(&version),
                })
            }
            C::InstallPhpExtension { version, name } => {
                tracing::info!(command = "install_php_extension", version = %version, name = %name);
                i.php
                    .install_extension(&version, &name)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions {
                    report: i.php.extensions_report(&version),
                })
            }
            C::ListPeclPackages => Ok(R::Names {
                names: i.php.pecl_packages().map_err(CoreError::ServiceError)?,
            }),
            C::ListCustomInstalls => Ok(R::CustomInstalls {
                entries: i.custom_installs.lock().unwrap().list(),
            }),

            // ---- Stage 6
            C::GetWebStatus => Ok(R::WebStatus {
                status: Box::new(
                    i.web
                        .status(&i.web_config(), &i.domains.lock().unwrap().list()),
                ),
            }),
            C::GetWebConfig => Ok(R::WebConfig {
                config: i.web_config(),
            }),
            C::ListDomains => Ok(R::Domains {
                domains: i.domain_summaries(),
            }),
            C::GetDomain { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                Ok(R::Domain {
                    domain: Box::new(d),
                })
            }
            C::AddDomain { domain } => {
                tracing::info!(command = "add_domain", hostname = %domain.hostname);
                Ok(R::Domain {
                    domain: Box::new(i.add_domain(domain)?),
                })
            }
            C::UpdateDomain { domain } => Ok(R::Domain {
                domain: Box::new(i.update_domain(domain)?),
            }),
            C::RemoveDomain { hostname } => {
                i.remove_domain(&hostname)?;
                Ok(R::Ok)
            }
            C::SetDomainEnabled { hostname, enabled } => {
                i.set_domain_enabled(&hostname, enabled)?;
                Ok(R::Ok)
            }
            C::RenameDomain {
                hostname,
                new_hostname,
            } => {
                tracing::info!(command = "rename_domain", from = %hostname, to = %new_hostname);
                Ok(R::Domain {
                    domain: Box::new(i.rename_domain(&hostname, &new_hostname)?),
                })
            }
            C::DuplicateDomain {
                hostname,
                new_hostname,
            } => Ok(R::Domain {
                domain: Box::new(i.duplicate_domain(&hostname, &new_hostname)?),
            }),
            C::SuggestDomain {
                project_id,
                template,
            } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                Ok(R::Text {
                    text: crate::domain::apply_template(&template, &project.name),
                })
            }
            C::ApplyWeb { overwrite } => {
                tracing::info!(command = "apply_web");
                let retry = CoreCommand::ApplyWeb { overwrite: vec![] };
                let reports = i.journaled(
                    "apply_web",
                    "Apply the web configuration",
                    Some("Restore the previous config from a site's history on the Config page."),
                    Some(retry),
                    || i.apply_web(&overwrite),
                )?;
                Ok(R::Applied { reports })
            }
            C::StopWeb => {
                i.web.stop();
                Ok(R::Ok)
            }
            C::ValidateWeb => Ok(R::Text {
                text: i.web.validate(&i.web_config())?,
            }),
            C::RestartSiteApp { hostname } => {
                i.web.restart_app(&hostname);
                i.apply_web(&[])?;
                Ok(R::Ok)
            }
            C::SyncHosts => {
                // Names our local DNS already answers for don't need a hosts entry.
                let hostnames: Vec<String> = i
                    .domains
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .filter(|d| d.enabled && !i.web.dns_covers(&d.hostname))
                    .map(|d| d.hostname)
                    .collect();
                let changed = crate::hosts::ensure(&hostnames).map_err(CoreError::WebError)?;
                Ok(R::Text {
                    text: if changed {
                        "The hosts file was updated.".into()
                    } else {
                        "The hosts file was already up to date.".into()
                    },
                })
            }
            C::GetCaInfo => Ok(R::CaInfo {
                info: i.certs.ca_info(),
            }),
            C::TrustCa => {
                i.certs.ca().ensure_created().map_err(CoreError::WebError)?;
                i.certs
                    .ca()
                    .trust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::UntrustCa => {
                i.certs
                    .ca()
                    .untrust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::ListCertificates => Ok(R::Certificates {
                certs: i.certs.list(),
            }),
            C::RegenerateCertificate { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                i.certs.issue(&d).map_err(CoreError::WebError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::RevokeCertificate { hostname } => {
                i.certs.revoke(&hostname).map_err(CoreError::WebError)?;
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::HealthCheck { hostname } => Ok(R::Health {
                report: i.health_check(&hostname)?,
            }),

            // ---- Stage 9
            C::ListWebConfigs => {
                let cfg = i.web_config();
                let domains = i.domains.lock().unwrap();
                // Sites can be spread over every installed server; list each one's own
                // files under the server that renders it.
                let files = crate::web::SERVER_IDS
                    .iter()
                    .filter_map(|id| i.web.list_configs(&cfg, &domains, id).ok())
                    .flatten()
                    .collect();
                Ok(R::Configs { files })
            }
            C::ReadWebConfig { hostname, part } => {
                let cfg = i.web_config();
                let server = hostname
                    .as_deref()
                    .and_then(|h| i.domains.lock().unwrap().get(h))
                    .map(|d| crate::domain::resolved_server(&d, &cfg))
                    .unwrap_or_else(|| cfg.default_server.clone());
                Ok(R::Text {
                    text: i.web.read_config(&server, hostname.as_deref(), part)?,
                })
            }
            C::WriteWebConfig {
                hostname,
                part,
                content,
            } => Ok(R::Text {
                text: i.write_web_config(&hostname, part, &content)?,
            }),
            C::SetOwnership {
                hostname,
                ownership,
            } => {
                i.set_ownership(&hostname, ownership)?;
                Ok(R::Ok)
            }
            C::ListConfigHistory { hostname } => {
                let cfg = i.web_config();
                Ok(R::ConfigVersions {
                    versions: i
                        .web
                        .list_history(&cfg, &i.domains.lock().unwrap(), &hostname),
                })
            }
            C::ReadConfigHistory { hostname, id } => {
                let cfg = i.web_config();
                Ok(R::Text {
                    text: i
                        .web
                        .read_history(&cfg, &i.domains.lock().unwrap(), &hostname, &id)?,
                })
            }
            C::RestoreConfigHistory { hostname, id } => Ok(R::Text {
                text: i.restore_web_history(&hostname, &id)?,
            }),
            C::ExportWebConfig {
                hostname,
                part,
                dest,
            } => {
                let cfg = i.web_config();
                i.web
                    .export_config(&cfg, &i.domains.lock().unwrap(), &hostname, part, &dest)?;
                Ok(R::Ok)
            }

            // ---- Stage 10
            C::CreateDatabase { engine, name } => {
                i.services
                    .create_database(&engine, &name)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDatabases { engine } => Ok(R::Names {
                names: i
                    .services
                    .list_databases(&engine)
                    .map_err(CoreError::ServiceError)?,
            }),
            C::CreateDbUser {
                engine,
                user,
                password,
                database,
            } => {
                tracing::info!(command = "create_db_user", engine = %engine, user = %user);
                i.services
                    .create_user(&engine, &user, &password, &database)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDbUsers { engine } => Ok(R::DbUsers {
                users: i
                    .services
                    .list_users(&engine)
                    .map_err(CoreError::ServiceError)?,
            }),
            C::GetConnectionInfo {
                engine,
                database,
                path,
            } => Ok(R::Connection {
                info: i
                    .services
                    .connection_info(&engine, database.as_deref(), path.as_deref())
                    .map_err(CoreError::ServiceError)?,
            }),
            C::ListSqlite => Ok(R::SqliteList {
                databases: i.sqlite.lock().unwrap().list(),
            }),
            C::DetectSqlite { project_id } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let found = crate::sqlite::detect_in_project(std::path::Path::new(&project.path));
                let mut store = i.sqlite.lock().unwrap();
                let mut out = Vec::new();
                for f in found {
                    out.push(store.associate(&f.display().to_string(), Some(project_id.clone()))?);
                }
                Ok(R::SqliteList { databases: out })
            }
            C::CreateSqlite { path, project_id } => {
                let exe = i.sqlite3_path()?;
                crate::sqlite::create(&exe, std::path::Path::new(&path))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::SqliteInfo {
                    info: i.sqlite.lock().unwrap().associate(&path, project_id)?,
                })
            }
            C::AssociateSqlite { path, project_id } => Ok(R::SqliteInfo {
                info: i.sqlite.lock().unwrap().associate(&path, project_id)?,
            }),
            C::ForgetSqlite { path } => {
                i.sqlite.lock().unwrap().forget(&path)?;
                Ok(R::Ok)
            }
            C::BackupDatabase { engine, database } => {
                tracing::info!(command = "backup_database", engine = %engine, database = %database);
                let dest = crate::dbbackup::backup(&i.services, &i.paths, &engine, &database)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: dest.display().to_string(),
                })
            }
            C::ListDbBackups { engine, database } => Ok(R::DbBackups {
                backups: crate::dbbackup::list(&i.paths, &engine, database.as_deref()),
            }),
            C::RestoreDatabase {
                engine,
                database,
                file,
            } => {
                tracing::info!(command = "restore_database", engine = %engine, database = %database);
                let title = format!("Restore {database} ({engine})");
                let safety = i.journaled("restore_database", &title, Some("The database as it was before is saved as a new backup on the Databases page."), None, || {
                    crate::dbbackup::restore(&i.services, &i.paths, &engine, &database, std::path::Path::new(&file)).map_err(CoreError::ServiceError)
                })?;
                Ok(R::Text {
                    text: safety.map(|p| p.display().to_string()).unwrap_or_default(),
                })
            }
            C::DeleteDbBackup { engine, file } => {
                crate::dbbackup::delete(&i.paths, &engine, std::path::Path::new(&file))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::BackupSqlite { path } => {
                let exe = i.sqlite3_path()?;
                let dest = crate::sqlite::backup(&exe, std::path::Path::new(&path))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: dest.display().to_string(),
                })
            }
            C::RestoreSqlite { path, backup } => {
                let exe = i.sqlite3_path()?;
                let safety = crate::sqlite::restore(
                    &exe,
                    std::path::Path::new(&path),
                    std::path::Path::new(&backup),
                )
                .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: safety.map(|p| p.display().to_string()).unwrap_or_default(),
                })
            }
            C::CheckSqlite { path } => {
                let exe = i.sqlite3_path()?;
                Ok(R::Integrity {
                    result: crate::sqlite::integrity_check(&exe, std::path::Path::new(&path))
                        .map_err(CoreError::ServiceError)?,
                })
            }
            C::ListExternalTools => Ok(R::ExternalTools {
                tools: i.ext_tools.lock().unwrap().list(),
            }),
            C::SaveExternalTool { tool } => {
                i.ext_tools.lock().unwrap().save(tool)?;
                Ok(R::ExternalTools {
                    tools: i.ext_tools.lock().unwrap().list(),
                })
            }
            C::RemoveExternalTool { id } => {
                i.ext_tools.lock().unwrap().remove(&id)?;
                Ok(R::ExternalTools {
                    tools: i.ext_tools.lock().unwrap().list(),
                })
            }
            C::OpenDatabase {
                engine,
                database,
                path,
                tool_id,
            } => {
                i.open_database(
                    &engine,
                    database.as_deref(),
                    path.as_deref(),
                    tool_id.as_deref(),
                )?;
                Ok(R::Ok)
            }

            // ---- Stage 7
            C::ListQuickApps => Ok(R::QuickApps {
                apps: i.catalog.lock().unwrap().list(),
            }),
            C::GetQuickApp { id } => Ok(R::QuickApp {
                detail: Box::new(i.catalog.lock().unwrap().get(&id)?),
            }),
            C::SaveQuickApp { yaml } => Ok(R::QuickApp {
                detail: Box::new(i.catalog.lock().unwrap().save(&yaml)?),
            }),
            C::DuplicateQuickApp {
                id,
                new_id,
                new_name,
            } => Ok(R::QuickApp {
                detail: Box::new(
                    i.catalog
                        .lock()
                        .unwrap()
                        .duplicate(&id, &new_id, &new_name)?,
                ),
            }),
            C::DeleteQuickApp { id } => {
                i.catalog.lock().unwrap().delete(&id)?;
                Ok(R::Ok)
            }
            C::FavoriteQuickApp { id, favorite } => {
                i.catalog.lock().unwrap().set_favorite(&id, favorite)?;
                Ok(R::Ok)
            }
            C::ExportQuickApp { id, dest } => {
                i.catalog.lock().unwrap().export(&id, &dest)?;
                Ok(R::Ok)
            }
            C::ImportQuickApp { source } => {
                tracing::info!(command = "import_quick_app", source = %source);
                i.catalog.lock().unwrap().import(&source)?;
                Ok(R::QuickApps {
                    apps: i.catalog.lock().unwrap().list(),
                })
            }
            C::TrustQuickAppSource { origin } => {
                i.catalog.lock().unwrap().trust_source(&origin)?;
                Ok(R::Ok)
            }
            C::PlanQuickApp { id, values } => Ok(R::QuickPlan {
                result: Box::new(i.plan_quick_app(&id, &values)?),
            }),
            C::StartQuickApp {
                id,
                values,
                approval,
                allow_elevated,
            } => {
                let result = i.plan_quick_app(&id, &values)?;
                let Some(plan) = result.plan else {
                    let msgs: Vec<String> =
                        result.errors.iter().map(|e| e.message.clone()).collect();
                    return Err(CoreError::QuickAppError(msgs.join("; ")));
                };
                if !result.trusted {
                    // §139: imported recipes never run without the user's explicit approval.
                    match approval.as_deref() {
                        Some("source") => {
                            let detail = i.catalog.lock().unwrap().get(&id)?;
                            if let Some(origin) = detail.view.origin {
                                i.catalog.lock().unwrap().trust_source(&origin)?;
                            }
                        }
                        Some("once") => {}
                        _ => return Err(CoreError::QuickAppError("this Quick App comes from an untrusted source. Review its commands and approve it first.".into())),
                    }
                }
                if plan.steps.iter().any(|s| s.elevated) && !allow_elevated {
                    return Err(CoreError::QuickAppError("this Quick App has steps that need administrator rights. Confirm them separately first.".into()));
                }
                tracing::info!(command = "start_quick_app", app = %id);
                let host = Arc::new(Host {
                    inner: i.clone(),
                    project_id: std::sync::Mutex::new(None),
                });
                Ok(R::QuickRunStarted {
                    run_id: i.runs.start(plan, host, allow_elevated),
                })
            }
            C::GetQuickRun { id } => i
                .runs
                .get(&id)
                .map(|r| R::QuickRun { run: Box::new(r) })
                .ok_or_else(|| CoreError::QuickAppError(format!("no run \"{id}\""))),
            C::ListQuickRuns => Ok(R::QuickRuns {
                runs: i.runs.list(),
            }),
            C::CancelQuickRun { id } => {
                i.runs.cancel(&id);
                Ok(R::Ok)
            }
            C::ListQuickCommands => Ok(R::QuickCommands {
                commands: i.quick_commands.list(),
            }),
            C::SaveQuickCommand { command } => {
                i.quick_commands.save(command)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }
            C::DeleteQuickCommand { id } => {
                i.quick_commands.delete(&id)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }
            C::RunQuickCommand { id, project_id } => Ok(R::MaybeProcess {
                id: i.run_quick_command(&id, project_id.as_deref())?,
            }),
            C::RunCommandLine {
                line,
                cwd,
                project_id,
            } => Ok(R::ProcessStarted {
                id: i.run_command_line(&line, cwd.as_deref(), project_id.as_deref(), None)?,
            }),
            C::ListHistory => Ok(R::History {
                entries: i.history.lock().unwrap().list(),
            }),
            C::DeleteHistory { id } => {
                i.history.lock().unwrap().delete(id)?;
                Ok(R::History {
                    entries: i.history.lock().unwrap().list(),
                })
            }
            C::ClearHistory => {
                i.history.lock().unwrap().clear()?;
                Ok(R::Ok)
            }
            C::SaveHistoryAsQuickCommand {
                id,
                command_id,
                name,
            } => {
                let entry = i.history.lock().unwrap().get(id).ok_or_else(|| {
                    CoreError::QuickAppError("that history entry no longer exists".into())
                })?;
                let cmd =
                    quick_command_from_line(&command_id, &name, &entry.line, entry.cwd.as_deref())
                        .map_err(CoreError::QuickAppError)?;
                i.quick_commands.save(cmd)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }

            // ---- Stage 8
            C::GetDashboard => {
                let cfg = i.web_config();
                let domains_list = i.domains.lock().unwrap().list();
                let web = i.web.status(&cfg, &domains_list);
                let domains = i.domain_summaries();
                let project_count = i.projects.lock().unwrap().list().len();
                let health = i.environment_health();
                Ok(R::Dashboard {
                    data: Box::new(DashboardData {
                        services: i.services.list(),
                        web,
                        domains,
                        project_count,
                        health,
                    }),
                })
            }
            C::GetEnvironmentHealth => Ok(R::EnvironmentHealth {
                items: i.environment_health(),
            }),
            C::ListLogSources => Ok(R::LogSources {
                sources: i.log_sources(),
            }),
            C::ReadLog { source, max_lines } => Ok(R::LogLines {
                lines: i.read_log(&source, max_lines.clamp(1, 20_000))?,
                source,
            }),
            C::ExportLog { source, dest } => {
                let lines = i.read_log(&source, 1_000_000)?;
                std::fs::write(&dest, lines.join("\n"))?;
                Ok(R::Ok)
            }
            C::ClearLog { source } => {
                i.clear_log(&source)?;
                Ok(R::Ok)
            }
            C::ListEditors => Ok(R::Editors {
                editors: crate::editors::detect(),
            }),
            C::GetSystemStats => Ok(R::SystemStats {
                stats: Box::new(i.system_stats()),
            }),
            C::ListMigrationSources => Ok(R::MigrationSources {
                sources: crate::migrate::detect(),
            }),
            C::ListForeignDatabases {
                source_id,
                password,
            } => Ok(R::Names {
                names: i.foreign_databases(&source_id, &password)?,
            }),
            C::MigrateDatabases {
                source_id,
                password,
                databases,
                target,
            } => {
                tracing::info!(command = "migrate_databases", source = %source_id, target = %target);
                let title = format!("Import databases into {target}");
                let results = i.journaled("migrate_databases", &title, Some("Imported databases can be dropped from the Databases page; the source was not changed."), None, || i.migrate_databases(&source_id, &password, &databases, &target))?;
                Ok(R::Migrated { results })
            }
            C::GetMigrationProgress => Ok(R::MigrationProgress {
                progress: i.migration.snapshot(),
            }),
            C::GetHelperService => Ok(R::HelperService {
                installed: crate::elevate::service_available(),
            }),
            C::InstallHelperService => {
                crate::elevate::install_service().map_err(CoreError::ServiceError)?;
                Ok(R::HelperService {
                    installed: crate::elevate::service_available(),
                })
            }
            C::UninstallHelperService => {
                crate::elevate::uninstall_service().map_err(CoreError::ServiceError)?;
                Ok(R::HelperService {
                    installed: crate::elevate::service_available(),
                })
            }

            // ---- Stage 11
            C::GetXdebug { version } => Ok(R::Xdebug {
                report: i.xdebug_report(&version),
            }),
            C::SetXdebug { version, settings } => {
                tracing::info!(command = "set_xdebug", version = %version);
                i.php
                    .set_xdebug_settings(&version, &settings)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Xdebug {
                    report: i.xdebug_report(&version),
                })
            }
            C::XdebugIdeConfig {
                project_id,
                ide,
                version,
            } => Ok(R::Text {
                text: i.xdebug_ide_config(&project_id, &ide, &version)?,
            }),
            C::DiscoverCommands { project_id } => Ok(R::CommandSources {
                sources: i.discover_commands(&project_id)?,
            }),
            C::GetComposerInfo { project_id } => Ok(R::Composer {
                info: Box::new(i.composer_info(&project_id)?),
            }),
            C::RunComposer {
                project_id,
                action,
                target,
            } => Ok(R::ProcessStarted {
                id: i.run_composer(&project_id, &action, target.as_deref())?,
            }),
            C::GetPackageManagers { project_id } => Ok(R::PackageManagers {
                info: i.package_managers(&project_id)?,
            }),
            C::EnablePackageManager {
                project_id,
                manager,
            } => Ok(R::ProcessStarted {
                id: i.enable_package_manager(&project_id, &manager)?,
            }),
            C::GetVenv { project_id } => Ok(R::Venv {
                info: i.venv_info(&project_id)?,
            }),
            C::CreateVenv {
                project_id,
                recreate,
            } => Ok(R::ProcessStarted {
                id: i.create_venv(&project_id, recreate)?,
            }),
            C::InstallVenvRequirements { project_id, what } => Ok(R::ProcessStarted {
                id: i.install_venv_requirements(&project_id, &what)?,
            }),
            C::RunDiagnostics => Ok(R::Diagnostics {
                findings: i.diagnose(),
            }),
            C::IgnoreDiagnostic { id, ignore } => {
                i.set_diagnostic_ignored(&id, ignore)?;
                Ok(R::Diagnostics {
                    findings: i.diagnose(),
                })
            }
            C::RestartService { id } => {
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    // A web server is restarted by re-applying it, so its sites' config
                    // is re-rendered and reloaded rather than only bounced.
                    i.services.stop(&id);
                    std::thread::sleep(Duration::from_millis(800));
                    i.apply_web_server(&id, &[]).map(|_| R::Ok)?;
                    return Ok(R::Ok);
                }
                i.services.stop(&id);
                // The old process needs a moment to release its port before the new one binds it.
                std::thread::sleep(Duration::from_millis(800));
                i.services.start(&id).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            // ---- .env editor
            C::ListEnvFiles { project_id } => Ok(R::EnvFiles {
                files: i.env_files(&project_id)?,
            }),
            C::ReadEnvFile { project_id, file } => Ok(R::EnvFile {
                view: Box::new(i.env_read(&project_id, &file)?),
            }),
            C::SaveEnvFile {
                project_id,
                file,
                content,
            } => {
                tracing::info!(command = "save_env_file", file = %file);
                Ok(R::EnvFile {
                    view: Box::new(i.env_write(&project_id, &file, &content)?),
                })
            }
            C::SetEnvValue {
                project_id,
                file,
                key,
                value,
            } => {
                // The value is never logged: env files hold secrets (§141).
                tracing::info!(command = "set_env_value", file = %file, key = %key);
                Ok(R::EnvFile {
                    view: Box::new(i.env_set(&project_id, &file, &key, &value)?),
                })
            }
            C::DeleteEnvKey {
                project_id,
                file,
                key,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_delete(&project_id, &file, &key)?),
            }),
            C::CompareEnvFiles { project_id, a, b } => Ok(R::EnvCompare {
                rows: i.env_compare(&project_id, &a, &b)?,
            }),
            C::ImportEnvFile {
                project_id,
                file,
                source,
                mode,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_import(&project_id, &file, &source, &mode)?),
            }),
            C::ExportEnvFile {
                project_id,
                file,
                dest,
            } => {
                i.env_export(&project_id, &file, &dest)?;
                Ok(R::Ok)
            }
            C::ListOperations => Ok(R::Operations {
                operations: i.journal.lock().unwrap().list(),
            }),
            C::DismissOperation { id } => {
                i.journal.lock().unwrap().dismiss(id);
                Ok(R::Ok)
            }
            C::MailpitEnvPlan { project_id, file } => Ok(R::MailEnvPlan {
                plan: Box::new(i.mailpit_env_plan(&project_id, &file)?),
            }),
            C::ApplyMailpitEnv { project_id, file } => {
                tracing::info!(command = "apply_mailpit_env", file = %file);
                Ok(R::MailEnvPlan {
                    plan: Box::new(i.apply_mailpit_env(&project_id, &file)?),
                })
            }
            C::MailDiagnostics { project_id } => Ok(R::MailChecks {
                checks: i.mail_diagnostics(project_id.as_deref()),
            }),
            C::SendTestMail { to } => {
                crate::mail::send_test_mail(crate::service::mailpit_smtp_port(), to.trim())
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: format!(
                        "Test message sent to {}. Open Mailpit to see it.",
                        to.trim()
                    ),
                })
            }
            C::CreateEnvFile {
                project_id,
                file,
                from,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_create(&project_id, &file, from.as_deref())?),
            }),

            C::GetStartupSettings => Ok(R::Startup {
                settings: i.startup_settings(),
            }),
            C::SetStartupSettings { settings } => {
                i.set_startup_settings(settings)?;
                Ok(R::Startup {
                    settings: i.startup_settings(),
                })
            }
            C::OpenPath { path } => {
                i.open_path(&path)?;
                Ok(R::Ok)
            }
            C::OpenUrl { url } => {
                i.open_url(&url)?;
                Ok(R::Ok)
            }
            C::OpenInEditor { path } => {
                i.open_in_editor(&path)?;
                Ok(R::Ok)
            }
            C::OpenWith { path, app } => {
                i.open_with(&path, &app)?;
                Ok(R::Ok)
            }
            C::OpenTerminal {
                project_id,
                shell,
                rows,
                cols,
            } => {
                tracing::info!(command = "open_terminal");
                Ok(R::Terminal {
                    id: i.open_terminal(project_id.as_deref(), shell, rows, cols)?,
                })
            }
            C::TerminalInput { id, data } => {
                i.terminals
                    .write(id, &data)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ResizeTerminal { id, rows, cols } => {
                i.terminals
                    .resize(id, rows, cols)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::CloseTerminal { id } => {
                i.terminals.close(id);
                Ok(R::Ok)
            }
            C::ListProjectShortcuts { project_id } => Ok(R::Shortcuts {
                shortcuts: i.project_shortcuts(&project_id)?,
            }),

            // ---- Stage 12
            C::GetManifest { project_id } => Ok(R::ManifestInfo {
                info: Box::new(i.manifest_info(&project_id)?),
            }),
            C::SaveManifest {
                project_id,
                manifest,
            } => Ok(R::Text {
                text: i.save_manifest(&project_id, manifest)?,
            }),
            C::SaveManifestText { project_id, text } => Ok(R::Text {
                text: i.save_manifest_text(&project_id, &text)?,
            }),
            C::PlanSetup { project_id } => Ok(R::SetupPlan {
                plan: Box::new(i.plan_setup(&project_id)?),
            }),
            C::ApplySetup {
                project_id,
                dry_run,
            } => {
                tracing::info!(command = "apply_setup", project = %project_id, dry_run);
                Ok(R::Setup {
                    report: Box::new(i.apply_setup(&project_id, dry_run)?),
                })
            }
            C::GetSetupProgress => Ok(R::SetupProgress {
                report: i.setup_progress().map(Box::new),
            }),
            C::ImportProcfile {
                project_id,
                dry_run,
            } => {
                let preview = if dry_run {
                    i.procfile_preview(&project_id)?
                } else {
                    i.import_procfile(&project_id)?
                };
                Ok(R::Procfile {
                    preview: Box::new(preview),
                })
            }
            C::ReadSiteFile { hostname, name } => Ok(R::Text {
                text: i.read_site_file(&hostname, &name)?,
            }),
            C::WriteSiteFile {
                hostname,
                name,
                content,
            } => Ok(R::Text {
                text: i.write_site_file(&hostname, &name, &content)?,
            }),

            // ---- Stage 13
            C::ListProfiles => Ok(R::Profiles {
                profiles: i.profiles.list(),
            }),
            C::SaveProfile { profile } => Ok(R::Profile {
                profile: Box::new(i.profiles.save(profile).map_err(CoreError::EnvError)?),
            }),
            C::DeleteProfile { id } => {
                i.profiles.delete(&id).map_err(CoreError::EnvError)?;
                Ok(R::Ok)
            }
            C::ExportProfile { id, dest } => {
                i.profiles
                    .export(&id, std::path::Path::new(&dest))
                    .map_err(CoreError::EnvError)?;
                Ok(R::Ok)
            }
            C::ReadProfileFile { source } => Ok(R::Profile {
                profile: Box::new(
                    crate::profiles::ProfileStore::read_file(std::path::Path::new(&source))
                        .map_err(CoreError::EnvError)?,
                ),
            }),
            C::ImportProfile { source } => {
                let p = crate::profiles::ProfileStore::read_file(std::path::Path::new(&source))
                    .map_err(CoreError::EnvError)?;
                Ok(R::Profile {
                    profile: Box::new(i.profiles.save(p).map_err(CoreError::EnvError)?),
                })
            }
            C::ApplyProfile {
                project_id,
                profile_id,
            } => Ok(R::Manifest {
                manifest: Box::new(i.apply_profile(&project_id, &profile_id)?),
            }),
            C::ProfileYaml { id } => {
                let p = i
                    .profiles
                    .get(&id)
                    .ok_or_else(|| CoreError::EnvError(format!("no profile \"{id}\"")))?;
                Ok(R::Text {
                    text: serde_yaml_ng::to_string(&p)
                        .map_err(|e| CoreError::EnvError(e.to_string()))?,
                })
            }
            C::SaveProfileYaml { yaml } => {
                let p: crate::profiles::Profile = serde_yaml_ng::from_str(&yaml).map_err(|e| {
                    CoreError::EnvError(format!("the profile doesn't read as YAML: {e}"))
                })?;
                Ok(R::Profile {
                    profile: Box::new(i.profiles.save(p).map_err(CoreError::EnvError)?),
                })
            }
            C::ProfileFromProject { project_id, name } => Ok(R::Profile {
                profile: Box::new(i.profile_from_project(&project_id, &name)?),
            }),
            C::GetProjectModes { project_id } => Ok(R::Modes {
                view: i.project_modes(&project_id)?,
            }),
            C::SetProjectMode { project_id, mode } => Ok(R::ModeResult {
                result: i.set_project_mode(&project_id, &mode)?,
            }),
            C::ListWorkers { project_id } => Ok(R::Workers {
                workers: i.worker_statuses(project_id.as_deref()),
            }),
            C::ListWorkerPresets => Ok(R::WorkerPresets {
                presets: crate::workers::presets(),
            }),
            C::SaveWorker { worker } => {
                let w = i.save_worker(worker)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(Some(&w.project_id)),
                })
            }
            C::RemoveWorker { id } => {
                i.remove_worker(&id)?;
                Ok(R::Ok)
            }
            C::StartWorker { id } => {
                i.start_worker(&id)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::StopWorker { id } => {
                i.stop_worker(&id);
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::RestartWorker { id } => {
                i.restart_worker(&id)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::StartProjectWorkers { project_id } => Ok(R::Count {
                count: i.start_project_workers(&project_id)?,
            }),
            C::StopProjectWorkers { project_id } => {
                i.stop_project_workers(&project_id);
                Ok(R::Ok)
            }
            C::ListSchedules { project_id } => Ok(R::Schedules {
                tasks: i.task_statuses(project_id.as_deref()),
            }),
            C::SaveSchedule { task } => {
                let t = i.save_schedule(task)?;
                Ok(R::Schedules {
                    tasks: i.task_statuses(t.project_id.as_deref()),
                })
            }
            C::RemoveSchedule { id } => {
                i.remove_schedule(&id)?;
                Ok(R::Ok)
            }
            C::RunScheduleNow { id } => Ok(R::TaskRun {
                run: i.run_task(&id)?,
            }),
            C::DescribeSchedule { schedule } => {
                crate::scheduler::Schedule::parse(&schedule).map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: crate::scheduler::describe(&schedule),
                })
            }
            C::ListSnapshots { project_id } => Ok(R::Snapshots {
                snapshots: i.list_snapshots(&project_id),
            }),
            C::CreateSnapshot {
                project_id,
                label,
                options,
            } => Ok(R::Snapshot {
                snapshot: Box::new(i.create_snapshot(&project_id, &label, options)?),
            }),
            C::DeleteSnapshot { project_id, id } => {
                i.delete_snapshot(&project_id, &id)?;
                Ok(R::Ok)
            }
            C::RestoreSnapshot {
                project_id,
                id,
                options,
            } => Ok(R::Restored {
                result: i.restore_snapshot(&project_id, &id, options)?,
            }),
            C::ExportSnapshot {
                project_id,
                id,
                dest,
            } => {
                i.export_snapshot(&project_id, &id, &dest)?;
                Ok(R::Ok)
            }
            C::PreviewImport { source } => Ok(R::ImportPreview {
                preview: Box::new(i.preview_import(&source)?),
            }),
            C::ImportEnvironment {
                source,
                target,
                name,
            } => Ok(R::Cloned {
                result: Box::new(i.import_environment(&source, &target, &name)?),
            }),
            C::CloneEnvironment {
                project_id,
                target,
                name,
                what,
            } => Ok(R::Cloned {
                result: Box::new(i.clone_environment(&project_id, &target, &name, &what)?),
            }),
            C::BackupSettings => Ok(R::SettingsBackup {
                backup: i.backup_settings()?,
            }),
            C::ListSettingsBackups => Ok(R::SettingsBackups {
                backups: i.list_settings_backups(),
            }),
            C::RestoreSettings { id } => Ok(R::SettingsBackup {
                backup: i.restore_settings(&id)?,
            }),
            C::GetResourceLimits => Ok(R::Resources {
                limits: i.resource_limits(),
            }),
            C::SetResourceLimits { limits } => {
                i.set_resource_limits(limits)?;
                Ok(R::Resources {
                    limits: i.resource_limits(),
                })
            }

            // ---- Stage 14
            C::ListTunnelProviders => Ok(R::TunnelProviders {
                providers: i.tunnel_providers(),
            }),
            C::ListTunnels => Ok(R::Tunnels {
                tunnels: i.list_tunnels(),
            }),
            C::SaveTunnel { tunnel } => {
                let t = i.save_tunnel(tunnel)?;
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&t.id)?),
                })
            }
            C::RemoveTunnel { id } => {
                i.remove_tunnel(&id)?;
                Ok(R::Ok)
            }
            C::StartTunnel {
                id,
                confirm_exposure,
            } => {
                tracing::info!(command = "start_tunnel", id = %id);
                Ok(R::Tunnel {
                    tunnel: Box::new(i.start_tunnel(&id, confirm_exposure)?),
                })
            }
            C::StopTunnel { id } => {
                i.stop_tunnel(&id);
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&id)?),
                })
            }
            C::CheckTunnel { id } => Ok(R::Tunnel {
                tunnel: Box::new(i.check_tunnel(&id)?),
            }),
            C::TunnelLog { id } => Ok(R::Lines {
                lines: i.tunnel_log(&id),
            }),
            C::SetTunnelToken { provider, token } => {
                // The token itself is never logged (§141).
                tracing::info!(command = "set_tunnel_token", provider = %provider);
                i.set_tunnel_token(&provider, token.as_deref())?;
                Ok(R::TunnelProviders {
                    providers: i.tunnel_providers(),
                })
            }
            C::SetTunnelPassword { id, password } => {
                i.set_tunnel_password(&id, password.as_deref())?;
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&id)?),
                })
            }
            C::ListTunnelRequests { id } => Ok(R::TunnelRequests {
                requests: i.tunnel_requests(&id)?,
            }),
            C::ClearTunnelRequests { id } => {
                i.clear_tunnel_requests(&id)?;
                Ok(R::Ok)
            }
            C::ReplayTunnelRequest { id, request_id } => Ok(R::TunnelRequest {
                request: Box::new(i.replay_tunnel_request(&id, request_id)?),
            }),
            C::SendTunnelTestRequest {
                id,
                method,
                path,
                headers,
                body,
            } => Ok(R::TunnelRequest {
                request: Box::new(i.send_tunnel_test(&id, &method, &path, &headers, &body)?),
            }),

            // ---- Stage 15
            C::GlobalSearch { query } => Ok(R::SearchResults {
                hits: i.global_search(&query),
            }),
            C::Doctor => Ok(R::DoctorReport {
                report: Box::new(i.doctor()),
            }),
            C::DiagnoseProject { project_id } => Ok(R::Diagnostics {
                findings: i.diagnose_project(&project_id)?,
            }),
            C::PlanRepair { project_id } => Ok(R::RepairPlan {
                plan: Box::new(i.plan_repair(project_id.as_deref())?),
            }),
            C::ApplyRepair {
                project_id,
                ids,
                confirm_destructive,
            } => {
                tracing::info!(command = "apply_repair", project = ?project_id, count = ids.len());
                Ok(R::RepairReport {
                    report: Box::new(self.apply_repair(
                        project_id.as_deref(),
                        &ids,
                        confirm_destructive,
                    )?),
                })
            }
            C::GitStatus { project_id } => Ok(R::GitStatus {
                status: Box::new(i.git_status(&project_id)?),
            }),
            C::GitInit { project_id } => Ok(R::GitStatus {
                status: Box::new(i.git_init(&project_id)?),
            }),
            C::GitBranches { project_id } => Ok(R::GitBranches {
                branches: i.git_branches(&project_id)?,
            }),
            C::GitCreateBranch {
                project_id,
                name,
                checkout,
            } => {
                i.git_create_branch(&project_id, &name, checkout)?;
                Ok(R::GitBranches {
                    branches: i.git_branches(&project_id)?,
                })
            }
            C::GitSwitchBranch { project_id, name } => {
                i.git_switch(&project_id, &name)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitDeleteBranch {
                project_id,
                name,
                force,
            } => {
                i.git_delete_branch(&project_id, &name, force)?;
                Ok(R::GitBranches {
                    branches: i.git_branches(&project_id)?,
                })
            }
            C::GitStage { project_id, paths } => {
                i.git_stage(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitUnstage { project_id, paths } => {
                i.git_unstage(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitDiscard { project_id, paths } => {
                tracing::info!(command = "git_discard", project = %project_id, files = paths.len());
                i.git_discard(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitCommit {
                project_id,
                message,
                amend,
            } => Ok(R::GitCommit {
                commit: i.git_commit(&project_id, &message, amend)?,
            }),
            C::GitSync {
                project_id,
                action,
                remote,
            } => {
                tracing::info!(command = "git_sync", project = %project_id, action = %action);
                Ok(R::GitResult {
                    result: i.git_sync(&project_id, &action, remote.as_deref())?,
                })
            }
            C::GitDiff {
                project_id,
                path,
                staged,
            } => Ok(R::Text {
                text: i.git_diff(&project_id, &path, staged)?,
            }),
            C::GitLog { project_id, limit } => Ok(R::GitCommits {
                commits: i.git_log(&project_id, limit)?,
            }),
            C::GitShow { project_id, hash } => Ok(R::Text {
                text: i.git_show(&project_id, &hash)?,
            }),
            C::GitAddRemote {
                project_id,
                name,
                url,
            } => {
                i.git_add_remote(&project_id, &name, &url)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitRemoveRemote { project_id, name } => {
                i.git_remove_remote(&project_id, &name)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitStash {
                project_id,
                action,
                message,
                index,
            } => Ok(R::GitResult {
                result: i.git_stash(&project_id, &action, message.as_deref(), index)?,
            }),
            C::GitAddIgnore {
                project_id,
                template,
            } => Ok(R::Count {
                count: i.git_add_ignore(&project_id, &template)?,
            }),
            C::GitSetCredentials {
                host,
                username,
                token,
            } => {
                // The token is never logged (§141).
                tracing::info!(command = "git_set_credentials", host = %host);
                i.git_set_credentials(&host, &username, token.as_deref())?;
                Ok(R::Ok)
            }
            C::ListSshKeys => Ok(R::SshKeys {
                keys: i.list_ssh_keys(),
            }),
            C::GitClone {
                url,
                target,
                branch,
                auth,
            } => {
                tracing::info!(command = "git_clone", target = %target);
                Ok(R::Project {
                    project: i.git_clone(&url, &target, branch.as_deref(), auth)?,
                })
            }

            C::ListPlugins => Ok(R::Plugins {
                plugins: i.list_plugins(),
            }),
            C::InstallPlugin { source } => {
                tracing::info!(command = "install_plugin", source = %source);
                Ok(R::Plugin {
                    plugin: Box::new(i.install_plugin(&source)?),
                })
            }
            C::SetPluginEnabled {
                id,
                enabled,
                approve,
            } => {
                tracing::info!(command = "set_plugin_enabled", id = %id, enabled);
                Ok(R::Plugin {
                    plugin: Box::new(i.set_plugin_enabled(&id, enabled, &approve)?),
                })
            }
            C::RemovePlugin { id } => {
                i.remove_plugin(&id)?;
                Ok(R::Ok)
            }
            C::PluginDetect { project_id } => Ok(R::PluginDetections {
                detections: i.plugin_detect(&project_id)?,
            }),
            C::ListCatalogSources => Ok(R::CatalogSources {
                catalogs: i.catalog_views(),
            }),
            C::AddCatalogSource {
                name,
                url,
                public_key,
            } => {
                i.add_catalog_source(&name, &url, &public_key)?;
                Ok(R::CatalogSources {
                    catalogs: i.catalog_views(),
                })
            }
            C::RemoveCatalogSource { id } => {
                i.remove_catalog_source(&id)?;
                Ok(R::CatalogSources {
                    catalogs: i.catalog_views(),
                })
            }
            C::RefreshCatalogs { id } => Ok(R::CatalogSources {
                catalogs: i.refresh_catalogs(id.as_deref()),
            }),
            C::InstallCatalogPlugin {
                source_id,
                plugin_id,
            } => {
                tracing::info!(command = "install_catalog_plugin", source = %source_id, plugin = %plugin_id);
                Ok(R::Plugin {
                    plugin: Box::new(i.install_catalog_plugin(&source_id, &plugin_id)?),
                })
            }

            C::GetApiStatus => Ok(R::ApiStatus {
                status: Box::new(i.api_status()),
            }),
            C::SetApiSettings {
                enabled,
                port,
                mode,
            } => {
                tracing::info!(command = "set_api_settings", enabled, port, mode = %mode);
                Ok(R::ApiStatus {
                    status: Box::new(i.set_api_settings(enabled, port, &mode)?),
                })
            }
            C::RotateApiToken => {
                tracing::info!(command = "rotate_api_token");
                Ok(R::Text {
                    text: i.rotate_api_token()?,
                })
            }
            C::ClearApiToken => {
                i.clear_api_token()?;
                Ok(R::ApiStatus {
                    status: Box::new(i.api_status()),
                })
            }
            C::GetUpdaterStatus => Ok(R::UpdaterStatus {
                status: Box::new(i.updater_status()),
            }),
            C::SetUpdaterSettings {
                endpoint,
                public_key,
            } => Ok(R::UpdaterStatus {
                status: Box::new(i.set_updater_settings(&endpoint, &public_key)?),
            }),
            C::CheckUpdate => Ok(R::Update {
                update: Box::new(i.check_update()?),
            }),
            C::DownloadUpdate => Ok(R::Update {
                update: Box::new(i.download_update()?),
            }),
            C::InstallUpdate => {
                tracing::info!(command = "install_update");
                i.install_update()?;
                Ok(R::Ok)
            }
            C::GetShellMenu => Ok(R::ShellMenu {
                status: i.shell_menu_status(),
            }),
            C::InstallShellMenu => Ok(R::ShellMenu {
                status: i.install_shell_menu()?,
            }),
            C::RemoveShellMenu => Ok(R::ShellMenu {
                status: i.remove_shell_menu()?,
            }),
            C::CheckNetwork { force } => Ok(R::Network {
                status: Box::new(i.network_status(force)),
            }),
            C::ExportSupportBundle { dest } => Ok(R::Lines {
                lines: i.export_support_bundle(&dest)?,
            }),

            C::LoadOverview { project_id } => Ok(R::LoadOverview {
                overview: Box::new(i.load_overview(&project_id)?),
            }),
            C::LoadReadScript { project_id, name } => Ok(R::Text {
                text: i.load_read_script(&project_id, &name)?,
            }),
            C::LoadSaveScript {
                project_id,
                name,
                content,
            } => {
                i.load_save_script(&project_id, &name, &content)?;
                Ok(R::Ok)
            }
            C::LoadDeleteScript { project_id, name } => {
                i.load_delete_script(&project_id, &name)?;
                Ok(R::Ok)
            }
            C::LoadListProfiles => Ok(R::LoadProfiles {
                profiles: i.load_profiles(),
            }),
            C::LoadSaveProfile { profile } => Ok(R::LoadProfiles {
                profiles: i.load_save_profile(profile)?,
            }),
            C::LoadDeleteProfile { id } => Ok(R::LoadProfiles {
                profiles: i.load_delete_profile(&id)?,
            }),
            C::LoadGenerate {
                project_id,
                profile,
                name,
            } => Ok(R::Text {
                text: i.load_generate(&project_id, &profile, name.as_deref())?,
            }),
            C::LoadRun {
                project_id,
                script,
                target,
                confirm_public,
                env,
            } => {
                tracing::info!(command = "load_run", project = %project_id, script = %script, public = confirm_public);
                Ok(R::LoadRun {
                    run: Box::new(i.load_run(
                        &project_id,
                        &script,
                        target.as_deref(),
                        confirm_public,
                        &env,
                    )?),
                })
            }
            C::LoadStatus { run_id } => Ok(R::LoadRun {
                run: Box::new(i.load_status(&run_id)?),
            }),
            C::LoadStop { run_id } => Ok(R::LoadRun {
                run: Box::new(i.load_stop(&run_id)?),
            }),
            C::LoadRuns { project_id } => Ok(R::LoadRuns {
                runs: i.load_runs(&project_id),
            }),
            C::LoadDeleteRun { project_id, run_id } => {
                i.load_delete_run(&project_id, &run_id)?;
                Ok(R::Ok)
            }

            C::AiGetState => Ok(R::AiState {
                state: Box::new(i.ai_state()),
            }),
            C::AiSaveSettings { enabled, features } => Ok(R::AiState {
                state: Box::new(i.ai_save_settings(enabled, features)?),
            }),
            C::AiSaveProvider { provider, api_key } => {
                // Only that a provider was saved, never its key (§141).
                tracing::info!(command = "ai_save_provider", provider = %provider.name, key_changed = api_key.is_some());
                Ok(R::AiState {
                    state: Box::new(i.ai_save_provider(provider, api_key)?),
                })
            }
            C::AiRemoveProvider { id } => Ok(R::AiState {
                state: Box::new(i.ai_remove_provider(&id)?),
            }),
            C::AiDetectLocal => Ok(R::AiDetected {
                servers: i.ai_detect_local(),
            }),
            C::AiTest { provider_id } => Ok(R::AiTest {
                result: i.ai_test(&provider_id)?,
            }),
            C::AiProbe { provider, api_key } => Ok(R::AiTest {
                result: i.ai_probe(provider, api_key)?,
            }),
            C::AiModels { provider_id } => Ok(R::AiModels {
                models: i.ai_models(&provider_id)?,
            }),
            C::AiPreview { request } => Ok(R::AiPrompt {
                prompt: Box::new(i.ai_prompt(&request)?),
            }),
            C::AiStart {
                request,
                confirm_remote,
            } => {
                tracing::info!(command = "ai_start", feature = %request.feature, confirm_remote);
                Ok(R::AiJob {
                    job: Box::new(i.ai_start(request, confirm_remote)?),
                })
            }
            C::AiJob { job_id } => Ok(R::AiJob {
                job: Box::new(i.ai_job(&job_id)?),
            }),
            C::AiCancel { job_id } => Ok(R::AiJob {
                job: Box::new(i.ai_cancel(&job_id)?),
            }),
            C::AiApply {
                actions,
                confirm_destructive,
            } => Ok(R::AiApplied {
                steps: self.ai_apply(actions, confirm_destructive),
            }),
            // @@arms-end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the isolated-home guard alive for the test's duration (it unsets `OLS_HOME` on drop).
    fn test_core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = SettingsService::load(&home.paths).unwrap();
        let core = Core::new(settings, home.paths.clone());
        (core, home)
    }

    #[test]
    fn sites_are_grouped_by_website_type() {
        let (core, home) = test_core();
        let root = home.paths.root().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let make =
            |host: &str, kind: crate::domain::SiteKind, app: Option<(&str, Option<&str>)>| Domain {
                hostname: host.into(),
                project_id: None,
                root: root.display().to_string(),
                kind,
                https: false,
                redirect_https: false,
                wildcard: false,
                enabled: true,
                ownership: Ownership::Managed,
                app: app.map(|(exe, runtime)| crate::domain::AppSpec {
                    executable: exe.into(),
                    args: vec![],
                    cwd: root.display().to_string(),
                    runtime: runtime.map(str::to_string),
                }),
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
            };
        let proxy = || crate::domain::SiteKind::Proxy {
            upstream_port: 3000,
            upstream_host: None,
            upstream_https: false,
        };
        let sites = [
            make(
                "a.test",
                crate::domain::SiteKind::Php { version: None },
                None,
            ),
            make("b.test", crate::domain::SiteKind::Static, None),
            make("c.test", proxy(), Some(("npm", Some("node")))),
            make("d.test", proxy(), Some(("uvicorn", None))),
            make("e.test", proxy(), None),
            make(
                "f.test",
                crate::domain::SiteKind::Proxy {
                    upstream_port: 80,
                    upstream_host: Some("10.0.0.5".into()),
                    upstream_https: false,
                },
                None,
            ),
        ];
        for s in sites {
            core.dispatch(CoreCommand::AddDomain { domain: s }).unwrap();
        }
        let groups: std::collections::BTreeMap<String, String> =
            match core.dispatch(CoreCommand::ListDomains).unwrap() {
                CoreResponse::Domains { domains } => {
                    domains.into_iter().map(|d| (d.hostname, d.group)).collect()
                }
                _ => panic!("expected Domains"),
            };
        let expect = [
            ("a.test", "php"),
            ("b.test", "static"),
            ("c.test", "nodejs"),
            ("d.test", "python"),
            ("e.test", "proxy"),
            ("f.test", "proxy"),
        ];
        for (host, group) in expect {
            assert_eq!(groups.get(host).map(String::as_str), Some(group), "{host}");
        }
    }

    #[test]
    fn env_files_round_trip_through_the_core_and_keep_a_backup() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("laravel");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".env"),
            "# keep me\nAPP_ENV=local\nDB_PASSWORD=old\n",
        )
        .unwrap();
        std::fs::write(dir.join(".env.example"), "APP_ENV=example\nNEW_KEY=1\n").unwrap();
        let pid = match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project.id,
            _ => panic!("expected Project"),
        };
        let view = |r: CoreResponse| match r {
            CoreResponse::EnvFile { view } => *view,
            _ => panic!("expected EnvFile"),
        };
        let v = view(
            core.dispatch(CoreCommand::SetEnvValue {
                project_id: pid.clone(),
                file: ".env".into(),
                key: "APP_ENV".into(),
                value: "production".into(),
            })
            .unwrap(),
        );
        assert!(v.content.starts_with("# keep me\nAPP_ENV=production\n"));
        assert!(
            v.entries
                .iter()
                .find(|e| e.key == "DB_PASSWORD")
                .unwrap()
                .secret
        );
        let backups = home.paths.data_dir().join("env_backups").join(&pid);
        assert_eq!(
            std::fs::read_dir(backups).unwrap().count(),
            1,
            "the previous version is kept"
        );

        // A file with a broken line is refused and the good one stays.
        assert!(core
            .dispatch(CoreCommand::SaveEnvFile {
                project_id: pid.clone(),
                file: ".env".into(),
                content: "oops no equals\n".into()
            })
            .is_err());
        assert!(std::fs::read_to_string(dir.join(".env"))
            .unwrap()
            .contains("APP_ENV=production"));

        // Names that could leave the project folder are refused.
        assert!(core
            .dispatch(CoreCommand::ReadEnvFile {
                project_id: pid.clone(),
                file: "../secrets.txt".into()
            })
            .is_err());

        match core
            .dispatch(CoreCommand::CompareEnvFiles {
                project_id: pid.clone(),
                a: ".env".into(),
                b: ".env.example".into(),
            })
            .unwrap()
        {
            CoreResponse::EnvCompare { rows } => assert!(rows
                .iter()
                .any(|r| r.key == "NEW_KEY" && r.status == "only_b")),
            _ => panic!("expected EnvCompare"),
        }
        let created = view(
            core.dispatch(CoreCommand::CreateEnvFile {
                project_id: pid.clone(),
                file: ".env.testing".into(),
                from: Some(".env.example".into()),
            })
            .unwrap(),
        );
        assert_eq!(created.entries.len(), 2);
        let v = view(
            core.dispatch(CoreCommand::DeleteEnvKey {
                project_id: pid,
                file: ".env.testing".into(),
                key: "NEW_KEY".into(),
            })
            .unwrap(),
        );
        assert_eq!(v.content, "APP_ENV=example\n");
    }

    #[test]
    fn diagnostics_report_a_missing_site_folder_and_remember_ignores() {
        let (core, home) = test_core();
        let gone = home.paths.root().join("gone");
        let domain = Domain {
            hostname: "ghost.test".into(),
            project_id: None,
            root: gone.display().to_string(),
            kind: crate::domain::SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        };
        // Written straight to the store: AddDomain would reject a folder that doesn't exist.
        core.inner().domains.lock().unwrap().add(domain).ok();
        let findings = |core: &Core| match core.dispatch(CoreCommand::RunDiagnostics).unwrap() {
            CoreResponse::Diagnostics { findings } => findings,
            _ => panic!("expected Diagnostics"),
        };
        let found = findings(&core);
        let f = found
            .iter()
            .find(|f| f.id == "site_root_missing:ghost.test")
            .expect("missing-folder finding");
        assert!(!f.ignored && !f.problem.is_empty() && !f.cause.is_empty() && !f.fix.is_empty());

        core.dispatch(CoreCommand::IgnoreDiagnostic {
            id: f.id.clone(),
            ignore: true,
        })
        .unwrap();
        assert!(
            findings(&core)
                .iter()
                .find(|x| x.id == f.id)
                .unwrap()
                .ignored
        );
        core.dispatch(CoreCommand::IgnoreDiagnostic {
            id: f.id.clone(),
            ignore: false,
        })
        .unwrap();
        assert!(
            !findings(&core)
                .iter()
                .find(|x| x.id == f.id)
                .unwrap()
                .ignored
        );
    }

    #[test]
    fn composer_and_venv_info_come_from_the_project_files() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("composer.json"),
            r#"{"require":{"monolog/monolog":"^3.0"}}"#,
        )
        .unwrap();
        let project = match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };
        match core
            .dispatch(CoreCommand::GetComposerInfo {
                project_id: project.id.clone(),
            })
            .unwrap()
        {
            CoreResponse::Composer { info } => assert_eq!(info.packages.len(), 1),
            _ => panic!("expected Composer"),
        }
        match core
            .dispatch(CoreCommand::GetVenv {
                project_id: project.id.clone(),
            })
            .unwrap()
        {
            CoreResponse::Venv { info } => assert!(!info.exists),
            _ => panic!("expected Venv"),
        }
        assert!(core
            .dispatch(CoreCommand::RunComposer {
                project_id: project.id,
                action: "require".into(),
                target: Some("--evil".into())
            })
            .is_err());
    }

    #[test]
    fn discovered_commands_include_package_scripts_with_the_projects_manager() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","build":"vite build"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        let project = match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };
        match core
            .dispatch(CoreCommand::DiscoverCommands {
                project_id: project.id,
            })
            .unwrap()
        {
            CoreResponse::CommandSources { sources } => {
                assert_eq!(sources.len(), 1, "no artisan / composer / manage.py here");
                assert_eq!(sources[0].prefix, ["pnpm", "run"]);
                assert_eq!(sources[0].commands.len(), 2);
            }
            _ => panic!("expected CommandSources"),
        }
    }

    #[test]
    fn renaming_a_domain_keeps_its_settings_and_refuses_taken_names() {
        let (core, home) = test_core();
        let root = home.paths.root().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let domain = |host: &str| Domain {
            hostname: host.into(),
            project_id: None,
            root: root.display().to_string(),
            kind: crate::domain::SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        };
        core.dispatch(CoreCommand::AddDomain {
            domain: domain("old.test"),
        })
        .unwrap();
        core.dispatch(CoreCommand::AddDomain {
            domain: domain("other.test"),
        })
        .unwrap();

        let renamed = core
            .dispatch(CoreCommand::RenameDomain {
                hostname: "old.test".into(),
                new_hostname: "My-Shop.Local".into(),
            })
            .unwrap();
        assert!(
            matches!(renamed, CoreResponse::Domain { domain } if domain.hostname == "my-shop.local" && domain.root == root.display().to_string())
        );
        assert!(core
            .dispatch(CoreCommand::RenameDomain {
                hostname: "my-shop.local".into(),
                new_hostname: "other.test".into()
            })
            .is_err());
        assert!(
            matches!(
                core.dispatch(CoreCommand::GetDomain {
                    hostname: "my-shop.local".into()
                }),
                Ok(CoreResponse::Domain { .. })
            ),
            "a failed rename leaves the site as it was"
        );
        assert!(
            core.dispatch(CoreCommand::RenameDomain {
                hostname: "openlocalserver.test".into(),
                new_hostname: "gone.test".into()
            })
            .is_err(),
            "the built-in home site can't be renamed away"
        );
        assert!(
            core.dispatch(CoreCommand::RemoveDomain {
                hostname: "openlocalserver.test".into()
            })
            .is_err(),
            "the built-in home site can't be deleted"
        );
    }

    /// Laragon-style: scanning a folder gives each servable project `<name>.test`, a
    /// deleted automatic domain stays deleted, and new folders are picked up on the next sync.
    #[test]
    fn scanned_projects_get_automatic_domains() {
        let (core, home) = test_core();
        let www = home.paths.root().join("www");
        for (dir, file) in [
            ("Shop", "index.php"),
            ("site", "index.html"),
            ("api", "package.json"),
        ] {
            std::fs::create_dir_all(www.join(dir)).unwrap();
            std::fs::write(www.join(dir).join(file), "x").unwrap();
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();

        let kinds = |core: &Core| -> Vec<(String, String)> {
            match core.dispatch(CoreCommand::ListDomains).unwrap() {
                CoreResponse::Domains { domains } => {
                    domains.into_iter().map(|d| (d.hostname, d.kind)).collect()
                }
                _ => panic!("expected Domains"),
            }
        };
        let mut found = kinds(&core);
        found.sort();
        assert_eq!(
            found,
            vec![
                ("openlocalserver.test".to_string(), "static".to_string()),
                ("shop.local".to_string(), "php".to_string()),
                ("site.local".to_string(), "static".to_string())
            ],
            "a Node project needs a dev server, so no automatic domain"
        );

        core.dispatch(CoreCommand::RemoveDomain {
            hostname: "shop.local".into(),
        })
        .unwrap();
        std::fs::create_dir_all(www.join("blog")).unwrap();
        std::fs::write(www.join("blog").join("index.php"), "x").unwrap();
        core.dispatch(CoreCommand::SyncAutoDomains).unwrap();
        let hosts: Vec<String> = kinds(&core).into_iter().map(|(h, _)| h).collect();
        assert!(
            hosts.contains(&"blog.local".to_string()),
            "a new folder is picked up"
        );
        assert!(
            !hosts.contains(&"shop.local".to_string()),
            "a deleted automatic domain is not recreated"
        );
    }

    #[test]
    fn ping_returns_pong_with_version() {
        let (core, _home) = test_core();
        match core.dispatch(CoreCommand::Ping).unwrap() {
            CoreResponse::Pong { version } => assert!(!version.is_empty()),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn set_then_get_setting_round_trips_through_dispatch() {
        let (core, _home) = test_core();
        core.dispatch(CoreCommand::SetSetting {
            key: "editor".into(),
            value: Value::String("vscode".into()),
        })
        .unwrap();
        match core
            .dispatch(CoreCommand::GetSetting {
                key: "editor".into(),
            })
            .unwrap()
        {
            CoreResponse::Setting { value, .. } => {
                assert_eq!(value, Some(Value::String("vscode".into())))
            }
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn get_missing_setting_returns_none_not_error() {
        let (core, _home) = test_core();
        match core
            .dispatch(CoreCommand::GetSetting {
                key: "does-not-exist".into(),
            })
            .unwrap()
        {
            CoreResponse::Setting { value, .. } => assert_eq!(value, None),
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn every_command_variant_round_trips_through_json_with_its_snake_case_tag() {
        // The UI builds these as JSON — a renamed field would silently break it.
        let cmd = CoreCommand::StartQuickApp {
            id: "laravel".into(),
            values: BTreeMap::from([("project_name".to_string(), "shop".to_string())]),
            approval: Some("once".into()),
            allow_elevated: false,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "start_quick_app");
        assert_eq!(json["values"]["project_name"], "shop");
        let back: CoreCommand = serde_json::from_value(json).unwrap();
        assert!(matches!(back, CoreCommand::StartQuickApp { .. }));

        let json = serde_json::json!({"type": "read_web_config", "hostname": null, "part": "main"});
        assert!(matches!(
            serde_json::from_value::<CoreCommand>(json).unwrap(),
            CoreCommand::ReadWebConfig {
                part: ConfigPart::Main,
                ..
            }
        ));
    }

    #[test]
    fn domains_reject_injection_in_structured_blocks_and_relative_roots() {
        use crate::domain::{HeaderRule, SiteBlocks, SiteKind};
        let (core, home) = test_core();
        let root = home.paths.root().join("site").display().to_string();
        let mut domain = Domain {
            hostname: "shop.test".into(),
            project_id: None,
            root: root.clone(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        };
        domain.blocks.headers.push(HeaderRule {
            name: "X-Test".into(),
            value: "a\"; } server { listen 1; ".into(),
        });
        assert!(
            core.dispatch(CoreCommand::AddDomain {
                domain: domain.clone()
            })
            .is_err(),
            "a header value must not break out of its directive"
        );

        domain.blocks.headers.clear();
        domain.root = "relative/path".into();
        assert!(core
            .dispatch(CoreCommand::AddDomain {
                domain: domain.clone()
            })
            .is_err());

        domain.root = root;
        assert!(core.dispatch(CoreCommand::AddDomain { domain }).is_ok());
        match core.dispatch(CoreCommand::ListDomains).unwrap() {
            CoreResponse::Domains { domains } => {
                // The seeded openlocalserver.test plus the added shop.test.
                assert_eq!(domains.len(), 2);
                let shop = domains.iter().find(|d| d.hostname == "shop.test").unwrap();
                assert_eq!(shop.url, "http://shop.test/");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn untrusted_quick_apps_refuse_to_run_without_approval() {
        let (core, home) = test_core();
        let file = home.paths.root().join("x.yaml");
        std::fs::write(&file, "id: theirs\nname: Theirs\nvariables:\n  - { name: project_name, required: true }\ncommands:\n  - echo hi\n").unwrap();
        core.dispatch(CoreCommand::ImportQuickApp {
            source: file.display().to_string(),
        })
        .unwrap();

        let values = BTreeMap::from([("project_name".to_string(), "demo".to_string())]);
        let err = core
            .dispatch(CoreCommand::StartQuickApp {
                id: "theirs".into(),
                values,
                approval: None,
                allow_elevated: false,
            })
            .unwrap_err();
        assert!(err.cause.contains("untrusted"), "{err:?}");
    }

    /// The Reverse Proxy Quick App points a domain at something running elsewhere.
    #[test]
    fn reverse_proxy_quick_app_plans_a_domain_to_another_host() {
        let (core, _home) = test_core();
        let values = BTreeMap::from([
            ("domain".to_string(), "portainer.test".to_string()),
            ("target_host".to_string(), "192.168.1.20".to_string()),
            ("target_port".to_string(), "9443".to_string()),
            ("target_https".to_string(), "true".to_string()),
        ]);
        let CoreResponse::QuickPlan { result } = core
            .dispatch(CoreCommand::PlanQuickApp {
                id: "reverse-proxy".into(),
                values,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(result.ok, "{:?}", result.errors);
        let text = serde_json::to_string(&result).unwrap();
        for want in ["create_domain", "portainer.test", "192.168.1.20", "9443"] {
            assert!(text.contains(want), "plan is missing {want}: {text}");
        }
    }

    #[test]
    fn plan_quick_app_reports_field_errors_instead_of_failing() {
        let (core, _home) = test_core();
        match core
            .dispatch(CoreCommand::PlanQuickApp {
                id: "laravel".into(),
                values: BTreeMap::new(),
            })
            .unwrap()
        {
            CoreResponse::QuickPlan { result } => {
                assert!(!result.ok);
                assert!(result.errors.iter().any(|e| e.field == "project_name"));
            }
            _ => panic!(),
        }
    }
}
