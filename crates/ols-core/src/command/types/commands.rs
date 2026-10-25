use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::app::StartupSettings;
use crate::dbtools::ExternalTool;
use crate::domain::{Domain, Ownership};
use crate::process::{ProcessId, ProcessSpec};
use crate::quickapp::commands::QuickCommand;
use crate::web::manager::ConfigPart;
use crate::xdebug::XdebugSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreCommand {
    Ping,
    GetSetting {
        key: String,
    },
    SetSetting {
        key: String,
        value: Value,
    },
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
    RunCommand {
        executable: String,
        args: Vec<String>,
        cwd: Option<String>,
        timeout_ms: u64,
    },
    ListCommandHistory,
    CheckPort {
        port: u16,
    },
    ListRuntimeCatalog,
    RefreshRuntimeCatalog {
        id: String,
    },
    InstallRuntime {
        id: String,
        version: String,
    },
    PauseRuntime {
        id: String,
        version: String,
        paused: bool,
    },
    CancelRuntime {
        id: String,
        version: String,
    },
    RemoveRuntime {
        id: String,
        version: String,
    },
    RegisterProject {
        path: String,
    },
    ScanAndRegisterProjects {
        path: String,
    },
    ListProjects,
    SyncAutoDomains,
    RemoveProject {
        id: String,
    },
    DeleteProject {
        id: String,
    },
    RenameIgnoredFolder {
        old_name: String,
        new_name: String,
    },
    ListDeletedItems,
    ForgetDeletedItem {
        value: String,
    },
    ClearDeletedItems,
    GetProjectDetail {
        id: String,
    },
    InspectSiteFolder {
        path: String,
    },
    RunInProject {
        project_id: String,
        runtime_id: String,
        args: Vec<String>,
    },
    ListWpProjects,
    WpSignIn {
        project_id: String,
        hostname: Option<String>,
        theme: Option<String>,
    },
    WpSignInRevoke {
        project_id: String,
    },
    ListServices,
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
    StopAll,
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
    ListDbTools,
    OpenDbTool {
        id: String,
    },
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
    ScanPhpFolder {
        dir: String,
    },
    ListPhpExtensions {
        version: String,
    },
    SetPhpExtension {
        version: String,
        name: String,
        enabled: bool,
    },
    InstallPhpExtension {
        version: String,
        name: String,
    },
    ListPeclPackages,
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
    DeleteSite {
        hostname: String,
    },
    SetDomainEnabled {
        hostname: String,
        enabled: bool,
    },
    BulkSetDomainsEnabled {
        hostnames: Vec<String>,
        enabled: bool,
    },
    BulkRemoveDomains {
        hostnames: Vec<String>,
    },
    BulkDeleteSites {
        hostnames: Vec<String>,
    },
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
    SetDomainIcon {
        hostname: String,
        icon: Option<String>,
    },
    SetSiteIconImage {
        hostname: Option<String>,
        folder: Option<String>,
        webp_base64: String,
    },
    RemoveSiteIconImage {
        hostname: Option<String>,
        folder: Option<String>,
    },
    ReadImageFile {
        path: String,
    },
    SuggestDomain {
        project_id: String,
        template: String,
    },
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
    SyncDdns {
        hostname: String,
        force: bool,
    },
    DetectPublicIp,
    SyncAllDdns {
        force: bool,
    },
    HealthCheck {
        hostname: String,
    },
    ListWebConfigs,
    ReadWebConfig {
        hostname: Option<String>,
        part: ConfigPart,
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
    CreateDatabase {
        engine: String,
        name: String,
        user: Option<String>,
        password: Option<String>,
    },
    ListDatabases {
        engine: String,
    },
    ListDatabaseTables {
        engine: String,
        database: String,
    },
    ExecuteSql {
        engine: String,
        database: Option<String>,
        sql: String,
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
    BackupDatabase {
        engine: String,
        database: String,
    },
    ListDbBackups {
        engine: String,
        database: Option<String>,
    },
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
    OpenWith {
        path: String,
        app: String,
    },
    ListProjectShortcuts {
        project_id: String,
    },
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
    ListEditors,
    GetSystemStats,
    ListMigrationSources,
    ListForeignDatabases {
        source_id: String,
        password: String,
    },
    MigrateDatabases {
        source_id: String,
        password: String,
        databases: Vec<String>,
        target: String,
    },
    GetMigrationProgress,
    GetHelperService,
    InstallHelperService,
    UninstallHelperService,
    GetXdebug {
        version: String,
    },
    SetXdebug {
        version: String,
        settings: XdebugSettings,
    },
    XdebugIdeConfig {
        project_id: String,
        ide: String,
        version: String,
    },
    DiscoverCommands {
        project_id: String,
    },
    GetComposerInfo {
        project_id: String,
    },
    RunComposer {
        project_id: String,
        action: String,
        target: Option<String>,
    },
    GetPackageManagers {
        project_id: String,
    },
    EnablePackageManager {
        project_id: String,
        manager: String,
    },
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
    RunDiagnostics,
    IgnoreDiagnostic {
        id: String,
        ignore: bool,
    },
    RestartService {
        id: String,
    },
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
    CreateEnvFile {
        project_id: String,
        file: String,
        from: Option<String>,
    },
    ListOperations,
    DismissOperation {
        id: u64,
    },
    MailpitEnvPlan {
        project_id: String,
        file: String,
    },
    ApplyMailpitEnv {
        project_id: String,
        file: String,
    },
    MailDiagnostics {
        project_id: Option<String>,
    },
    SendTestMail {
        to: String,
    },
    GetManifest {
        project_id: String,
    },
    SaveManifest {
        project_id: String,
        manifest: Option<crate::manifest::EnvironmentManifest>,
    },
    SaveManifestText {
        project_id: String,
        text: String,
    },
    PlanSetup {
        project_id: String,
    },
    ApplySetup {
        project_id: String,
        dry_run: bool,
    },
    GetSetupProgress,
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
    ExportSites {
        hostnames: Vec<String>,
        options: crate::sitebundle::BundleOptions,
        dest: String,
        password: Option<String>,
    },
    PreviewSiteBundle {
        source: String,
    },
    ImportSites {
        source: String,
        password: Option<String>,
        on_conflict: crate::sitebundle::OnConflict,
        name: Option<String>,
        options: crate::snapshots::RestoreOptions,
    },
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
    GetAutoBackup,
    SetAutoBackup {
        settings: crate::auto_backup::AutoBackupSettings,
    },
    SetSiteAutoBackup {
        hostname: String,
        site: crate::auto_backup::AutoBackupSiteSettings,
    },
    RunAutoBackup,
    ListTunnelProviders,
    ListTunnels,
    SaveTunnel {
        tunnel: crate::tunnel::TunnelConfig,
    },
    RemoveTunnel {
        id: String,
    },
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
    GlobalSearch {
        query: String,
    },
    Doctor,
    DiagnoseProject {
        project_id: String,
    },
    PlanRepair {
        project_id: Option<String>,
    },
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
    GitSetCredentials {
        host: String,
        username: String,
        token: Option<String>,
    },
    ListSshKeys,
    GitClone {
        url: String,
        target: String,
        branch: Option<String>,
        auth: Option<crate::git::GitAuth>,
    },
    ListPlugins,
    InstallPlugin {
        source: String,
    },
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
    GetApiStatus,
    SetApiSettings {
        enabled: bool,
        port: u16,
        mode: String,
    },
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
    LoadListProfiles,
    LoadSaveProfile {
        profile: crate::loadtest::LoadProfile,
    },
    LoadDeleteProfile {
        id: String,
    },
    LoadGenerate {
        project_id: String,
        profile: crate::loadtest::LoadProfile,
        name: Option<String>,
    },
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
    AiGetState,
    AiSaveSettings {
        enabled: bool,
        features: BTreeMap<String, String>,
    },
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
    AiProbe {
        provider: crate::ai::AiProvider,
        api_key: Option<String>,
    },
    AiPreview {
        request: crate::ai::AiRequest,
    },
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
    AiApply {
        actions: Vec<CoreCommand>,
        confirm_destructive: bool,
    },
    GetAppGuide {
        version: Option<String>,
    },
}
