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
use crate::service::{ConnectionInfo, DbUser, ServiceAggregate, ServiceStatus};
use crate::sqlite::{IntegrityResult, SqliteInfo};
use crate::web::manager::{ApplyReport, ConfigFile, ConfigVersion, WebStatus};
use crate::web::WebConfig;
use crate::xdebug::XdebugReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSiteResult {
    pub domain: Option<Box<Domain>>,
    pub needs_setup: bool,
    pub suggested_hostname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub services: Vec<ServiceStatus>,
    pub service_aggregate: ServiceAggregate,
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
    SiteIconLibrary {
        icons: Vec<crate::app::SiteIconLibraryEntry>,
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
        /// Project ids without a connected site from projects_view; None enables legacy UI logic.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unconnected_project_ids: Option<Vec<String>>,
        /// Project ids whose site is hidden by `.ols_ignore_site`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        site_ignored_project_ids: Option<Vec<String>>,
    },
    ProjectDetail {
        detail: Box<ProjectDetail>,
    },
    CreateSiteForProject {
        domain: Option<Box<Domain>>,
        needs_setup: bool,
        suggested_hostname: String,
    },
    ProjectTypes {
        types: Vec<crate::project::ProjectTypePreset>,
    },
    ProjectFolderInspection {
        inspection: crate::app::projects::manage::ProjectFolderInspection,
    },
    CustomProjectTypes {
        types: Vec<crate::project::CustomProjectType>,
    },
    PendingAddProject {
        path: Option<String>,
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
    WpCoreRepair {
        result: crate::wordpress::WpCoreRepair,
    },
    DeletedItems {
        items: Vec<crate::app::DeletedItem>,
    },
    Services {
        services: Vec<ServiceStatus>,
    },
    ServiceAggregate {
        aggregate: ServiceAggregate,
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
    XdebugProfiles {
        files: Vec<crate::profiler::ProfileFile>,
    },
    XdebugProfile {
        profile: Box<crate::profiler::Profile>,
    },
    DefenderStatus {
        state: crate::defender::DefenderState,
        excluded_by_ols: Vec<String>,
        suggestions: Vec<String>,
        dev_drive: Vec<crate::defender::DevDriveInfo>,
    },
    SiteImportCandidates {
        candidates: Vec<crate::migrate_sites::SiteImportCandidate>,
    },
    PhpUpgradeReport {
        report: Option<crate::php_upgrade::PhpUpgradeReport>,
    },
    ReclaimReport {
        report: Option<crate::reclaim::ReclaimReport>,
    },
    GuardSnapshots {
        snapshots: Vec<crate::migration_guard::GuardSnapshot>,
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
    MigrationSource {
        source: crate::migrate::MigrationSource,
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
    ExcludedSiteMoved {
        hostname: String,
        source: String,
        destination: String,
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
    DockerStatus {
        status: crate::docker::DockerStatus,
    },
    DockerContainers {
        containers: Vec<crate::docker::ContainerSummary>,
    },
    DockerLogs {
        lines: Vec<String>,
    },
    DockerComposeStacks {
        stacks: Vec<crate::docker::compose::ComposeStack>,
    },
    DockerComposeFile {
        path: String,
        content: String,
    },
    DockerComposeCheck {
        check: crate::docker::compose::ComposeCheck,
    },
    DockerComposePresets {
        presets: Vec<crate::docker::builder::ServicePreset>,
    },
    DockerizePlan {
        plan: crate::docker::dockerize::DockerizePlan,
    },
    DockerComposePlan {
        plan: crate::docker::builder::BuildPlan,
    },
    DockerContainerDetails {
        details: crate::docker::details::ContainerDetails,
    },
    DockerImages {
        images: Vec<crate::docker::storage::ImageSummary>,
    },
    DockerVolumes {
        volumes: Vec<crate::docker::storage::VolumeSummary>,
    },
    DockerDiskUsage {
        usage: crate::docker::storage::DiskUsage,
    },
    DockerPrunePlan {
        plan: crate::docker::storage::PrunePlan,
    },
    /// A container was created and started.
    DockerRun {
        id: String,
    },
    DockerComposeConnect {
        info: crate::docker::connect::ConnectInfo,
    },
    DockerComposeSecrets {
        secrets: Vec<(String, String)>,
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
