# OpenLocalServer — Full Documentation

## 1. Introduction
OpenLocalServer is a Windows-local development environment manager. It downloads and runs PHP, Node, databases, caches, mail, and web servers, gives each project isolated runtimes plus own `.test` domain with trusted local HTTPS, without manual system edits. Pre-release, stage 15/20. Portable `data/` beside exe; `OLS_HOME` overrides paths.

Scope: runtimes (PHP/Node/Python/Composer), databases (MariaDB/Postgres/Mongo/Redis/SQLite), web (Nginx/Apache/Caddy), projects (detect/setup/env/git/workers/scheduler/snapshots), domains + TLS + DNS, tunnels, diagnostics/doctor, plugins, quick-apps/commands, AI assist, local API, CLI, updater.

## 2. Architecture
One core, many front doors. `ols-core` holds all logic behind `Core::dispatch(CoreCommand)`. Tauri shell exposes single IPC `run_command`; CLI talks over named-pipe control channel (auto-spawns daemon); HTTP API posts same JSON to `127.0.0.1:7420/v1/command`.

See `architecture_overview.md` and diagrams:
- `diagrams/architecture.mmd` — system context
- `diagrams/dataflow.mmd` — setup-to-healthy flow
- `diagrams/classes.mmd` — core classes
- `diagrams/sequence.mmd` — domain + apply sequence

State: `Arc<Inner>` shared across threads; lock order domains-first. ProcessSupervisor owns Tokio runtime + all children. Managers communicate via broadcast events (`process-event`, `terminal-event`, `runtime-event`).

Privilege model: main app never elevated. `ols-helper` closed set (hosts-apply/remove, nrpt-add/remove, install-service) via UAC or LocalSystem pipe service with SDDL lock.

## 3. Modules
### 3.1 System core
Dispatch, state, IPC, infra. `lib.rs` re-exports; `app.rs` Inner + all command impls; `command.rs` ~150-variant dispatcher; `control.rs` named-pipe RPC with token; `api.rs` localhost HTTP with bearer + allowlists; `paths.rs` resolution; `settings.rs` atomic JSON; `secrets.rs` keyring; `process.rs` supervisor; `port.rs` conflict reporter; `exec.rs` capture helper; `error.rs` Diagnostic shape; `logging.rs` + `redact.rs`; `diagnostics.rs` findings engine; `repair.rs` doctor; `search.rs` global search; `support.rs` bundle; `network.rs` probe; `journal.rs` op log; `resources.rs` caps; `elevate.rs` helper runner.

### 3.2 Runtime
`catalog.rs` pinned HTTPS+SHA256 manifests; `catalogs.rs` minisign-signed remotes; `runtime.rs` download-verify-extract + probe; `php.rs` per-version php-cgi pools + extensions; `nodepm.rs` lockfile/corepack; `composer.rs` reader; `venv.rs` finder; `custom_install.rs` pinned paths win; `resolver.rs` manifest>detected>global; `detection.rs` marker scan; `plugin.rs` declarative plugins (wasm refused); `updater.rs` signed self-update.

### 3.3 Services
`service.rs` DB/cache/mail lifecycle + TCP probe; `custom_service.rs` user procs (custom- prefix, no shell); `dbtools.rs` external GUI detect; `mail.rs` per-framework Mailpit planner with diff; `monitor.rs` sysinfo rollup + site cost; `workers.rs` up to 16 copies; `scheduler.rs` 1-min cron tick, no overlap.

### 3.4 Web
`web/mod.rs` trait + SiteSpec; `web/manager.rs` render-validate-backup-apply-reload-health + rollback + drift; `web/nginx.rs`, `web/apache.rs` (foreground httpd, no service), `web/caddy.rs` (auto-HTTPS off, local CA); `domain.rs` validation; `ca.rs` deterministic root 2024-2044 via rcgen; `certs.rs` per-domain reuse unless SAN/<30d; `hosts.rs` idempotent block; `dns.rs` UDP wildcard; `health.rs` 5-step chain; `tunnel.rs` provider abstraction + confirm gate + port blocklist; `inspector.rs` forwarding proxy, 500 req, redacted.

### 3.5 Data
`dbbackup.rs` mysqldump/pg_dump with safety copy; `sqlite.rs` WAL-safe .backup + integrity_check; `migrate.rs` Laragon/XAMPP/Wamp live-dump importer.

