use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "olsc", version, about = "OLS from the command line", long_about = None)]
pub struct Cli {
    /// Print the raw JSON response instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Start the web server and the services set to start with the app.
    Start,
    /// Stop the web server, every service, worker and tunnel.
    Stop,
    /// Stop, then start.
    Restart,
    /// What is running.
    Status,
    /// Build this project's environment from its manifest (§73).
    Setup {
        /// Show the plan only; change nothing.
        #[arg(long)]
        dry_run: bool,
        /// The project folder (default: the current folder).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Apply without asking.
        #[arg(short, long)]
        yes: bool,
    },
    /// Check everything OLS depends on (§113).
    Doctor,
    /// Diagnose and fix what is safe (§114); a project name limits it to that project.
    Repair {
        project: Option<String>,
        /// Show the fixes that would run; change nothing.
        #[arg(long)]
        dry_run: bool,
        #[arg(short, long)]
        yes: bool,
    },
    /// Search projects, sites, services, commands, configs and logs.
    Search { query: Vec<String> },
    #[command(subcommand)]
    Project(ProjectCmd),
    #[command(subcommand)]
    Runtime(RuntimeCmd),
    #[command(subcommand)]
    Php(PhpCmd),
    #[command(subcommand)]
    Service(ServiceCmd),
    #[command(subcommand)]
    Domain(ListOnly),
    #[command(subcommand)]
    Certificate(ListOnly),
    #[command(subcommand)]
    Tunnel(TunnelCmd),
    #[command(subcommand, name = "quick-app")]
    QuickApp(ListOnly),
    #[command(subcommand, name = "quick-command")]
    QuickCommand(QuickCommandCmd),
    #[command(subcommand)]
    Worker(WorkerCmd),
    #[command(subcommand)]
    Snapshot(SnapshotCmd),
    /// Plugins: extra runtimes, Quick Apps, detections and health checks (§133).
    #[command(subcommand)]
    Plugin(PluginCmd),
    /// Signed catalogs of runtimes and plugins (§87).
    #[command(subcommand)]
    Catalog(CatalogCmd),
    /// The local HTTP API (§137): status, on/off, token.
    #[command(subcommand)]
    Api(ApiCmd),
    /// Signed updates (§145).
    #[command(subcommand)]
    Update(UpdateCmd),
    /// The Explorer right-click menu (§124).
    #[command(subcommand, name = "shell-menu")]
    ShellMenu(ShellMenuCmd),
    /// Write a redacted bundle for a bug report.
    #[command(name = "support-bundle")]
    SupportBundle { dest: PathBuf },
    /// Whether the internet is reachable.
    Network,
    /// Tests: `olsc test load` runs a k6 load test (§ Stage 18).
    #[command(subcommand)]
    Test(TestCmd),
    /// The AI assistant (opt-in, bring your own model): `olsc ai ask`, `olsc ai explain`.
    #[command(subcommand)]
    Ai(AiCmd),
    /// View the OpenLocalServer comprehensive user manual and operations guide.
    Guide {
        /// Specific version to view (default: current version).
        version: Option<String>,
    },
    // @@cli-cmds
    /// Run the core in the background without the window (started automatically when needed).
    Daemon {
        /// Stop a running daemon.
        #[arg(long)]
        stop: bool,
    },
}

