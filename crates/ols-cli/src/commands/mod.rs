pub mod ai;
pub mod extensions;
pub mod projects;
pub mod services;
pub mod system;

use crate::args::Cmd;
use crate::ctx::{Ctx, R};
use std::path::PathBuf;

pub fn run(ctx: &Ctx, cmd: Cmd) -> R<()> {
    match cmd {
        Cmd::Daemon { .. } => unreachable!(),
        Cmd::Start => system::start(ctx),
        Cmd::Stop => system::stop_everything(ctx),
        Cmd::Restart => system::restart(ctx),
        Cmd::Status => system::status(ctx),
        Cmd::Setup { dry_run, path, yes } => system::setup(
            ctx,
            path.unwrap_or_else(|| PathBuf::from(".")),
            dry_run,
            yes,
        ),
        Cmd::Doctor => system::doctor(ctx),
        Cmd::Repair {
            project,
            dry_run,
            yes,
        } => system::repair(ctx, project, dry_run, yes),
        Cmd::Search { query } => system::search(ctx, query),
        Cmd::Project(p) => projects::project(ctx, p),
        Cmd::Runtime(r) => services::runtime(ctx, r),
        Cmd::Php(p) => services::php(ctx, p),
        Cmd::Service(s) => services::service(ctx, s),
        Cmd::Domain(_) => projects::domain(ctx),
        Cmd::Certificate(_) => projects::certificate(ctx),
        Cmd::Tunnel(t) => services::tunnel(ctx, t),
        Cmd::QuickApp(_) => projects::quick_app(ctx),
        Cmd::QuickCommand(qc) => projects::quick_command(ctx, qc),
        Cmd::Worker(w) => projects::worker(ctx, w),
        Cmd::Snapshot(s) => projects::snapshot(ctx, s),
        Cmd::Plugin(p) => extensions::plugin(ctx, p),
        Cmd::Catalog(c) => extensions::catalog(ctx, c),
        Cmd::Api(a) => extensions::api(ctx, a),
        Cmd::Update(u) => extensions::update(ctx, u),
        Cmd::ShellMenu(m) => system::shell_menu(ctx, m),
        Cmd::SupportBundle { dest } => system::support_bundle(ctx, dest),
        Cmd::Network => system::network(ctx),
        Cmd::Test(t) => extensions::test(ctx, t),
        Cmd::Ai(c) => ai::ai_cmd(ctx, c),
        Cmd::Guide { version } => system::guide(ctx, version),
    }
}