### 3.6 Project
`project.rs` registry ID=sha256[..16]; `project_tools.rs` wrappers; `manifest.rs` strict YAML; `profiles.rs` templates; `setup.rs` 14-step pipeline + lock file; `snapshots.rs` zip + clone; `envfile.rs` lossless editor; `command_catalog.rs` Symfony/artisan/npm discovery; `git.rs` real binary + ASK_PASS; `shortcuts.rs`; `editors.rs`; `terminal.rs` pty max 8; `shell_menu.rs` HKCU; `procfile.rs`; `loadtest.rs` k6 VU cap 200; `xdebug.rs` trigger-default; `quickapp/*` schema->plan->run.

### 3.7 CLI / Helper / Shell / UI
CLI `ols` never manages directly; daemon mode runs core headless + scheduler clock. Helper validates ip=127.0.0.1/::1 and suffix leading-dot. Tauri shell builds tray from domains/services, single-instance, close-to-tray. UI `core.ts` sole IPC wrapper; no router, page state + `ols:navigate` events; `useWeb` shared poll 4s; pages per concern (see catalog.txt).

## 4. File Analysis
### Rust core essentials
- `lib.rs`: Purpose: crate root + re-exports. Key exports Core/CoreCommand/CoreResponse. Depends on command/paths/process/project/runtime/service/settings.
- `app.rs`: Purpose: shared Inner state + command impls. Holds supervisor/runtimes/services/certs/php/web/terminals/journal/workers/tunnels/api/ai. Lock order domains-first.
- `command.rs`: Purpose: single dispatcher. ~150 variants covering processes/runtimes/projects/services/web/DB/quickapps/AI/tunnels. Input CoreCommand JSON, output CoreResponse or Diagnostic.
- `control.rs`: Purpose: named-pipe RPC. control.json {pipe,token,pid}. Token-gated JSON lines. Handles shutdown handover.
- `api.rs`: Purpose: localhost HTTP. Port 7420, bearer SHA256, Origin reject, read_only vs operate lists, 1MB cap.
- `paths.rs`: Purpose: OLS_HOME > debug repo/data > exe data > OS app-data. migrate_legacy once.
- `process.rs`: Purpose: supervise all children. Tree-kill taskkill /T /F, 500-line ring, restart policy, broadcast.
- `runtime.rs`: Purpose: verified installs only. Zip root-strip, PATH probe 60s cache, temp version probe, broadcast Installed/Failed.
- `php.rs`: Purpose: FastCGI pools. One pool/version, auto curl/mbstring/pdo/openssl/opcache, zend for opcache/xdebug.
- `service.rs`: Purpose: DB/mail lifecycle. Reuses runtime + supervisor, TCP probe, no dep-ordered start yet.
- `web/manager.rs`: Purpose: safe apply pipeline. Archive replaced, rollback on validator fail, drift flag, manages pools/hosts/DNS.
- `domain.rs`: Purpose: domain data + sanitize. Proxy host/port checks, upstream_url, ownership gates.
- `ca.rs`/`certs.rs`: Purpose: local PKI. Deterministic CA rebuildable, 397-day leaves, reuse unless SAN/expiry, revoke=delete dir.
- `setup.rs`: Purpose: reproducible env. Order runtimes->pm->DB->domain->DNS->SSL->mail->workers->scheduler->tunnel. Journal-wrapped, rollback safe reverses, writes lock.
- `quickapp/schema.rs`/`plan.rs`/`run.rs`: Purpose: recipe safety. Strict types + regex + show_if; Minijinja strict; masked secrets in review; 4000-line log, 30-min step timeout.
- Remaining files follow same pattern: pure purpose, explicit errors, no silent fallback. Full per-file table in analysis agents; one-line index in `catalog.txt`.

### Tauri / CLI / Helper
- `src-tauri/src/lib.rs`: Tauri shell, tray, shutdown 30s wait, auto-fix loop 300s, quick-app finish poll 2s.
- `src-tauri/src/main.rs`: windows_subsystem hide console in release.
- `ols-cli/src/main.rs`: clap CLI, Ctx::call over pipe, daemon spawn detached on NotRunning, table/diag printers.
- `ols-helper/src/main.rs`: validated hosts/NRPT only; exit 5 on AccessDenied triggers UAC retry.
- `ols-helper/src/service.rs`: LocalSystem pipe, SDDL SY+BA full / IU RW, 64K line JSON.