#[derive(Subcommand)]
pub enum ListOnly {
    List,
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    List,
    /// Register a folder as a project.
    Add {
        path: PathBuf,
    },
    /// Forget a project (its files are not touched).
    Remove {
        name: String,
    },
    /// Set it up and start its services and workers.
    Start {
        name: String,
    },
    /// Stop its workers.
    Stop {
        name: String,
    },
    /// Clone a Git repository into a new project.
    Clone {
        url: String,
        path: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum RuntimeCmd {
    List,
    /// Install a runtime or service; the newest available version when none is given.
    Install {
        id: String,
        version: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PhpCmd {
    /// Make a PHP version the default for projects that don't ask for one.
    Use { version: String },
}

#[derive(Subcommand)]
pub enum ServiceCmd {
    List,
    Start {
        id: String,
    },
    Stop {
        id: String,
    },
    Restart {
        id: String,
    },
    /// The service's recent output.
    Logs {
        id: String,
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(Subcommand)]
pub enum TunnelCmd {
    List,
    /// Start a tunnel (by name or project name). Makes the site public.
    Start {
        name: String,
        /// Confirm the first exposure without asking.
        #[arg(short, long)]
        yes: bool,
    },
    Stop {
        name: String,
    },
}

#[derive(Subcommand)]
pub enum QuickCommandCmd {
    List,
    Run {
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WorkerCmd {
    List,
    Start { project: String },
    Stop { project: String },
}

#[derive(Subcommand)]
pub enum SnapshotCmd {
    List {
        project: String,
    },
    Create {
        project: String,
        #[arg(long, default_value = "From the command line")]
        label: String,
        /// Include .env files.
        #[arg(long)]
        env: bool,
        /// Include database data.
        #[arg(long)]
        databases: bool,
        /// Include project files.
        #[arg(long)]
        files: bool,
    },
}

#[derive(Subcommand)]
pub enum PluginCmd {
    List,
    /// Install from a folder or a .zip. It stays off until you enable it.
    Install {
        source: PathBuf,
    },
    /// Turn a plugin on after reviewing the permissions it asks for.
    Enable {
        id: String,
        /// Approve the listed permissions without asking.
        #[arg(short, long)]
        yes: bool,
    },
    Disable {
        id: String,
    },
    Remove {
        id: String,
    },
}

#[derive(Subcommand)]
pub enum CatalogCmd {
    List,
    /// Add a catalog with its publisher's minisign public key.
    Add {
        name: String,
        url: String,
        public_key: String,
    },
    Remove {
        id: String,
    },
    /// Download and verify one catalog, or all.
    Refresh {
        id: Option<String>,
    },
    /// Install a plugin a catalog lists (it stays off until enabled).
    Install {
        catalog: String,
        plugin: String,
    },
}
#[derive(Subcommand)]
pub enum ApiCmd {
    Status,
    /// Turn the API on (needs a token) or off.
    Enable {
        #[arg(long, default_value_t = ols_core::api::DEFAULT_PORT)]
        port: u16,
        /// Allow starting, stopping and applying, not only reading.
        #[arg(long)]
        operate: bool,
    },
    Disable,
    /// Make a new token (shown once; the old one stops working).
    Token,
}

#[derive(Subcommand)]
pub enum UpdateCmd {
    /// Look for a newer version and verify its signature.
    Check,
    /// Download the update and check it against the signed manifest.
    Download,
    /// Start the downloaded installer.
    Install,
}

#[derive(Subcommand)]
pub enum ShellMenuCmd {
    Status,
    Install,
    Remove,
}
#[derive(Subcommand)]
pub enum TestCmd {
    /// Run a k6 script against the project's site. The exit code follows the script's thresholds, so CI can use it.
    Load {
        /// Project name (default: the project in the current folder).
        project: Option<String>,
        /// Script in .openlocalserver/k6 (default: the only one, or a generated smoke test).
        script: Option<String>,
        /// The site's hostname, when the project has several.
        #[arg(long)]
        site: Option<String>,
        /// A ready-made or saved test plan (smoke, load, stress, spike, soak, ...) to write as the script when there is none.
        #[arg(long, default_value = "smoke")]
        profile: String,
        /// A variable for the script, NAME=VALUE (repeat for more). Read in the script as __ENV.NAME.
        #[arg(long = "var")]
        vars: Vec<String>,
        /// Allow testing a public tunnel address.
        #[arg(long)]
        public: bool,
    },
}
#[derive(Subcommand)]
pub enum AiCmd {
    /// Providers, what is turned on, and where each feature goes.
    Status,
    /// Turn the assistant on (it is off until you do; nothing is sent while it is off).
    On,
    Off,
    /// Check that a provider answers and list its models.
    Test {
        provider: Option<String>,
    },
    /// Ask a question about the logs, or with --feature palette, describe what you want set up.
    Ask {
        question: Vec<String>,
        /// logs (default) or palette.
        #[arg(long, default_value = "logs")]
        feature: String,
        /// Read these logs (ids from `olsc status`/the Logs page, e.g. web:error). Default: app and web error.
        #[arg(long = "log")]
        logs: Vec<String>,
        #[command(flatten)]
        run: AiRun,
    },
    /// Explain a diagnostics finding (its id, or part of its text) and propose a fix.
    Explain {
        finding: String,
        #[command(flatten)]
        run: AiRun,
    },
}

#[derive(clap::Args)]
pub struct AiRun {
    /// Send even if the provider is outside this computer.
    #[arg(long)]
    pub yes: bool,
    /// Run the proposed steps without asking (destructive steps still need --destructive).
    #[arg(long)]
    pub apply: bool,
    #[arg(long)]
    pub destructive: bool,
    /// Show what would be sent and stop.
    #[arg(long)]
    pub preview: bool,
}
// @@cli-enums
