# Architecture Overview — OLS

## Project Type
olsc is a local development environment manager for Windows. Successor to XAMPP / Laragon with broader runtime coverage and extensibility. Each project gets isolated runtimes, own domain, trusted local HTTPS, without manual system edits. Status: pre-release, stage 15/20 built.

## Technologies Used
- **Backend:** Rust (edition 2021, stable), Tokio full runtime, tracing + tracing-appender, serde / serde_json / serde_yaml, reqwest (rustls), sha2/sha1, zip, minijinja, regex, rcgen + rustls, portable-pty, sysinfo, chrono, hyper, minisign-verify, notify, keyring, directories, clap 4.
- **Desktop shell:** Tauri 2.11.6, tauri-plugin-dialog, tauri-plugin-single-instance, tray-icon; desktop alerts are built by the app itself on the `windows` crate (WinRT toast), not by a notification plugin.
- **Frontend:** TypeScript, React 19, Vite 8, Tailwind CSS 4, shadcn/ui (Radix), CodeMirror 6, xterm.js 5 + fit addon, lucide-react, Tauri API 2.11.
- **Managed runtimes:** PHP NTS 8.1–8.5 + Xdebug + Composer, Node 22/24 + corepack (npm/pnpm/yarn), Python venv, MariaDB 11.4, PostgreSQL, MongoDB, Redis (redis-windows), Mailpit, Nginx 1.28, Apache 2.4, Caddy 2.11, SQLite, k6, portable Git, HeidiSQL / pgAdmin / NoSQLBooster / Tiny RDM (Redis one-click).
- **Storage:** single SQLite file (`app.db`, WAL) under portable `data/` directory; settings as key/value rows, collections as JSON blobs. `OLS_HOME` env overrides all paths.

## Architecture Pattern
One core, many front doors. All logic lives in `ols-core`; UI, CLI, and HTTP API are thin dispatchers over `CoreCommand`.

```
GUI (React) ──┐
CLI (ols) ────┼──> Core::dispatch(CoreCommand) -> Inner (Arc-shared state)
HTTP API ─────┘         |
                        v
              Managers: Runtime, Service, Web, Domain, Certs,
              Project, ProcessSupervisor, Workers, Scheduler,
              Tunnel, Mail, Diagnostics, QuickApp, Plugin
```

Key patterns:
- Command dispatcher (AD1): single `CoreCommand` enum (~150-200 variants), single `CoreResponse`, single error shape `Diagnostic{problem,cause,fix}`.
- Supervisor: owns all child processes, tree-kill via taskkill, 500-line output ring, crash restart policy, event broadcast.
- Privilege separation: main app never elevated. `ols-helper` exposes closed validated set (hosts-apply/remove, nrpt-add/remove) via UAC `runas` or resident LocalSystem service + named pipe.
- Declarative config: `.openlocalserver/*.yaml` manifests, plugin.yaml, quick-app recipes with strict schema + Minijinja rendering.
- Operation journal + rollback: `operations.json`, setup pipeline with dry-run, config versioning with drift detection.

## Module Boundaries
| Layer | Crates / Dirs | Responsibility |
|---|---|---|
| System core | ols-core: lib, app, command, control, api, paths, settings, secrets, process, port, exec, error, logging, redact, diagnostics, repair, search, support, network, journal, resources, elevate | State, dispatch, IPC, infra |
| Runtime | ols-core: catalog, catalogs, runtime, php, nodepm, composer, venv, custom_install, resolver, detection, plugin, updater | Install, resolve, extend runtimes |
| Services | ols-core: service, custom_service, dbtools, mail, monitor, workers, scheduler | Background servers, workers, cron |
| Web | ols-core: web/*, domain, ca, certs, hosts, dns, health, tunnel, inspector | Domains, TLS, servers, exposure |
| Data | ols-core: dbbackup, sqlite, migrate | Databases, files, migration |
| Project | ols-core: project, project_tools, manifest, profiles, setup, snapshots, envfile, command_catalog, git, shortcuts, editors, terminal, shell_menu, procfile, loadtest, xdebug, quickapp/* | Projects, envs, automation |
| CLI | olsc | `ols` on PATH (`olsc.exe` beside the app), daemon mode |
| Helper | ols-helper | Elevated hosts/NRPT/service pipe |
| Shell | src-tauri | Tauri window, tray, single IPC `run_command`, events |
| UI | ui/src | React pages, components, `core.ts` IPC wrapper |

## Infrastructure Components
- ProcessSupervisor (own Tokio runtime, broadcast events: process-event, terminal-event, runtime-event).
- RuntimeManager (download -> SHA256 verify -> extract, PATH probe cache, broadcast).
- ServiceManager (MariaDB/Postgres/Mongo/Redis/Mailpit lifecycle, TCP health probe).
- WebManager (render -> validate -> backup -> apply -> reload -> health; rollback on validator fail). One pipeline run per web server, each with its own process, ports and config files; a rollback is scoped to the server that failed.
- PhpPools (one php-cgi pool per version, auto extensions, opcache/xdebug zend).
- CertificateManager + LocalCa (rcgen, 397-day leaves, <30d renewal, per-domain dirs).
- DnsServer (UDP 127.0.0.1 wildcard A records) + NRPT rules + hosts-file managed block.
- TunnelManager (cloudflare/ngrok/localtunnel) + Inspector proxy (500 req cap, redacted).
- Scheduler (1-min tick, cron/every_*, no overlap), Workers (max 16 copies), TerminalManager (portable-pty, max 8).
- Control channel (named pipe + token in control.json), HTTP API (127.0.0.1:7420, bearer SHA256, read_only/operate).

## Service Relationships
- UI -> Tauri `run_command` -> Core::dispatch -> Inner managers -> ProcessSupervisor -> OS processes.
- CLI -> control pipe -> app Core, or auto-spawn `olsc daemon` when app closed.
- WebManager depends on: DomainStore, Certs, PhpPools, DnsServer, Hosts (via helper), RuntimeManager, ProcessSupervisor, WebConfig. ServiceManager depends on WebManager only to list and probe the web servers.
- ServiceManager depends on: RuntimeManager, ProcessSupervisor, Port checker, CustomServiceStore.
- Setup pipeline depends on: Detection, Manifest, Resolver, Domain, Workers, Scheduler, QuickApp commands; wrapped in Journal.
- QuickApp: schema -> plan (review) -> run (background executor, host handles runtimes/domains/elevation).

## High-Level Behavior
1. User registers project folder; detection reads markers (composer.json, package.json, manage.py) without writing.
2. Setup resolves runtimes (manifest > detected > global), plans 14-step env (runtimes, DB, domain, DNS, SSL, mail, workers, scheduler), applies with journal + rollback.
3. WebManager renders nginx/apache/caddy configs, validates (`-t`), archives old, reloads, health-checks DNS->TCP->TLS->HTTP chain.
4. Domains default `<project>.test` with wildcard via local DNS; HTTPS via local CA trusted once to CurrentUser Root.
5. Services start on demand or autostart; workers/schedulers run while app/daemon open; tunnels require explicit public-confirm.
6. Diagnostics engine scans setup, returns fixable `CoreCommand`s; doctor proposes safe auto-repair; support bundle exports redacted zip.