### Frontend
- `core.ts`: only IPC caller. `App.tsx`: page state router. `main.tsx`: ThemeProvider mount.
- Pages: Dashboard (3s poll get_dashboard), Sites (merge domains+orphans, group/search), QuickApps (debounced plan 250ms, trust gate), Commands (prefix build, process-event stream), WebServer (5-key save-then-apply), Config (ownership + DiffView), Tunnels (exposure confirm + Inspector), Databases (per-engine tools + backup-before-restore), Services (3s poll), Runtimes (runtime-event + version compare), Profiles/Plugins/Logs (1.5s poll + severity regex)/Processes (2s stats)/Settings.
- Libs: hooks (usePoll/useAction), theme, web (shared state), ai/confirm/nav buses, wait (poll-until-real), utils cn.
- Components: SiteDialog (big tabbed editor), Terminal (xterm bridge), EnvEditor (lossless), CodeEditor (CodeMirror + diff), Diagnostics/Doctor, Release/SettingsExtras, CustomServices, PHP/Xdebug dialogs, Migrate, SystemMonitor, ui primitives (no IPC).

### Docs / build
README pre-release notice; scripts/dev.bat (npm+helper+vite+tauri dev); scripts/build-installer.bat 4-stage (tool+lock preflight, npm install if needed + npm run build, cargo build --release, ISCC v6/v7 optional, upload prompt); scripts/upload-release.bat (Open-Local-Server setup + portable zip + certutil SHA256SUMS via gh release); vite chunkSizeWarningLimit 1600 (main bundle ~1.5MB: xterm + CodeMirror); Inno Setup x64compatible lzma, lowest privileges, dir/program-group pages enabled; render-icon.mjs Resvg 1024.

## 5. Business Rules
- Resolution manifest > detected > global; global never overrides explicit.
- Downloads require HTTPS + SHA256; mismatch aborts; catalogs minisign-verified each load.
- Web ownership Managed/Advanced/Manual; drift preserved + flagged; invalid keeps old.
- Domains any name, default `<project>.test`, wildcard via local DNS+NRPT; conflict detect; reverse proxy to any host/port; static index.php still runs PHP.
- Local CA trusted once to CurrentUser Root; wildcard certs; HTTP->HTTPS toggle; health 5-step chain.
- DB version isolation; safety backup before restore; secrets in keyring only; redaction everywhere.
- Mailpit mandatory; .env diff preview; local mail never leaves machine.
- Tunnels default off, first start needs confirm_exposure, public badge, DB/mail/xdebug ports blocked.
- Imported quick-apps/plugins untrusted until approved; elevated steps separate confirm.
- Diagnostics {problem,cause,fix}; safe auto-fix once/session; destructive asks.
- Workers/schedulers run while app/daemon open; memory caps next restart; k6 VU cap 200.
- Snapshots zip skips node_modules/vendor/.venv; import renames domains/DBs/ports/certs.
- Updates signed manifest + installer SHA; only own-folder exes startable; no telemetry default.

## 6. Data Models
See `data_models.txt` for full entity list. Core: Project, Domain, Runtime, DatabaseInstance, WebConfig+History, Certificate+CA, Tunnel, QuickApp+Run, QuickCommand+History, Plugin+CatalogSource, Profile+Environment, Worker, ScheduledTask, ServiceStatus, ProcessInfo, Settings keys, Secrets (keyring), Journal, Snapshot zip. Manifests under `.openlocalserver/`. Plugin `plugin.yaml` declarative. Recipe YAML with 14 variable types and `{{placeholders}}` + filters.

## 7. Flows
- Register -> detect -> resolve -> plan -> apply (journal) -> domain -> cert -> render -> validate -> backup -> reload -> DNS/hosts/NRPT -> services/workers/scheduler -> health. See `diagrams/dataflow.mmd`.
- Tunnel: create (untrusted token via env) -> confirm_exposure -> start supervised proc -> inspector proxy -> public badge.
- QuickApp: schema validate -> answers -> plan (review + masked secrets) -> approve source -> run background (cancel flag) -> notify.
- Doctor: diagnose -> explain -> propose CoreCommands -> confirm destructive -> re-diagnose.
- CLI offline: call -> NotRunning -> spawn daemon detached -> 30s pipe wait -> send.

## 8. Diagram References
- `diagrams/architecture.mmd`
- `diagrams/dataflow.mmd`
- `diagrams/classes.mmd`
- `diagrams/sequence.mmd`
