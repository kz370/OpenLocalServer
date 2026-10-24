use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{DomainSummary, HealthItem, LogSource, QuickPlanResult, StartupSettings};
use crate::certs::{CaInfo, CertInfo};
use crate::custom_install::CustomInstall;
use crate::dbtools::{DbTool, ExternalTool};
use crate::domain::{Domain, Ownership};
use crate::health::HealthReport;
use crate::port::PortStatus;
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo, ProcessSpec};
use crate::project::{Project, ProjectDetail};
use crate::quickapp::commands::{HistoryEntry, QuickCommand};
use crate::quickapp::{EntryDetail, EntryView, RunView};
use crate::runtime::CatalogEntry;
use crate::service::{ConnectionInfo, DbUser, ServiceStatus};
use crate::sqlite::{IntegrityResult, SqliteInfo};
use crate::web::WebConfig;
use crate::web::manager::{ApplyReport, ConfigFile, ConfigPart, ConfigVersion, WebStatus};
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
    /// Hold an in-flight download where it is, or let it continue. The partial file is
    /// kept, so resuming finishes the same transfer instead of starting over.
    PauseRuntime {
        id: String,
        version: String,
        paused: bool,
    },
    /// Stop an in-flight install. The partial download is deleted and nothing is
    /// installed; this is a user choice, not a failure, so it is not reported as one.
    CancelRuntime {
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
    /// Deletes a project by moving its folder into an `ols_excluded_sites` folder
    /// in the same directory instead of permanently deleting it.
    DeleteProject {
        id: String,
    },
    /// Renames a folder in the ignored list and renames matching folders in root directories.
    RenameIgnoredFolder {
        old_name: String,
        new_name: String,
    },
    /// The deletion history shown in Settings: project folders and automatic sites the
    /// user deleted, newest first. Empty when nothing has been deleted.
    ListDeletedItems,
    /// Drops one entry from the deletion history so scans and automatic domains see that
    /// folder again. Files on disk are never touched.
    ForgetDeletedItem {
        value: String,
    },
    /// Empties the deletion history at once. Returns how many entries were dropped.
    ClearDeletedItems,
    GetProjectDetail {
        id: String,
    },
    /// Inspects a folder before or during site creation/editing (e.g. Node / Python / PHP)
    /// to detect its framework, scripts, package managers, virtual environments and entry points.
    InspectSiteFolder {
        path: String,
    },
    /// Runtime-aware terminal (§19): runs `runtime_id`'s resolved binary for this
    /// project, with its bin dir first on PATH, and streams output like any other
    /// managed process (reuses the Process Supervisor — same live-output UI).
    RunInProject {
        project_id: String,
        runtime_id: String,
        args: Vec<String>,
    },
    /// Every registered project that is a WordPress install, with the site that serves
    /// it. The Sites page reads this to decide which rows get a "WP Admin" action; the
    /// answer comes from the folder on disk, never from the project's name.
    ListWpProjects,
    /// Writes a one-time admin sign-in bridge for a WordPress project and answers with the
    /// link to open. No password is read, asked for or changed: WordPress turns a random,
    /// short-lived token into a session cookie itself, once, from this machine.
    WpSignIn {
        project_id: String,
        /// Which of the project's sites to open. `None` takes the first enabled one.
        hostname: Option<String>,
        /// The colourway the app is showing ("light" | "dark"), so the WordPress sign-in
        /// screen matches the window that opened it. `None` leaves it to the browser.
        theme: Option<String>,
    },
    /// Cancels a pending one-time sign-in link. Removes the temporary helper file, so a
    /// link that was never clicked cannot be used later.
    WpSignInRevoke {
        project_id: String,
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
    /// Stop all running services, web servers, workers, and supervised processes.
    StopAll,

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

    // Custom install locations (Stage 5, user-requested): point OLS at a tool or
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
    /// Deletes a site by moving its project folder into an `ols_excluded_sites` folder
    /// in the same directory instead of permanently deleting it.
    DeleteSite {
        hostname: String,
    },
    SetDomainEnabled {
        hostname: String,
        enabled: bool,
    },
    /// One batch, one apply: a group of sites is turned on or off together. Sites that
    /// cannot change (the built-in home site going off, a hostname that is no longer
    /// there) are reported in the `Bulk` response instead of failing the whole edit.
    BulkSetDomainsEnabled {
        hostnames: Vec<String>,
        enabled: bool,
    },
    /// Deletes a group of sites in one go. Destructive: it skips the built-in home site
    /// and names it in the response rather than refusing the batch.
    BulkRemoveDomains {
        hostnames: Vec<String>,
    },
    /// Deletes a group of sites by moving their folders into `ols_excluded_sites` in their respective directories.
    BulkDeleteSites {
        hostnames: Vec<String>,
    },
    /// Pins a group of sites to one web server, or puts them all back on the default
    /// when `server` is `None`. An unknown server id refuses the whole batch before
    /// anything is written.
    BulkSetDomainServer {
        hostnames: Vec<String>,
        server: Option<String>,
    },
    DuplicateDomain {
        hostname: String,
        new_hostname: String,
    },
    RenameDomain {
        hostname: String,
        new_hostname: String,
    },
    /// Update or clear a site's custom Font Awesome icon identifier.
    SetDomainIcon {
        hostname: String,
        icon: Option<String>,
    },
    /// Save a WebP icon image into `<project_folder>/ols_site_icon.webp`.
    SetSiteIconImage {
        hostname: Option<String>,
        folder: Option<String>,
        webp_base64: String,
    },
    /// Remove `<project_folder>/ols_site_icon.webp`.
    RemoveSiteIconImage {
        hostname: Option<String>,
        folder: Option<String>,
    },
    /// Reads an arbitrary image from disk into a data URL for client preview & WebP conversion.
    ReadImageFile {
        path: String,
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
    VerifySslFiles {
        cert_path: String,
        key_path: String,
    },
    IssueLetsEncrypt {
        hostname: String,
    },
    /// Updates the DNS A-record for a domain with dynamic IP provider (only when IP changes unless forced).
    SyncDdns {
        hostname: String,
        force: bool,
    },
    /// Queries the server's public IP using OS commands.
    DetectPublicIp,
    /// Synchronizes all enabled domains with configured DDNS providers.
    SyncAllDdns {
        force: bool,
    },
    HealthCheck {
        hostname: String,
    },

    // ---- Stage 9: config management ---------------------------------------------
    ListWebConfigs,
    ReadWebConfig {
        hostname: Option<String>,
        part: ConfigPart,
        /// Which server's file to read. Every server has its own main config, so a main
        /// file (no hostname) is only identified by its server. Omitted means the
        /// default server, which is what a site file resolves to anyway.
        server: Option<String>,
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
        /// `None` = connect as the engine's root/postgres superuser, which is how the
        /// bundled servers are provisioned. `Some` = also create this user, owned by the
        /// new database, and remember its password in the OS credential store.
        user: Option<String>,
        password: Option<String>,
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

    // ---- Site bundles: exporting and importing whole sites (§165) -------------------
    /// Writes one bundle for the picked sites and answers at once with the task id to
    /// watch it by. `password` seals the `.env` files and database dumps.
    ExportSites {
        hostnames: Vec<String>,
        options: crate::sitebundle::BundleOptions,
        /// A folder; one bundle per export, named after the site(s).
        dest: String,
        password: Option<String>,
    },
    /// Reads a bundle for review, before anything is created.
    PreviewSiteBundle {
        source: String,
    },
    /// Imports a bundle. `on_conflict` decides what happens to a hostname that already
    /// exists; updating one takes a snapshot of the current state first.
    ImportSites {
        source: String,
        password: Option<String>,
        on_conflict: crate::sitebundle::OnConflict,
        /// The base name for a renamed import; blank means "pick a free one".
        name: Option<String>,
        options: crate::snapshots::RestoreOptions,
    },
    /// §164: the background task list, newest first.
    ListTasks,
    CancelTask {
        id: String,
    },
    DismissTask {
        id: String,
    },
    ClearFinishedTasks,
    GetResourceLimits,
    SetResourceLimits {
        limits: crate::resources::ResourceLimits,
    },
    /// §130: the automatic backup plan, when it next runs, and the last pass.
    GetAutoBackup,
    SetAutoBackup {
        settings: crate::auto_backup::AutoBackupSettings,
    },
    /// One site's own backup settings, made in that site's Backups page.
    SetSiteAutoBackup {
        hostname: String,
        site: crate::auto_backup::AutoBackupSiteSettings,
    },
    /// Runs the automatic backup pass right now, on the same rules the clock would use.
    RunAutoBackup,

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
    GetAppGuide {
        version: Option<String>,
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
    ImageFile {
        data_url: String,
    },
    /// What `StopAll` could not stop: the names of processes still alive after the
    /// sweep. Empty means everything stopped, which is the only case where the status
    /// mark is expected to turn red.
    StoppedEverything {
        still_running: Vec<String>,
    },
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
    SiteFolderInspection {
        inspection: crate::detection::SiteFolderInspection,
    },
    WpProjects {
        projects: Vec<crate::wordpress::WpProject>,
    },
    /// The one-time link to open in the browser. It carries the token, so this response is
    /// never logged, cached or written to disk by the app.
    WpSignIn {
        url: String,
        expires_at: u64,
    },
    DeletedItems {
        items: Vec<crate::app::DeletedItem>,
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
    Certificate {
        cert: Box<CertInfo>,
    },
    SslVerification {
        valid: bool,
        issuer: String,
        sans: Vec<String>,
        expires_at: u64,
        days_left: i64,
    },
    DdnsSync {
        result: crate::ddns::DdnsSyncResult,
    },
    DdnsSyncAll {
        results: Vec<crate::ddns::DdnsSyncResult>,
    },
    PublicIp {
        ip: String,
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
    /// What a bulk site edit did: how many sites changed, and one line per site it
    /// refused. `skipped` is never silently dropped — a batch that landed on twenty of
    /// twenty-five sites has to say which five it left alone and why.
    Bulk {
        changed: usize,
        skipped: Vec<String>,
    },
    HelperService {
        installed: bool,
        /// The version the resident service reports, when it answers.
        version: Option<String>,
        /// SemVer comparison of the service copy against this app: `development_version`
        /// when the service is newer (dev build), `up_to_date` when they match, or
        /// `update_available` when the service is older. `None` when the service is not
        /// answering, which is "not installed", not "out of date".
        status: Option<crate::updater::UpdateStatus>,
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
    /// A background task was started; watch it on the Processes page.
    TaskStarted {
        id: String,
    },
    Tasks {
        tasks: Vec<crate::tasks::TaskView>,
    },
    SiteBundlePreview {
        preview: Box<crate::sitebundle::BundlePreview>,
    },
    SiteBundleExported {
        summary: Box<crate::sitebundle::BundleSummary>,
    },
    SiteBundleImported {
        result: Box<crate::sitebundle::BundleImportResult>,
    },
    Resources {
        limits: crate::resources::ResourceLimits,
        /// Whether a wanted CPU limit can actually be applied on this machine, so the
        /// Resources card can say so instead of claiming Windows has no CPU cap.
        cpu: crate::resources::CpuCapStatus,
    },
    /// §159: the plan, when it next runs, how many automatic backups are kept, and the
    /// last pass's result.
    AutoBackup {
        status: Box<crate::auto_backup::AutoBackupStatus>,
    },
    AutoBackupRun {
        run: Box<crate::auto_backup::AutoBackupRun>,
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
    AppGuide {
        markdown: String,
        version: String,
    },
    // @@responses-end
}
