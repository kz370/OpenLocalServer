use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{DomainSummary, HealthItem, LogSource, QuickPlanResult, StartupSettings};
use crate::certs::{CaInfo, CertInfo};
use crate::custom_install::CustomInstall;
use crate::dbtools::{DbTool, ExternalTool};
use crate::domain::Domain;
use crate::health::HealthReport;
use crate::port::PortStatus;
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo};
use crate::project::{Project, ProjectDetail};
use crate::quickapp::commands::{HistoryEntry, QuickCommand};
use crate::quickapp::{EntryDetail, EntryView, RunView};
use crate::runtime::CatalogEntry;
use crate::service::{ConnectionInfo, DbUser, ServiceStatus};
use crate::sqlite::{IntegrityResult, SqliteInfo};
use crate::web::manager::{ApplyReport, ConfigFile, ConfigVersion, WebStatus};
use crate::web::WebConfig;
use crate::xdebug::XdebugReport;

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
    /// What `StopAll` could not stop: the names of processes still alive after the.
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
    /// The one-time link to open in the browser. It carries the token, so this response is.
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
    /// What a bulk site edit did: how many sites changed, and one line per site it.
    Bulk {
        changed: usize,
        skipped: Vec<String>,
    },
    HelperService {
        installed: bool,
        /// The version the resident service reports, when it answers.
        version: Option<String>,
        /// SemVer comparison of the service copy against this app: `development_version`.
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
        /// Whether a wanted CPU limit can actually be applied on this machine, so the.
        cpu: crate::resources::CpuCapStatus,
    },
    /// §159: the plan, when it next runs, how many automatic backups are kept, and the.
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
