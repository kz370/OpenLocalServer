//! `ols`: the OpenLocalServer command line (§136).
//!
//! Every command is a `CoreCommand` sent over the local control channel to whoever owns
//! the core: the desktop app when it is open, otherwise a background `ols daemon` that is
//! started on demand. The CLI never manages anything itself (architecture decision 1).

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use ols_core::command::{CoreCommand, CoreResponse};
use ols_core::control::{self, ClientError};
use ols_core::{AppPaths, Diagnostic};

#[derive(Parser)]
#[command(name = "ols", version, about = "OpenLocalServer from the command line", long_about = None)]
struct Cli {
    /// Print the raw JSON response instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// Check everything OpenLocalServer depends on (§113).
    Doctor,
    /// Diagnose and fix what is safe (§114); a project name limits it to that project.
    Repair {
        project: Option<String>,
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
    /// Tests: `ols test load` runs a k6 load test (§ Stage 18).
    #[command(subcommand)]
    Test(TestCmd),
    // @@cli-cmds
    /// Run the core in the background without the window (started automatically when needed).
    Daemon {
        /// Stop a running daemon.
        #[arg(long)]
        stop: bool,
    },
}

#[derive(Subcommand)]
enum ListOnly {
    List,
}

#[derive(Subcommand)]
enum ProjectCmd {
    List,
    /// Register a folder as a project.
    Add { path: PathBuf },
    /// Forget a project (its files are not touched).
    Remove { name: String },
    /// Set it up and start its services and workers.
    Start { name: String },
    /// Stop its workers.
    Stop { name: String },
    /// Clone a Git repository into a new project.
    Clone { url: String, path: PathBuf },
}

#[derive(Subcommand)]
enum RuntimeCmd {
    List,
    /// Install a runtime or service; the newest available version when none is given.
    Install { id: String, version: Option<String> },
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Make a PHP version the default for projects that don't ask for one.
    Use { version: String },
}

#[derive(Subcommand)]
enum ServiceCmd {
    List,
    Start { id: String },
    Stop { id: String },
    Restart { id: String },
    /// The service's recent output.
    Logs {
        id: String,
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum TunnelCmd {
    List,
    /// Start a tunnel (by name or project name). Makes the site public.
    Start {
        name: String,
        /// Confirm the first exposure without asking.
        #[arg(short, long)]
        yes: bool,
    },
    Stop { name: String },
}

#[derive(Subcommand)]
enum QuickCommandCmd {
    List,
    Run {
        id: String,
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkerCmd {
    List,
    Start { project: String },
    Stop { project: String },
}

#[derive(Subcommand)]
enum SnapshotCmd {
    List { project: String },
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
enum PluginCmd {
    List,
    /// Install from a folder or a .zip. It stays off until you enable it.
    Install { source: PathBuf },
    /// Turn a plugin on after reviewing the permissions it asks for.
    Enable {
        id: String,
        /// Approve the listed permissions without asking.
        #[arg(short, long)]
        yes: bool,
    },
    Disable { id: String },
    Remove { id: String },
}

#[derive(Subcommand)]
enum CatalogCmd {
    List,
    /// Add a catalog with its publisher's minisign public key.
    Add { name: String, url: String, public_key: String },
    Remove { id: String },
    /// Download and verify one catalog, or all.
    Refresh { id: Option<String> },
    /// Install a plugin a catalog lists (it stays off until enabled).
    Install { catalog: String, plugin: String },
}
#[derive(Subcommand)]
enum ApiCmd {
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
enum UpdateCmd {
    /// Look for a newer version and verify its signature.
    Check,
    /// Download the update and check it against the signed manifest.
    Download,
    /// Start the downloaded installer.
    Install,
}

#[derive(Subcommand)]
enum ShellMenuCmd {
    Status,
    Install,
    Remove,
}
#[derive(Subcommand)]
enum TestCmd {
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
// @@cli-enums

struct Ctx {
    paths: AppPaths,
    json: bool,
}

type R<T> = Result<T, String>;

fn diag(d: Diagnostic) -> String {
    let mut s = d.problem;
    if !d.cause.is_empty() {
        s.push_str(&format!("\n  {}", d.cause));
    }
    if let Some(f) = d.fix {
        s.push_str(&format!("\n  → {f}"));
    }
    s
}

#[cfg(windows)]
fn spawn_daemon() -> R<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| format!("could not start the background service: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_daemon() -> R<()> {
    Err("the background service is Windows-only for now".into())
}

impl Ctx {
    /// Sends a command, starting the background daemon first if nothing is running.
    fn call(&self, cmd: CoreCommand) -> R<CoreResponse> {
        match control::send(&self.paths, cmd.clone()) {
            Ok(r) => r.map_err(diag),
            Err(ClientError::NotRunning) => {
                eprintln!("Starting OpenLocalServer in the background…");
                spawn_daemon()?;
                let started = Instant::now();
                while control::is_running(&self.paths).is_none() {
                    if started.elapsed() > Duration::from_secs(30) {
                        return Err("the background service did not start (see the log in the data folder)".into());
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                control::send(&self.paths, cmd).map_err(|e| e.to_string())?.map_err(diag)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn print_json(&self, r: &CoreResponse) -> bool {
        if self.json {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
        self.json
    }

    fn project_id(&self, name: &str) -> R<String> {
        let CoreResponse::Projects { projects } = self.call(CoreCommand::ListProjects)? else { return Err("unexpected reply".into()) };
        projects
            .iter()
            .find(|p| p.id == name || p.name.eq_ignore_ascii_case(name))
            .map(|p| p.id.clone())
            .ok_or_else(|| format!("no project named \"{name}\" (see `ols project list`)"))
    }

    /// The project for a folder, registering it when it isn't one yet.
    fn project_for_path(&self, path: &Path) -> R<(String, String)> {
        let full = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let full = full.display().to_string();
        let full = full.strip_prefix(r"\\?\").unwrap_or(&full).to_string();
        let CoreResponse::Projects { projects } = self.call(CoreCommand::ListProjects)? else { return Err("unexpected reply".into()) };
        if let Some(p) = projects.iter().find(|p| p.path.eq_ignore_ascii_case(&full)) {
            return Ok((p.id.clone(), p.name.clone()));
        }
        match self.call(CoreCommand::RegisterProject { path: full })? {
            CoreResponse::Project { project } => {
                eprintln!("Registered {} as a project.", project.name);
                Ok((project.id, project.name))
            }
            _ => Err("unexpected reply".into()),
        }
    }
}

fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn table(rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols).map(|c| rows.iter().map(|r| r.get(c).map(|s| s.chars().count()).unwrap_or(0)).max().unwrap_or(0)).collect();
    for r in rows {
        let line: Vec<String> = r.iter().enumerate().map(|(i, s)| if i + 1 == r.len() { s.clone() } else { format!("{s:<w$}", w = widths[i]) }).collect();
        println!("{}", line.join("  ").trim_end());
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = AppPaths::resolve();
    if let Err(e) = paths.ensure_dirs() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Cmd::Daemon { stop } = &cli.cmd {
        return daemon(&paths, *stop);
    }
    let ctx = Ctx { paths, json: cli.json };
    match run(&ctx, cli.cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn daemon(paths: &AppPaths, stop: bool) -> ExitCode {
    if stop {
        match control::is_running(paths) {
            Some(i) if i.kind == "daemon" => {
                control::take_over_from_daemon(paths);
                println!("The background service stopped.");
            }
            Some(_) => println!("The desktop app is running; there is no background service to stop."),
            None => println!("Nothing is running."),
        }
        return ExitCode::SUCCESS;
    }
    if let Some(i) = control::is_running(paths) {
        eprintln!("OpenLocalServer is already running ({}, process {}).", i.kind, i.pid);
        return ExitCode::FAILURE;
    }
    ols_core::logging::init(&paths.logs_dir());
    let settings = match ols_core::SettingsService::load(paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let core = ols_core::Core::new(settings, paths.clone());
    let mut server = match control::serve(core.clone(), paths, "daemon") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("ols daemon running");
    ols_core::scheduler::start_clock(core.inner());
    server.wait_for_shutdown();
    tracing::info!("ols daemon handing over / stopping");
    let i = core.inner();
    i.stop_all_tunnels();
    i.stop_all_workers();
    i.terminals.close_all();
    i.web.stop();
    for s in core.services().list() {
        if s.running {
            core.services().stop(&s.id);
        }
    }
    control::close(paths);
    ExitCode::SUCCESS
}

fn run(ctx: &Ctx, cmd: Cmd) -> R<()> {
    match cmd {
        Cmd::Daemon { .. } => unreachable!(),
        Cmd::Start => {
            let r = ctx.call(CoreCommand::ApplyWeb { overwrite: vec![] })?;
            if !ctx.print_json(&r) {
                println!("Web server started.");
            }
            if let CoreResponse::Startup { settings } = ctx.call(CoreCommand::GetStartupSettings)? {
                for id in settings.autostart_services {
                    match ctx.call(CoreCommand::StartService { id: id.clone() }) {
                        Ok(_) => println!("Started {id}."),
                        Err(e) if e.contains("already running") => {}
                        Err(e) => eprintln!("{id}: {e}"),
                    }
                }
            }
        }
        Cmd::Stop => stop_everything(ctx)?,
        Cmd::Restart => {
            stop_everything(ctx)?;
            std::thread::sleep(Duration::from_millis(800));
            run(ctx, Cmd::Start)?;
        }
        Cmd::Status => {
            let r = ctx.call(CoreCommand::GetDashboard)?;
            if ctx.print_json(&r) {
                return Ok(());
            }
            let CoreResponse::Dashboard { data } = r else { return Ok(()) };
            println!("Web server: {} ({})", if data.web.running { "running" } else { "stopped" }, data.web.server);
            let mut rows = vec![vec!["SERVICE".into(), "STATE".into(), "PORT".into()]];
            for s in data.services.iter().filter(|s| s.installed) {
                rows.push(vec![s.name.clone(), if s.running { "running".into() } else { "stopped".into() }, s.port.map(|p| p.to_string()).unwrap_or_default()]);
            }
            table(rows);
            println!("{} project(s), {} site(s)", data.project_count, data.domains.len());
            if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
                for t in tunnels.iter().filter(|t| t.state == "connected" || t.state == "starting") {
                    println!("PUBLIC: {} → {}", t.config.target, t.public_url.clone().unwrap_or_else(|| "connecting…".into()));
                }
            }
        }
        Cmd::Setup { dry_run, path, yes } => setup(ctx, path.unwrap_or_else(|| PathBuf::from(".")), dry_run, yes)?,
        Cmd::Doctor => {
            let r = ctx.call(CoreCommand::Doctor)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::DoctorReport { report } = r {
                    print!("{}", ols_core::repair::doctor_text(&report));
                    if report.errors > 0 {
                        return Err(format!("{} error(s) found; `ols repair` fixes what it safely can", report.errors));
                    }
                }
            }
        }
        Cmd::Repair { project, yes } => {
            let pid = project.map(|p| ctx.project_id(&p)).transpose()?;
            let CoreResponse::RepairPlan { plan } = ctx.call(CoreCommand::PlanRepair { project_id: pid.clone() })? else { return Ok(()) };
            if plan.findings.is_empty() {
                println!("Nothing to repair.");
                return Ok(());
            }
            for f in &plan.findings {
                println!("• {}\n    Cause: {}\n    Fix: {}{}", f.problem, f.cause, f.fix, if f.fix_command.is_some() { " (automatic)" } else { "" });
            }
            let safe = plan.actions.iter().filter(|a| !a.destructive).count();
            if safe == 0 {
                println!("\nNothing here can be fixed automatically.");
                return Ok(());
            }
            if !yes && !confirm(&format!("\nApply {safe} safe fix(es)?")) {
                println!("Nothing changed. Run with --yes to apply.");
                return Ok(());
            }
            let r = ctx.call(CoreCommand::ApplyRepair { project_id: pid, ids: vec![], confirm_destructive: false })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::RepairReport { report } = r {
                    for s in report.steps {
                        println!("{} {}{}", if s.ok { "✓" } else { "✗" }, s.label, if s.ok { String::new() } else { format!(" ({})", s.detail) });
                    }
                    println!("{} fixed, {} left.", report.fixed, report.after.len());
                }
            }
        }
        Cmd::Search { query } => {
            let r = ctx.call(CoreCommand::GlobalSearch { query: query.join(" ") })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::SearchResults { hits } = r {
                    if hits.is_empty() {
                        println!("Nothing found.");
                    }
                    table(hits.into_iter().map(|h| vec![h.kind, h.title, h.excerpt.unwrap_or(h.subtitle)]).collect());
                }
            }
        }
        Cmd::Project(p) => project(ctx, p)?,
        Cmd::Runtime(RuntimeCmd::List) => {
            let r = ctx.call(CoreCommand::ListRuntimeCatalog)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::RuntimeCatalog { entries } = r {
                    let mut rows = vec![vec!["ID".into(), "NAME".into(), "VERSION".into(), "STATE".into()]];
                    rows.extend(entries.into_iter().map(|e| vec![e.id, e.name, e.version, if e.installed { "installed".into() } else { "available".into() }]));
                    table(rows);
                }
            }
        }
        Cmd::Runtime(RuntimeCmd::Install { id, version }) => install(ctx, &id, version.as_deref())?,
        Cmd::Php(PhpCmd::Use { version }) => {
            ctx.call(CoreCommand::SetSetting { key: "runtime.php.global".into(), value: serde_json::json!(version) })?;
            println!("PHP {version} is now the default for projects that don't ask for a version.");
        }
        Cmd::Service(s) => service(ctx, s)?,
        Cmd::Domain(_) => {
            let r = ctx.call(CoreCommand::ListDomains)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Domains { domains } = r {
                    let mut rows = vec![vec!["SITE".into(), "TYPE".into(), "STATE".into(), "FOLDER".into()]];
                    rows.extend(domains.into_iter().map(|d| vec![d.url, d.group, if d.enabled { "enabled".into() } else { "disabled".into() }, d.folder]));
                    table(rows);
                }
            }
        }
        Cmd::Certificate(_) => {
            let r = ctx.call(CoreCommand::ListCertificates)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Certificates { certs } = r {
                    let mut rows = vec![vec!["HOST".into(), "DAYS LEFT".into(), "NAMES".into()]];
                    rows.extend(certs.into_iter().map(|c| vec![c.hostname, c.days_left.to_string(), c.sans.join(", ")]));
                    table(rows);
                }
            }
        }
        Cmd::Tunnel(t) => tunnel(ctx, t)?,
        Cmd::QuickApp(_) => {
            let r = ctx.call(CoreCommand::ListQuickApps)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::QuickApps { apps } = r {
                    table(apps.into_iter().map(|a| vec![a.id, a.name, a.description]).collect());
                }
            }
        }
        Cmd::QuickCommand(QuickCommandCmd::List) => {
            let r = ctx.call(CoreCommand::ListQuickCommands)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::QuickCommands { commands } = r {
                    table(commands.into_iter().map(|c| vec![c.id, c.name, c.description]).collect());
                }
            }
        }
        Cmd::QuickCommand(QuickCommandCmd::Run { id, project }) => {
            let pid = match project {
                Some(p) => Some(ctx.project_id(&p)?),
                None => ctx.project_for_path(Path::new(".")).ok().map(|(id, _)| id),
            };
            match ctx.call(CoreCommand::RunQuickCommand { id: id.clone(), project_id: pid })? {
                CoreResponse::MaybeProcess { id: Some(process) } => follow(ctx, process)?,
                _ => println!("Done."),
            }
        }
        Cmd::Worker(w) => {
            match w {
                WorkerCmd::List => {
                    let r = ctx.call(CoreCommand::ListWorkers { project_id: None })?;
                    if !ctx.print_json(&r) {
                        if let CoreResponse::Workers { workers } = r {
                            table(workers.into_iter().map(|w| vec![w.worker.name, format!("{}/{}", w.running, w.worker.count), w.command_line]).collect());
                        }
                    }
                }
                WorkerCmd::Start { project } => {
                    let id = ctx.project_id(&project)?;
                    if let CoreResponse::Count { count } = ctx.call(CoreCommand::StartProjectWorkers { project_id: id })? {
                        println!("{count} worker process(es) running.");
                    }
                }
                WorkerCmd::Stop { project } => {
                    let id = ctx.project_id(&project)?;
                    ctx.call(CoreCommand::StopProjectWorkers { project_id: id })?;
                    println!("Workers stopped.");
                }
            }
        }
        Cmd::Snapshot(SnapshotCmd::List { project }) => {
            let id = ctx.project_id(&project)?;
            let r = ctx.call(CoreCommand::ListSnapshots { project_id: id })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Snapshots { snapshots } = r {
                    table(snapshots.into_iter().map(|s| vec![s.id, s.label, s.summary.join(" · ")]).collect());
                }
            }
        }
        Cmd::Snapshot(SnapshotCmd::Create { project, label, env, databases, files }) => {
            let id = ctx.project_id(&project)?;
            let r = ctx.call(CoreCommand::CreateSnapshot { project_id: id, label, options: ols_core::snapshots::SnapshotOptions { env, databases, files } })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Snapshot { snapshot } = r {
                    println!("Snapshot saved: {}", snapshot.path);
                }
            }
        }
        Cmd::Plugin(p) => plugin(ctx, p)?,
        Cmd::Catalog(c) => catalog(ctx, c)?,
        Cmd::Api(a) => api(ctx, a)?,
        Cmd::Update(u) => update(ctx, u)?,
        Cmd::ShellMenu(m) => {
            let r = ctx.call(match m {
                ShellMenuCmd::Status => CoreCommand::GetShellMenu,
                ShellMenuCmd::Install => CoreCommand::InstallShellMenu,
                ShellMenuCmd::Remove => CoreCommand::RemoveShellMenu,
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::ShellMenu { status } = r {
                    println!("The Explorer menu is {}.", if status.installed { "installed" } else { "not installed" });
                }
            }
        }
        Cmd::SupportBundle { dest } => {
            let r = ctx.call(CoreCommand::ExportSupportBundle { dest: dest.display().to_string() })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Lines { lines } = r {
                    println!("Wrote {} with: {}", dest.display(), lines.join(", "));
                    println!("Secrets are redacted, but read it before you share it.");
                }
            }
        }
        Cmd::Network => {
            let r = ctx.call(CoreCommand::CheckNetwork { force: true })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Network { status } = r {
                    println!("{}", if status.online { "Online." } else { "Offline: downloads, tunnels and updates won't work." });
                    for p in status.probes {
                        println!("  {} {}", if p.ok { "✓" } else { "✗" }, p.name);
                    }
                }
            }
        }
        Cmd::Test(TestCmd::Load { project, script, site, profile, vars, public }) => load_test(ctx, project, script, site, profile, vars, public)?,
        // @@cli-arms
    }
    Ok(())
}

fn stop_everything(ctx: &Ctx) -> R<()> {
    if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
        for t in tunnels.into_iter().filter(|t| t.state != "stopped") {
            ctx.call(CoreCommand::StopTunnel { id: t.config.id })?;
        }
    }
    if let CoreResponse::Projects { projects } = ctx.call(CoreCommand::ListProjects)? {
        for p in projects {
            ctx.call(CoreCommand::StopProjectWorkers { project_id: p.id })?;
        }
    }
    ctx.call(CoreCommand::StopWeb)?;
    if let CoreResponse::Services { services } = ctx.call(CoreCommand::ListServices)? {
        for s in services.into_iter().filter(|s| s.running) {
            ctx.call(CoreCommand::StopService { id: s.id.clone() })?;
            println!("Stopped {}.", s.name);
        }
    }
    println!("Everything is stopped.");
    Ok(())
}

fn setup(ctx: &Ctx, path: PathBuf, dry_run: bool, yes: bool) -> R<()> {
    let (id, _) = ctx.project_for_path(&path)?;
    let r = ctx.call(CoreCommand::PlanSetup { project_id: id.clone() })?;
    let CoreResponse::SetupPlan { plan } = r else { return Err("unexpected reply".into()) };
    if ctx.json && dry_run {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap_or_default());
        return Ok(());
    }
    print!("{}", ols_core::setup::plan_text(&plan));
    if !plan.ok {
        return Err("the plan has blocking conflicts (✗ above); nothing was changed".into());
    }
    let todo = plan.steps.iter().filter(|s| !s.done).count();
    if dry_run {
        println!("\nDry run: nothing was changed. {todo} step(s) would run.");
        return Ok(());
    }
    if todo == 0 {
        println!("\nEverything is already in place.");
        return Ok(());
    }
    if !yes && !confirm(&format!("\nApply these {todo} step(s)?")) {
        println!("Nothing changed. Run with --yes to apply.");
        return Ok(());
    }

    // Follow the steps while the (blocking) setup runs.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let done = done.clone();
        let paths = ctx.paths.clone();
        std::thread::spawn(move || {
            let mut shown = std::collections::HashSet::new();
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(Ok(CoreResponse::SetupProgress { report: Some(r) })) = control::send(&paths, CoreCommand::GetSetupProgress) {
                    for s in r.steps.iter().filter(|s| matches!(s.status, ols_core::setup::StepStatus::Running)) {
                        if shown.insert(s.label.clone()) {
                            println!("  … {}", s.label);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(600));
            }
        })
    };
    let result = ctx.call(CoreCommand::ApplySetup { project_id: id, dry_run: false });
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = watcher.join();
    let r = result?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::Setup { report } = r else { return Ok(()) };
    println!();
    for s in &report.steps {
        use ols_core::setup::StepStatus as S;
        let mark = match s.status {
            S::Done | S::Skipped => "✓",
            S::Failed => "✗",
            S::RolledBack => "↺",
            _ => "·",
        };
        println!("{mark} {}{}", s.label, s.detail.as_ref().map(|d| format!(" ({d})")).unwrap_or_default());
    }
    for r in &report.rolled_back {
        println!("undone: {r}");
    }
    match report.error {
        Some(e) => Err(e),
        None => {
            println!("\nThe environment is set up.{}", report.lock_written.map(|l| format!(" Wrote {l}.")).unwrap_or_default());
            Ok(())
        }
    }
}

fn project(ctx: &Ctx, cmd: ProjectCmd) -> R<()> {
    match cmd {
        ProjectCmd::List => {
            let r = ctx.call(CoreCommand::ListProjects)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Projects { projects } = r {
                    table(projects.into_iter().map(|p| vec![p.name, p.path]).collect());
                }
            }
        }
        ProjectCmd::Add { path } => {
            let (_, name) = ctx.project_for_path(&path)?;
            println!("{name} is a project.");
        }
        ProjectCmd::Remove { name } => {
            let id = ctx.project_id(&name)?;
            ctx.call(CoreCommand::RemoveProject { id })?;
            println!("Removed {name} (its files were not touched).");
        }
        ProjectCmd::Start { name } => {
            let id = ctx.project_id(&name)?;
            let CoreResponse::Projects { projects } = ctx.call(CoreCommand::ListProjects)? else { return Ok(()) };
            let path = projects.into_iter().find(|p| p.id == id).map(|p| PathBuf::from(p.path)).unwrap_or_default();
            setup(ctx, path, false, true)?;
            if let CoreResponse::Count { count } = ctx.call(CoreCommand::StartProjectWorkers { project_id: id })? {
                if count > 0 {
                    println!("{count} worker process(es) running.");
                }
            }
        }
        ProjectCmd::Stop { name } => {
            let id = ctx.project_id(&name)?;
            ctx.call(CoreCommand::StopProjectWorkers { project_id: id })?;
            println!("{name}'s workers are stopped.");
        }
        ProjectCmd::Clone { url, path } => {
            let target = if path.is_absolute() { path } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
            println!("Cloning {url}…");
            if let CoreResponse::Project { project } = ctx.call(CoreCommand::GitClone { url, target: target.display().to_string(), branch: None })? {
                println!("{} is ready at {}. Next: cd there and run `ols setup`.", project.name, project.path);
            }
        }
    }
    Ok(())
}

fn install(ctx: &Ctx, id: &str, version: Option<&str>) -> R<()> {
    let CoreResponse::RuntimeCatalog { entries } = ctx.call(CoreCommand::ListRuntimeCatalog)? else { return Err("unexpected reply".into()) };
    let candidates: Vec<_> = entries.iter().filter(|e| e.id == id && version.is_none_or(|v| e.version == v || e.version.starts_with(&format!("{v}.")))).collect();
    let entry = candidates.last().ok_or_else(|| {
        let have: Vec<String> = entries.iter().filter(|e| e.id == id).map(|e| e.version.clone()).collect();
        if have.is_empty() { format!("\"{id}\" is not in the catalog (see `ols runtime list`)") } else { format!("{id} {} is not available; available: {}", version.unwrap_or(""), have.join(", ")) }
    })?;
    if entry.installed {
        println!("{} {} is already installed.", entry.name, entry.version);
        return Ok(());
    }
    let (name, v) = (entry.name.clone(), entry.version.clone());
    ctx.call(CoreCommand::InstallRuntime { id: id.into(), version: v.clone() })?;
    print!("Installing {name} {v}");
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        print!(".");
        let _ = std::io::stdout().flush();
        if let CoreResponse::RuntimeCatalog { entries } = ctx.call(CoreCommand::ListRuntimeCatalog)? {
            if entries.iter().any(|e| e.id == id && e.version == v && e.installed) {
                println!(" done.");
                return Ok(());
            }
        }
        if started.elapsed() > Duration::from_secs(45 * 60) {
            println!();
            return Err("the install did not finish in 45 minutes; see the log".into());
        }
    }
}

fn service(ctx: &Ctx, cmd: ServiceCmd) -> R<()> {
    match cmd {
        ServiceCmd::List => {
            let r = ctx.call(CoreCommand::ListServices)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Services { services } = r {
                    let mut rows = vec![vec!["ID".into(), "NAME".into(), "STATE".into(), "PORT".into()]];
                    rows.extend(services.into_iter().map(|s| {
                        let state = if !s.installed { "not installed" } else if s.running { "running" } else { "stopped" };
                        vec![s.id, s.name, state.into(), s.port.map(|p| p.to_string()).unwrap_or_default()]
                    }));
                    table(rows);
                }
            }
        }
        ServiceCmd::Start { id } => {
            ctx.call(CoreCommand::StartService { id: id.clone() })?;
            println!("Starting {id}.");
        }
        ServiceCmd::Stop { id } => {
            ctx.call(CoreCommand::StopService { id: id.clone() })?;
            println!("Stopped {id}.");
        }
        ServiceCmd::Restart { id } => {
            ctx.call(CoreCommand::RestartService { id: id.clone() })?;
            println!("Restarted {id}.");
        }
        ServiceCmd::Logs { id, lines } => {
            let CoreResponse::LogSources { sources } = ctx.call(CoreCommand::ListLogSources)? else { return Ok(()) };
            let CoreResponse::Services { services } = ctx.call(CoreCommand::ListServices)? else { return Ok(()) };
            let name = services.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or(id.clone());
            let source = sources
                .iter()
                .rfind(|s| s.kind == "process" && s.name.eq_ignore_ascii_case(&name))
                .or_else(|| sources.iter().find(|s| s.id == id || (id.starts_with("web") && s.id == "web:error")))
                .ok_or_else(|| format!("{name} has no log yet (it hasn't run since the app started)"))?;
            if let CoreResponse::LogLines { lines, .. } = ctx.call(CoreCommand::ReadLog { source: source.id.clone(), max_lines: lines })? {
                for l in lines {
                    println!("{l}");
                }
            }
        }
    }
    Ok(())
}

fn tunnel(ctx: &Ctx, cmd: TunnelCmd) -> R<()> {
    let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? else { return Err("unexpected reply".into()) };
    let find = |name: &str| -> R<ols_core::tunnel::TunnelStatus> {
        let pid = ctx.project_id(name).ok();
        tunnels
            .iter()
            .find(|t| t.config.id == name || t.config.name.eq_ignore_ascii_case(name) || (pid.is_some() && t.config.project_id == pid))
            .cloned()
            .ok_or_else(|| format!("no tunnel named \"{name}\"; create one on the Tunnels page"))
    };
    match cmd {
        TunnelCmd::List => {
            let mut rows = vec![vec!["NAME".into(), "PROVIDER".into(), "STATE".into(), "TARGET".into(), "PUBLIC URL".into()]];
            rows.extend(tunnels.iter().map(|t| vec![t.config.name.clone(), t.config.provider.clone(), t.state.clone(), t.config.target.clone(), t.public_url.clone().unwrap_or_default()]));
            table(rows);
        }
        TunnelCmd::Start { name, yes } => {
            let t = find(&name)?;
            let mut r = ctx.call(CoreCommand::StartTunnel { id: t.config.id.clone(), confirm_exposure: false })?;
            if let CoreResponse::Tunnel { tunnel } = &r {
                if tunnel.state == "needs_confirmation" {
                    println!("{}", tunnel.exposure);
                    if !yes && !confirm("Make it public?") {
                        println!("Not started.");
                        return Ok(());
                    }
                    r = ctx.call(CoreCommand::StartTunnel { id: t.config.id.clone(), confirm_exposure: true })?;
                }
            }
            // Wait briefly for the provider's address.
            for _ in 0..40 {
                if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
                    if let Some(s) = tunnels.into_iter().find(|x| x.config.id == t.config.id) {
                        if let Some(url) = s.public_url {
                            println!("PUBLIC: {url} → {}\nStop it with: ols tunnel stop {}", s.config.target, s.config.name);
                            return Ok(());
                        }
                        if let Some(e) = s.error {
                            return Err(e);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            let _ = r;
            println!("Started; the provider hasn't given an address yet. Check with `ols tunnel list`.");
        }
        TunnelCmd::Stop { name } => {
            let t = find(&name)?;
            ctx.call(CoreCommand::StopTunnel { id: t.config.id })?;
            println!("{} is no longer public.", t.config.name);
        }
    }
    Ok(())
}

/// Prints a process's output until it exits.
fn follow(ctx: &Ctx, id: ols_core::process::ProcessId) -> R<()> {
    let mut printed = 0usize;
    while let CoreResponse::ProcessOutput { lines, .. } = ctx.call(CoreCommand::GetProcessOutput { id })? {
        if lines.len() < printed {
            printed = 0;
        }
        for l in &lines[printed..] {
            println!("{l}");
        }
        printed = lines.len();
        let CoreResponse::Processes { processes } = ctx.call(CoreCommand::ListProcesses)? else { break };
        match processes.into_iter().find(|p| p.id == id) {
            Some(p) if matches!(p.state, ols_core::process::ProcessState::Running | ols_core::process::ProcessState::Starting) => std::thread::sleep(Duration::from_millis(400)),
            Some(p) => {
                return match p.exit_code {
                    Some(0) | None => Ok(()),
                    Some(c) => Err(format!("exited with code {c}")),
                };
            }
            None => break,
        }
    }
    Ok(())
}

fn plugin(ctx: &Ctx, cmd: PluginCmd) -> R<()> {
    let list = |ctx: &Ctx| -> R<Vec<ols_core::plugin::PluginInfo>> {
        match ctx.call(CoreCommand::ListPlugins)? {
            CoreResponse::Plugins { plugins } => Ok(plugins),
            _ => Err("unexpected reply".into()),
        }
    };
    match cmd {
        PluginCmd::List => {
            let plugins = list(ctx)?;
            let mut rows = vec![vec!["ID".into(), "NAME".into(), "VERSION".into(), "STATE".into(), "ADDS".into()]];
            rows.extend(plugins.iter().map(|p| {
                let state = if p.problem.is_some() { "unusable" } else if p.enabled { "on" } else { "off" };
                let mut adds = Vec::new();
                for (n, what) in [(p.runtimes, "runtimes"), (p.quick_apps, "quick apps"), (p.detections, "detections"), (p.health_checks, "health checks")] {
                    if n > 0 {
                        adds.push(format!("{n} {what}"));
                    }
                }
                vec![p.manifest.id.clone(), p.manifest.name.clone(), p.manifest.version.clone(), state.into(), adds.join(", ")]
            }));
            table(rows);
        }
        PluginCmd::Install { source } => {
            let abs = std::fs::canonicalize(&source).unwrap_or(source);
            let r = ctx.call(CoreCommand::InstallPlugin { source: abs.display().to_string().trim_start_matches(r"\\?\").to_string() })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Plugin { plugin } = r {
                    println!("Installed {} {}. It is off; turn it on with: ols plugin enable {}", plugin.manifest.name, plugin.manifest.version, plugin.manifest.id);
                }
            }
        }
        PluginCmd::Enable { id, yes } => {
            let plugin = list(ctx)?.into_iter().find(|p| p.manifest.id == id).ok_or_else(|| format!("no plugin named {id}"))?;
            if let Some(problem) = &plugin.problem {
                return Err(problem.clone());
            }
            println!("{} asks for:", plugin.manifest.name);
            for p in &plugin.permissions {
                println!("  - {}", p.description);
            }
            if !plugin.permissions.is_empty() && !yes && !confirm("Allow these?") {
                println!("Not turned on.");
                return Ok(());
            }
            let approve = plugin.manifest.permissions.clone();
            ctx.call(CoreCommand::SetPluginEnabled { id: id.clone(), enabled: true, approve })?;
            println!("{id} is on.");
        }
        PluginCmd::Disable { id } => {
            ctx.call(CoreCommand::SetPluginEnabled { id: id.clone(), enabled: false, approve: vec![] })?;
            println!("{id} is off.");
        }
        PluginCmd::Remove { id } => {
            ctx.call(CoreCommand::RemovePlugin { id: id.clone() })?;
            println!("Removed {id}.");
        }
    }
    Ok(())
}

fn catalog(ctx: &Ctx, cmd: CatalogCmd) -> R<()> {
    let show = |ctx: &Ctx, r: CoreResponse| {
        if ctx.print_json(&r) {
            return;
        }
        if let CoreResponse::CatalogSources { catalogs } = r {
            let mut rows = vec![vec!["ID".into(), "NAME".into(), "SIGNATURE".into(), "RUNTIMES".into(), "PLUGINS".into(), "NOTE".into()]];
            rows.extend(catalogs.iter().map(|c| {
                vec![
                    c.source.id.clone(),
                    c.source.name.clone(),
                    if c.verified { "verified".into() } else { "not verified".into() },
                    c.doc.as_ref().map(|d| d.runtimes.len().to_string()).unwrap_or_default(),
                    c.doc.as_ref().map(|d| d.plugins.iter().map(|p| p.id.clone()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                    c.error.clone().or_else(|| c.note.clone()).unwrap_or_default(),
                ]
            }));
            table(rows);
        }
    };
    match cmd {
        CatalogCmd::List => show(ctx, ctx.call(CoreCommand::ListCatalogSources)?),
        CatalogCmd::Add { name, url, public_key } => {
            let r = ctx.call(CoreCommand::AddCatalogSource { name, url, public_key })?;
            show(ctx, r);
            println!("Added. Run `ols catalog refresh` to download it.");
        }
        CatalogCmd::Remove { id } => show(ctx, ctx.call(CoreCommand::RemoveCatalogSource { id })?),
        CatalogCmd::Refresh { id } => show(ctx, ctx.call(CoreCommand::RefreshCatalogs { id })?),
        CatalogCmd::Install { catalog, plugin } => {
            ctx.call(CoreCommand::InstallCatalogPlugin { source_id: catalog, plugin_id: plugin.clone() })?;
            println!("Installed {plugin}. It is off; turn it on with: ols plugin enable {plugin}");
        }
    }
    Ok(())
}
fn api(ctx: &Ctx, cmd: ApiCmd) -> R<()> {
    let show = |ctx: &Ctx, r: CoreResponse| {
        if ctx.print_json(&r) {
            return;
        }
        if let CoreResponse::ApiStatus { status } = r {
            println!("API: {} ({} mode) at {}", if status.running { "running" } else { "off" }, status.settings.mode, status.url);
            println!("Token: {}", if status.token_set { "set" } else { "not set (ols api token)" });
            if let Some(e) = status.error {
                println!("Problem: {e}");
            }
        }
    };
    match cmd {
        ApiCmd::Status => show(ctx, ctx.call(CoreCommand::GetApiStatus)?),
        ApiCmd::Enable { port, operate } => {
            let CoreResponse::ApiStatus { status } = ctx.call(CoreCommand::GetApiStatus)? else { return Err("unexpected reply".into()) };
            if !status.token_set {
                if let CoreResponse::Text { text } = ctx.call(CoreCommand::RotateApiToken)? {
                    println!("New API token (shown once): {text}");
                }
            }
            show(ctx, ctx.call(CoreCommand::SetApiSettings { enabled: true, port, mode: if operate { "operate" } else { "read_only" }.into() })?);
        }
        ApiCmd::Disable => {
            let CoreResponse::ApiStatus { status } = ctx.call(CoreCommand::GetApiStatus)? else { return Err("unexpected reply".into()) };
            show(ctx, ctx.call(CoreCommand::SetApiSettings { enabled: false, port: status.settings.port, mode: status.settings.mode })?);
        }
        ApiCmd::Token => {
            if let CoreResponse::Text { text } = ctx.call(CoreCommand::RotateApiToken)? {
                println!("New API token (shown once; the old one no longer works):
{text}");
            }
        }
    }
    Ok(())
}

fn update(ctx: &Ctx, cmd: UpdateCmd) -> R<()> {
    match cmd {
        UpdateCmd::Check => {
            let r = ctx.call(CoreCommand::CheckUpdate)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Update { update } = r {
                    if update.available {
                        println!("Version {} is available (you have {}). Signature verified.
{}", update.latest, update.current, update.notes);
                        println!("Download it with: ols update download");
                    } else {
                        println!("You are up to date ({}).", update.current);
                    }
                }
            }
        }
        UpdateCmd::Download => {
            let r = ctx.call(CoreCommand::DownloadUpdate)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Update { update } = r {
                    println!("Downloaded and verified: {}
Install it with: ols update install", update.downloaded.unwrap_or_default());
                }
            }
        }
        UpdateCmd::Install => {
            if !confirm("Start the installer? OpenLocalServer will be replaced.") {
                return Ok(());
            }
            ctx.call(CoreCommand::InstallUpdate)?;
            println!("The installer has started.");
        }
    }
    Ok(())
}
fn load_test(ctx: &Ctx, project: Option<String>, script: Option<String>, site: Option<String>, profile: String, vars: Vec<String>, public: bool) -> R<()> {
    let id = match project {
        Some(p) => ctx.project_id(&p)?,
        None => ctx.project_for_path(Path::new("."))?.0,
    };
    let CoreResponse::LoadOverview { overview } = ctx.call(CoreCommand::LoadOverview { project_id: id.clone() })? else { return Err("unexpected reply".into()) };
    if !overview.k6.installed {
        return Err("k6 isn't installed: run `ols runtime install k6`".into());
    }
    let mut env: Vec<(String, String)> = Vec::new();
    for v in &vars {
        let (k, val) = v.split_once('=').ok_or_else(|| format!("--var wants NAME=VALUE, not {v}"))?;
        env.push((k.to_string(), val.to_string()));
    }
    let script = match script {
        Some(s) => s,
        None => match overview.scripts.as_slice() {
            [] => {
                let CoreResponse::LoadProfiles { profiles } = ctx.call(CoreCommand::LoadListProfiles)? else { return Err("unexpected reply".into()) };
                let plan = profiles.into_iter().find(|p| p.id == profile).ok_or_else(|| format!("no test plan named {profile}"))?;
                for v in plan.variables.iter().filter(|v| !v.value.is_empty()) {
                    if !env.iter().any(|(k, _)| *k == v.name) {
                        env.push((v.name.clone(), v.value.clone()));
                    }
                }
                let CoreResponse::Text { text } = ctx.call(CoreCommand::LoadGenerate { project_id: id.clone(), profile: plan, name: None })? else { return Err("unexpected reply".into()) };
                println!("No script yet; wrote the {profile} test: .openlocalserver/k6/{text}");
                text
            }
            [only] => only.name.clone(),
            many => return Err(format!("choose a script: {}", many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "))),
        },
    };
    let CoreResponse::LoadRun { run } = ctx.call(CoreCommand::LoadRun { project_id: id, script: script.clone(), target: site, confirm_public: public, env })? else { return Err("unexpected reply".into()) };
    println!("Running {script} against {} ...", run.target);
    let run_id = run.id;
    let last = loop {
        std::thread::sleep(Duration::from_secs(2));
        let CoreResponse::LoadRun { run } = ctx.call(CoreCommand::LoadStatus { run_id: run_id.clone() })? else { return Err("unexpected reply".into()) };
        let m = &run.metrics;
        if !ctx.json {
            println!("  {} requests, {:.1}/s, p95 {:.0} ms, errors {:.1}%, {} users", m.requests, m.rps, m.p95_ms, m.error_rate * 100.0, m.vus);
        }
        if run.state != "running" {
            break run;
        }
    };
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&last).unwrap_or_default());
    } else {
        let m = &last.metrics;
        println!("
{}: {} requests, p50 {:.0} ms, p95 {:.0} ms, p99 {:.0} ms, errors {:.2}%", last.state.to_uppercase(), m.requests, m.p50_ms, m.p95_ms, m.p99_ms, m.error_rate * 100.0);
    }
    match last.state.as_str() {
        "passed" => Ok(()),
        _ => Err(last.message.unwrap_or_else(|| "the test didn't pass".into())),
    }
}
// @@cli-fns
