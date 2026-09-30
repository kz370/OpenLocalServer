# OLS — Implementation Plan

This file has two parts:

1. **Progress** — where the build stands, kept up to date as work lands.
2. **The original plan** — the staged plan the project started from. Crate names match the code
   (`ols-core`, `ols-helper`, `ols-cli`); the CLI is `ols`, and project manifests live in `.openlocalserver/`.

---

## Progress (as of 2026-09-26)

The full list of missing features is in [STATUS.md](STATUS.md).

**Current stage: 15 — Power UX + repair (built, 2026-09-26).** Releases 0.1–0.3 are feature-complete. None is
released: the 0.1 gate (the clean-VM Laravel flow) and the 0.3 gate (`git clone && ols setup` on a clean machine)
have not been run, and the new screens have not all been checked in the running app.

| Stage | Status | Open items |
|---|---|---|
| 0 Repository foundations | Partial | README, LICENSE, CONTRIBUTING, SECURITY, CHANGELOG; CI workflow; cargo-deny |
| 1 Core skeleton | Mostly done | Settings, stores and data are JSON files, not SQLite with migrations; no operation journal (decision 5) |
| 2 Supervisor, runner, ports | Done | Process trees are killed with `taskkill /T`, not Job Objects |
| 3 Packages + runtimes | Done | — |
| 4 Projects, detection, terminal, env, editors | Built | Interactive terminal, Open With menu and file shortcuts written 2026-09-26, not yet compiled or run |
| 5 Services, databases, secrets, DB tools | Built | PostgreSQL, Redis, custom services, SQL backup and restore, Mailpit `.env` integration and mail checks written 2026-09-26, not yet compiled or run |
| 6 Domains, CA, HTTPS, Nginx, app servers | Done | — |
| 7 Quick Apps + Quick Commands | Done | — |
| 8 Dashboard, logs, tray, packaging | Built, not verified | Installer and E2E (tauri-driver) not exercised; clean-VM release gate not run |
| 9 Web config, reverse proxy, wildcards | Done | — |
| 10 More servers and databases | Done | — |
| **11 Runtime depth + diagnostics** | **Built** | Xdebug, Composer, corepack, Python venv, `DiagnosticEngine` v1 and the `.env` editor exist; screens not yet checked in the running app |
| 12 CLI, manifests, reproducible setup | Built | `ols` CLI over a per-user, per-install named pipe (auto-starts `ols daemon` when the app is closed); full manifest schema, lock file, setup plan / dry run / apply with rollback of safe changes, conflict detection. The 0.3 gate has not been run |
| 13 Profiles, modes, workers, scheduler, snapshots | Built | 8 built-in profiles plus your own (import/export), 4 modes; queue workers; cron scheduler (runs while the app or daemon is open); snapshots, settings backups, environment import/export and cloning |
| 14 Tunnels + traffic | Built | Cloudflare, ngrok, LocalTunnel; first-exposure confirmation, internal-port refusal, access control in our own proxy; traffic inspector with redaction, replay and a webhook tester. Real providers not yet exercised end to end (needs their programs installed) |
| 15 Power UX + repair | Built | Command palette and global search (Ctrl+Shift+P / Ctrl+K), doctor, project diagnostics and repair, Git repository manager (portable Git in the catalog), resource limits |
| 16 Plugins + catalogs | Built (2026-09-26) | Declarative plugins with permissions, 4 built-in runtime plugins (Go, Bun, Java, .NET), minisign-signed catalogs. Not done: WASM code plugins, DB/service/tool plugins, Ruby, a default catalog and key. See [PLUGINS.md](PLUGINS.md) |
| 17 Release hardening | Built (2026-09-26) | Signed updater, localhost HTTP API, Explorer menu, offline indicator, support bundle, machine diagnostics, security review ([SECURITY_REVIEW.md](SECURITY_REVIEW.md)). Not done: update signing key, code-signed installer, 1.0 E2E on a clean VM |
| 18 Load testing with k6 | Built (2026-09-26) | k6 in the catalog, scripts, generator, live results, saved runs, `ols test load`. Not done: Quick Command entry, CPU cap, run against a real site in the app. See [LOAD_TESTING.md](LOAD_TESTING.md) |
| 19 AI assistant | Built (2026-09-26) | OpenAI-compatible providers (LM Studio, Ollama, Hugging Face, OpenRouter, custom) with streaming, read-only tools, redaction, a prompt preview, an allowlisted approve-to-run plan, all six features, `ols ai`. Not done: run against a real model, per-answer cost for providers other than OpenRouter, rendering the answer as Markdown. See [AI_ASSISTANT.md](AI_ASSISTANT.md) |
| 20 (release 2.0, Linux) | Not started | Last |

### Done early or beyond the plan
- **Resident helper service** (Risks: "UAC prompt fatigue"): `ols-helper install-service` installs a Windows
  service after one UAC prompt; the app then uses a local named pipe with the same validated command set.
- **Local DNS for whole reserved TLDs**: `.test`, `.localhost`, `.internal` resolve through the built-in DNS and
  one NRPT rule, with no hosts-file writes.
- **XAMPP / Laragon / WampServer database import** (§126, planned for Stage 17): dumps live or from a throwaway
  copy of the data folder, no `.sql` export needed.
- **Laragon-style automatic domains** for every folder in a scanned projects folder.
- **System monitor**: CPU and RAM rings, disks in use, and CPU / RAM / disk per site.
- **Leftover-server cleanup** at startup after the app was killed.
- **Reverse proxy to any host** (Docker, another computer) and a Reverse Proxy Quick App.
- **Domain rename**, editor picker (VS Code default), log clearing, in-app confirmations, brand icons.

- **Site config inside the domain edit dialog**: a "Web server config" tab beside the site's settings, with the same
  editor, structured blocks, history and ownership controls as the Web config page (which stays as it is).

### Added to the plan (2026-09-26)
- **Git repository manager** (extends §125, Stage 15): built. Clone into a new site, status, branches,
  stage/commit, pull/push/fetch, diff, log, remotes, stash, `.gitignore` templates; HTTPS credentials in the
  Secrets Manager. Uses an existing Git or a portable MinGit from the Runtimes catalog.
- **Load testing with k6** (Stage 18, below): performance tests for a project's sites from inside the app.
- **AI assistant** (Stage 19, below): opt-in, bring-your-own model: LM Studio (local), Hugging Face, OpenRouter,
  or any OpenAI-compatible server. It explains problems and proposes fixes; the user approves every change.
- **Projects and Sites are one page** (built 2026-09-26, aaPanel style): the Sites page is one table, a row per site plus projects that have no domain yet. Each row's Settings button opens a large dialog with a side menu: site settings, web server config and Servers (start, stop or restart the web server, PHP, the site's app and services), then the project's overview and tools (environment, terminal, commands such as artisan, .env, Git, workers, snapshots, repair, mail, Composer, Node, Python, Xdebug). The web server and certificates are on a separate Web server page.

### Changed (2026-09-26)
- **No macOS.** OLS targets Windows, and Linux last (Stage 20). macOS work is dropped from the plan.
- **Linux moved to the end**: stages after 15 are now 16 plugins, 17 hardening (Windows 1.0), 18 k6, 19 AI,
  20 Linux (2.0).

### Deviations from the plan
- Product name *OLS*; crates `ols-core`, `ols-helper` and `ols-cli` (binary `ols-cli`; `ols` reaches it through
  `OLS.exe`, which hands over any argument that is not its own — see `cli_dispatch`); no separate
  `platform` or `catalog` crates yet. Project manifests live in `.openlocalserver/`.
- The CLI talks to the app over a named pipe with a per-session token in `control.json`; when the app is closed,
  the CLI starts `ols daemon`, which hands over to the app when the app starts.
- Persistent state is JSON files under the data directory rather than SQLite + migrations.
- The data directory is portable: `data/` beside the executable (or the repo in debug builds).

---

# The original plan

## Context

`I:\Development\OpenLocalServer` holds only `OLS_Master_SRS_v4.md` (174 sections): a free, open-source,
cross-platform local dev environment manager (XAMPP/Laragon successor) built on Tauri 2 + Rust +
TypeScript + SQLite. Nothing is implemented yet. This plan turns the SRS into ordered, shippable stages
that follow the SRS release train (§164 MVP 0.1 → §165 0.2 → §166 0.3 → §167 1.0).

Decisions taken:
- Frontend: **React + TypeScript** (Vite, TanStack Query, Zustand, shadcn/ui + Tailwind).
- Platform order: **Windows first**. Every OS-specific call sits behind a `platform` trait from day one;
  Linux implementations come last, in Stage 20 (macOS is not planned).
- Scope: detailed stages for 0.1, coarser stages for 0.2 / 0.3 / 1.0.
- **Excluded from this plan:** Docker integration, WSL integration, container orchestration, and the rest
  of §168 "Future Features" (remote envs, cloud deploy). Nothing in the plan depends on them. §168's AI
  diagnostics were excluded at first and are now Stage 19 (added 2026-09-26).
- **Added beyond SRS:** OLS integrates **HeidiSQL** (MySQL/MariaDB/SQLite GUI) and **pgAdmin 4**
  (PostgreSQL GUI) and can open them already connected to a project database.
  - It **first detects** an existing install on the system.
  - It downloads a tool only when the tool is missing **and** the user confirms.

Deliverable on approval: copy this plan into the repo as `docs/IMPLEMENTATION_PLAN.md`, then start Stage 0.

---

## Architecture decisions (fixed before Stage 1)

1. **One core, many front doors (§8.1, §170).** All operations are a serializable `CoreCommand` enum
   handled by one dispatcher in `ols-core`. Tauri `invoke`, the CLI, and the local HTTP API all route to
   it. The UI never spawns processes (§8.2).
2. **Single process owner.** The Tauri app hosts the core and the Process Supervisor. It opens a local control
   channel (Windows named pipe; Unix socket later) that requires a per-session token. The CLI talks to it, or
   starts a headless core (`ols daemon`) when the GUI isn't running.
3. **Privilege separation (§8.5, §138).** `ols-helper` accepts a small, closed JSON command set
   (hosts block, NRPT DNS rule, later service install). On Windows it is launched per operation via
   `ShellExecute runas` (UAC). The main app never runs elevated.
4. **Declarative definitions (§8.3).** Runtimes, services, tools, Quick Apps, and Quick Commands are YAML/JSON
   definitions with a Rust schema. Built-in definitions ship with the app, and plugins add more later.
5. **Operation journal (§78, §163).** Every multi-step operation writes steps to the `events` table. This
   gives rollback, crash-interrupted operation detection on next start, and progress UI.
6. **Event-driven, not polling (§162).** The supervisor waits on process handles for exit. CPU/mem samples
   run only while a view that needs them is open. Log tail uses file-change notifications.
7. **Test isolation (§161).** `OLS_HOME` overrides every path, and tests use a reserved port range.
8. **Schema grows per stage.** Each stage adds migrations for its §150 tables (e.g. Stage 9 adds
   `web_config_history`, Stage 13 adds `profiles/snapshots/backups`, Stage 14 adds `tunnels`).

### Cargo workspace layout
```
openlocalserver/
├── crates/
│   ├── ols-core/      # domain + managers (§147 modules, §149 services)
│   ├── ols-platform/  # traits + windows/ impl (macos/, linux/ later)
│   ├── ols-helper/    # privileged helper binary
│   ├── ols-cli/       # clap CLI (Stage 12)
│   └── ols-catalog/   # package/service/tool/quick-app schema + built-in catalog data
├── src-tauri/              # Tauri 2 shell: IPC bindings, tray, windows
├── ui/                     # React app (§148 feature folders)
└── docs/
```

### Key crates
tokio, tracing (JSON + redaction layer), sqlx (SQLite), serde + a maintained YAML crate, reqwest + sha2,
zip/tar/flate2, rcgen (CA), keyring (secrets), portable-pty, sysinfo, windows-rs (Job Objects, cert store,
registry), minijinja, clap, tauri-specta. UI: xterm.js, CodeMirror 6 (Stage 9).

---

## RELEASE 0.1 — MVP (SRS §164)

### Stage 0 — Repository foundations
- `git init`, Cargo workspace, pnpm UI workspace, Tauri 2 scaffold (React-TS).
- OSS files (§144): README, LICENSE (user picks), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, CHANGELOG, `docs/`.
- rustfmt, clippy, ESLint/Prettier, cargo-deny. GitHub Actions on windows-latest.
- **Exit:** empty Tauri window builds and launches; CI green.

### Stage 1 — Core skeleton
- `AppPaths` (§146, OS conventions, `OLS_HOME` override). SQLite + migrations for 0.1 tables.
- Settings service. Error model with a user-facing `Diagnostic { problem, cause, fix }` shape (§112, §115).
- Structured logging (§118) with a secret-redaction layer (§141) and per-component files.
- Event bus (core → UI) and the operation journal (decision 5), including detection of interrupted operations on startup.
- `CoreCommand` dispatcher + tauri-specta bindings.
- UI shell: sidebar (§148 features), routing, theme, empty Dashboard/Settings.
- **Exit:** UI round-trips `ping` and `settings.get/set`. Redaction unit test passes.

### Stage 2 — Process Supervisor, Command Runner, Port Manager
- `ProcessSupervisor` (§107): PID/PPID/exe/args/cwd/env/start/exit/CPU/mem, using the full state machine.
  Job Objects kill process trees. Output goes to rotating logs plus a live stream.
- Crash recovery policy (§108): retries, delay, and log retention.
- `CommandRunner` (§90–91): all fields and modes, cancellation, and history (§93).
- `PortManager` (§109): detect the owner of a port and report the conflict. Never kill unrelated processes (§75).
- UI: Processes page and a virtualized live log tail.
- **Exit:** unit tests for the state machine and the restart policy. Integration test kills a spawned process tree.

### Stage 3 — Package Manager + Runtime Manager
- Package manifest schema (§20): version, platform, arch, URL, sha256, signature (optional now),
  dependencies, layout, executables, and config templates. Built-in catalog JSON.
- Download → cache (§127) → HTTPS + SHA-256 verify (§21) → atomic extraction (§163). A failed check aborts.
- Windows sources: PHP NTS zips, Node zips (npm included), and python-build-standalone (pip/venv included).
- **Composer (moved up from 0.2):** install `composer.phar` and run it with the project's resolved PHP. Laravel
  and Symfony Quick Apps need it.
- `RuntimeManager`: install/remove/list, global default, and a php.ini template per version.
- UI: Runtimes page (installed/available, progress, set global).
- **Exit:** install PHP 8.4, Node 22, Python 3.13, and Composer into temp home. A tampered hash fails the install.

### Stage 4 — Projects, detection, resolution, terminal, env vars, editors
- `ProjectManager` (§40): create, register existing folder, and delete registration.
- Framework detection (§42): all listed marker files and frameworks.
- Requirement detection (§43, §154):
  - PHP: version constraint and `ext-*` from composer.json.
  - Node: `engines`/.nvmrc and package manager from the lockfile (npm/pnpm/yarn).
  - Python: `requires-python`, requirements.txt, existing `.venv`.
  - DB engine from `.env` / framework config.
- `.openlocalserver/environment.yaml` parser (§71). Resolution goes project → profile → global (§18). The profile
  layer is a stub until Stage 13. Global never overrides project (§11).
- Runtime-aware terminal (§19): portable-pty + xterm.js, with the resolved runtimes on PATH.
- **Environment variables editor (§103):**
  - Edit `.env`, `.env.local`, `.env.testing`, and `.env.development`: add, edit, and delete.
  - Import, export, and compare files.
  - Secrets are hidden by default. Validation runs before saving.
- `EditorManager` (§94–99): Notepad++ (registry + both Program Files dirs), VS Code, Cursor, PhpStorm,
  IntelliJ IDEA, Sublime, system default, and custom. Placeholders `{{file}}/{{line}}/{{column}}/{{project_path}}`.
- Open With menu (§97) and file shortcuts (§100): project, public/, config/, .env, logs, and server configs.
- UI: project list, detail, and the creation wizard (§41, all fields; later-stage fields stay disabled until their stage).
- **Exit:**
  - Every framework fixture is detected.
  - Two projects' terminals show different `php -v`.
  - The .env editor round-trips a file without reformatting it.

### Stage 5 — Services, databases, secrets, DB GUI tools
- `ServiceDefinition` abstraction: package, init, config template, args, ports, health, and data dir.
- Order of work:
  - **Mailpit** (§61–66): SMTP 1025 / UI 8025, configurable. Message count comes from the Mailpit API (§64),
    and the mail diagnostics checklist (§66) runs process, SMTP, UI, project settings, and a test send.
  - **MySQL:** zip install, `--initialize-insecure`, one datadir per instance.
  - **PostgreSQL:** EDB binaries, `initdb`.
  - **Redis:** see Risks.
- `DatabaseManager`: create DB and user/role, then backup/restore via `mysqldump`/`pg_dump` (§32, §34). Remove
  instance (with a backup offer, §130). Show connection info.
- Instance isolation (§38) and ordered startup that waits on health (§68).
- **Secrets Manager (§104):** keyring-backed store for DB passwords, API keys, tunnel tokens, and SSH keys.
  Every other subsystem reads secrets through it.
- **Custom services UI (§67):** define name, exe, args, port, and health check. Custom services use the same supervisor.
- Mailpit project integration (§63): writes `MAIL_*` into `.env` after showing a diff.
- **DB GUI tools (user request, §102). Detect first, download only with the user's consent:**
  1. **Detect.** Scan for an existing install:
     - Registry uninstall keys (HKLM + HKCU, 64- and 32-bit views) and `App Paths`.
     - Default dirs (`Program Files\HeidiSQL`, `Program Files\pgAdmin 4`) and PATH.
     - Scoop, Chocolatey, and winget install locations.
     - A user-chosen path in Settings.
  2. **Found:** register that install as the tool and use it. OLS never modifies it (§126).
  3. **Not found:** show "HeidiSQL not installed" with **[Download & Install]**, **[Locate manually]**, and
     **[Skip]**. Nothing downloads without that click.
  4. **Download path:** the catalog entry goes through the Package Manager (HTTPS + SHA-256 verify).
     - **HeidiSQL:** portable zip, no admin.
     - **pgAdmin 4:** official installer run silently for the current user.
  5. Detection re-runs on app start and from a "Rescan" button. A missing tool shows as unavailable instead of
     triggering a download.
  - **Connect without exposing passwords:**
    - HeidiSQL: OLS writes a session entry to HeidiSQL's settings.
    - pgAdmin: servers go in through a `servers.json` import plus a passfile.
  - Project/database actions: "Open in HeidiSQL" and "Open in pgAdmin", already connected to that database.
  - Tool definitions carry detection rules (registry keys, paths, exe name), so plugins can add more tools later.
- UI: Services page, database panel, and the project Mail card.
- **Exit:**
  - Every service starts on a test port and becomes healthy.
  - A DB is created, backed up, and restored.
  - A test email is captured.
  - HeidiSQL/pgAdmin detection tests pass: an installed tool is found with no download, and a missing tool only
    offers a download.
  - After the user confirms the download, the tool installs and opens connected to a test DB.

### Stage 6 — Domains, hosts, Local CA, HTTPS, Nginx, app servers
- `ols-helper` v1: delimited OLS block in the hosts file. Closed command set with validated input.
- `DomainManager` (§44–48): templates, subdomains, port mappings, and conflict detection.
- Local CA (§50): key protected by restrictive ACLs (§142). Trusted in the Windows CurrentUser Root store after user confirmation.
- `CertificateManager` (§51): generate/renew/revoke/regenerate, expiry and trust checks, and the full detail view
  (issuer, domains, dates, trust, project, cert/key paths). Keys are never shown, logged, or sent to third parties.
- Nginx manager: install, Managed server blocks, `nginx -t` before apply, reload, and the HTTP→HTTPS toggle (§52).
- PHP: one supervised `php-cgi` pool per PHP version. Each site's `fastcgi_pass` targets the pool of its
  resolved version (§11).
- **Node/Python sites (fixes MVP gap):** a supervised app dev-server process (e.g. `npm run dev`,
  `uvicorn`, `manage.py runserver`) behind a basic Nginx `proxy_pass`. Full reverse-proxy management comes in Stage 9.
- HTTPS health chain (§53): DNS → TCP → TLS → cert → trust → HTTP.
- **Exit:** `https://a.test` runs PHP 8.1, `https://b.test` runs 8.4, and `https://c.test` proxies a Vite
  app. All three are trusted in Edge/Chrome.

### Stage 7 — Quick Apps + Quick Commands
- YAML schema (§83–86, §152–153): requirements, all §84 variable types, conditions, and pre/post hooks.
  Templates use minijinja.
- Execution goes through CommandRunner and the journal. Each run gets its own log source (§117).
- Trust and security (§88, §92, §139):
  - Imported catalogs are untrusted until the user approves them.
  - Before a run, a review dialog lists the commands and the permissions needed (install, write files, DB, DNS, certs).
  - Elevated steps ask for separate confirmation.
- **Built-in catalog (all 12 from §80):** Laravel, Symfony, WordPress, React+Vite, Vue+Vite, Next.js,
  Express API, FastAPI, Django, Plain PHP, Static HTML, Custom App.
- UI: gallery (search, filter, favorite, duplicate, edit, delete, import, export, create), wizard (§81), and
  Quick Commands with the history actions (§93).
- **Exit:** the Laravel Quick App produces the §155 result list on a clean home.

### Stage 8 — Dashboard, logs, tray, packaging → **ship 0.1**
- Dashboard (§173), environment health (§116), and one-click project actions (§101).
- Logs page (§117): all sources, with live tail, search, filter, severity, copy, and export.
- Tray (§120), notifications (§119), and startup settings (§121) including start with Windows. Tunnels stay off.
- NSIS/MSI installer. E2E via tauri-driver on the §160 flow, without tunnels.
- **Exit:** a fresh Windows VM goes from install to Laravel over trusted HTTPS with mail captured and HeidiSQL connected.

---

## RELEASE 0.2 (SRS §165)

### Stage 9 — Web config management, reverse proxy, wildcards
- CodeMirror editor with highlighting, search/replace, diff, revert, backup/restore, and open folder (§25).
- Managed/Advanced/Manual ownership with hash drift detection and the §27 protection dialog (§26–27).
- Apply pipeline (§28), history compare/restore/export (§29), and site enable/disable/validate/reload/duplicate/
  delete (§24).
- Structured GUI for common blocks (§23): upstreams, locations, FastCGI, proxy, TLS, redirects, headers, and includes.
  Raw edit stays available.
- Reverse-proxy mappings UI (§30).
- Wildcard domains (§46–47): a hickory-dns resolver on 127.0.0.1 plus a Windows NRPT rule for `.test` (through the
  helper). Wildcard certificates.

### Stage 10 — More servers and databases
- Apache (`httpd -t`, VirtualHosts, modules) and Caddy (`caddy validate`) behind the WebServer trait.
- Multiple web-server versions where practical (§22).
- MariaDB (users), MongoDB (connection info, logs, health), and SQLite (create, detect, associate, path,
  backup/restore, integrity check, open in HeidiSQL, §36).
- External DB tool config (§102): the user can register any other tool per engine.

### Stage 11 — Runtime depth + diagnostics v1
- PHP extensions per version (§12), Xdebug (modes, port, client host, IDE config, per project, §13), and full
  Composer commands (§14).
- pnpm/yarn via corepack (§15). Python venv create/detect/activate/recreate/install (§17).
- `DiagnosticEngine` v1 (§112): Problem/Cause/Fix/[Fix]/[Ignore]/[Details].

---

## RELEASE 0.3 (SRS §166)

### Stage 12 — CLI, manifests, reproducible setup
- `ols-cli` (§136) over the control channel.
- Manifests (§71): environment, services, and commands. Lock file (§72), `ols setup` (§73, §159),
  `--dry-run` (§77), Environment Plan preview (§76), and rollback (§78).
- Environment Resolver pipeline (§74) and full conflict detection (§75), including file ownership.

### Stage 13 — Profiles, modes, workers, scheduler, snapshots
- Profiles (§69) replace the Stage 4 stub. Project modes (§70).
- Queue workers (§105), GUI scheduler (§106), snapshots (§131), backups (§130), import/export including
  profiles (§132), and environment cloning (§158).

### Stage 14 — Tunnels + traffic
- `TunnelProvider` trait (§56). Adapters: Cloudflare Tunnel, ngrok and LocalTunnel, plus a mock for
  tests. Provider auth goes through the Secrets Manager. Optional access controls where the provider supports them.
- Safety (§59, §140): explicit start, first-exposure warning, public badge, prominent stop, no DB or admin
  exposure, redacted tokens, and keys never uploaded (§142). Tunnel UI and health (§58, §60).
- Request/traffic inspector with redaction (§110) and the webhook tester (§111).

### Stage 15 — Power UX + repair
- Command palette (§122) and global search (§123).
- Automatic repair (§114), explained diagnostics (§115), and `ols doctor` (§113).
- **Git repository manager** (§125, expanded 2026-09-26): per-project repo view inside the app. Clone into a new
  site, status and changed files, branches (create/switch/delete), stage and commit, pull/push/fetch, diff, log,
  remotes, stash, and `.gitignore` help. Credentials go through the Secrets Manager. Uses the system Git, or a
  portable Git downloaded through the Package Manager when none is found.
- Resource controls (§129).

---

## RELEASE 1.0 — Windows (SRS §167)

### Stage 16 — Plugins + catalogs
- Plugin manifest (§135) with explicit permissions (§134).
  - Phase A: declarative plugins covering runtime, DB, service, tool, framework, detection, Quick App, and health check.
  - Phase B: sandboxed code plugins (WASM) for tunnel providers, diagnostics, and UI panels (§133).
- Future runtimes (§9) are delivered as runtime plugins: Go, Ruby, Java, Bun, .NET.
- Signed remote runtime/DB/service catalogs (minisign) and Git/company/community Quick App catalogs with trust (§87–88).

### Stage 17 — Release hardening → **ship 1.0 (Windows)**
- Signed updater (§145), localhost HTTP API with a token (§137), and XAMPP/Laragon import (§126).
- Explorer context menu (§124), offline indicators (§128), and telemetry off by default (§143).
- Advanced diagnostics (§167) and full docs (§144).
- Security review against the §138 checklist.

## RELEASE 1.1

### Stage 18 — Load testing with k6
Performance testing for a project's sites without leaving the app, using [k6](https://k6.io) (Grafana's open-source
load tester, a single binary).
- **k6 runtime** through the Package Manager (official release archive, checksum verified), or an existing install.
- **Test scripts per project** in `.openlocalserver/k6/*.js`, committed with the project. A wizard writes a first
  script from the project's site and routes (a smoke test, a load test with ramping VUs, a spike test).
- **Run against the local site** (trusted HTTPS through the local CA via `SSL_CERT_FILE` / k6's own TLS options)
  or, deliberately, against a running tunnel's public URL. Refuse hosts that aren't the project's own sites.
- **Live results**: k6's JSON output streamed into the UI (requests/s, p50/p95/p99 latency, error rate, VUs,
  checks), thresholds as pass/fail, and each run kept with its summary so runs can be compared.
- **Quick Command and CLI**: `ols test load [project] [script]`, exit code from the thresholds, so CI can use it.
- **Safety**: CPU/VU limits from Settings → Resources, a stop button, and a warning before pointing k6 at a
  public tunnel (that sends real traffic through the provider).
- **Exit**: a Laravel project gets a generated smoke test, runs it from the Environment tab and the CLI, and a failed
  threshold shows as a failed run.

### Stage 19 — AI assistant (bring your own model)
Opt-in and off by default. OLS ships no model and no account; the user points it at one.

**Providers.** One `AiProvider` interface over the OpenAI chat-completions API (streaming, and tool calls where
the model supports them), so every provider is the same code with a different base URL and key:
- **LM Studio** (local, the default suggestion): `http://localhost:1234/v1`. Detected when running; models read
  from `/v1/models`. Ollama (`http://localhost:11434/v1`) works the same way.
- **Hugging Face**: the Inference Providers router (`https://router.huggingface.co/v1`) with an HF token, or a
  self-hosted Text Generation Inference / Inference Endpoint URL.
- **OpenRouter**: `https://openrouter.ai/api/v1` with an API key; model list and prices from its API, and the
  cost of each answer shown.
- **Custom OpenAI-compatible**: any base URL, key and model name (llama.cpp server, vLLM, LocalAI, a company
  gateway, ...).
Keys live in the Secrets Manager. Settings has a provider list with "Test connection" and a model picker, and
per-feature choice of provider (for example a local model for logs, a larger one for config help).

**Privacy and safety.**
- Local-first: a provider that sends data off the computer is labelled as such everywhere it is used.
- Everything sent goes through the same redaction as logs and the traffic inspector (`.env` values, passwords,
  tokens, keys, cookies, signatures), and "Show what will be sent" previews the exact prompt.
- Nothing is sent automatically: every request starts from a user action.
- **The AI proposes, the core disposes.** Answers are text or a proposed plan of `CoreCommand`s, checked against an
  allowlist and shown like a setup plan; nothing runs until the user approves it, destructive steps separately.
  The model's tools are read-only core queries (list sites, read a log tail, read a config, read a manifest).
  It never gets a shell.

**Features, in order.**
1. **Explain and fix**: an "Explain" button on diagnostics, failed setup steps, error log lines and failed
   requests. The answer says what went wrong and why, and offers a fix as an approvable plan.
2. **Write manifests and configs**: draft `.openlocalserver/environment.yaml` from a repository (reviewed through
   the normal setup plan); explain a web-config block and propose edits as a diff.
3. **Ask the logs**: questions across web, PHP, database, worker and app logs, with the lines it used quoted.
4. **Traffic**: explain a captured webhook, and write a handler or a k6 script (Stage 18) from recorded requests.
5. **Commit messages** from the staged diff in the Git tab.
6. **Plain-language palette**: "a Laravel site with Redis called shop" becomes a Quick App or setup plan to confirm.

**CLI**: `ols ai ask "..."`, `ols ai explain <finding>`.
**Tests**: a mock OpenAI-compatible server; redaction and allowlist tests; no test calls a real provider.
**Exit**: with LM Studio running a local model, a failing web-server start is explained and fixed after one
approval; the same works with OpenRouter; with only local providers configured, nothing leaves the computer.

---

## RELEASE 2.0 — Linux (last)

### Stage 20 — Linux
macOS is out of scope, and Linux is deliberately last (decided 2026-09-26): it is the largest remaining
stage, because almost every Windows-specific piece needs a Linux counterpart and Linux has no single set of
official binaries for PHP, MariaDB and the rest. Everything before it keeps the `platform`-style seams
(helper, trust store, DNS, control channel, process groups) so this stage adds implementations, not redesigns.
- Helper: polkit (pkexec) for the few privileged actions; an optional systemd service like the Windows helper.
- Trust store: update-ca-certificates (and the Fedora/Arch equivalents) plus NSS for Firefox and Chromium.
- DNS: a systemd-resolved drop-in for `.test`; the hosts file where resolved isn't used.
- Control channel on a Unix socket; process groups instead of `taskkill /T`.
- Linux package catalog (official tarballs with checksums); a DB GUI alternative to HeidiSQL (which is Windows-only; pgAdmin is cross-platform).
- CI matrix on Windows and Linux. Packages: AppImage and .deb.
- Linux package sources per runtime: official tarballs with checksums where they exist (Node, Caddy, MariaDB,
  PostgreSQL, Redis, Mailpit), and a documented choice for PHP (static builds or the distro's packages).
- **Exit**: the 0.1 Laravel flow and `git clone && ols setup` pass on Ubuntu LTS and Fedora.

---

## SRS coverage map
| SRS § | Stage |
|---|---|
| 1–8 vision, goals, architecture | Architecture decisions, Stage 0–1 |
| 9–19 runtimes, PHP/Node/Python, resolution, terminal | 3, 4, 11 (future runtimes → 16) |
| 20–21, 127 catalog, integrity, cache | 3, 16 |
| 22–30 web servers, config, proxy | 6, 9, 10 |
| 31–39 databases | 5, 10 (+ HeidiSQL/pgAdmin in 5) |
| 40–43, 154 projects, detection | 4 |
| 44–53 domains, DNS, CA, HTTPS | 6, 9 |
| 54–60, 110–111 tunnels, traffic | 14 |
| 61–66 Mailpit | 5 |
| 67–68 custom services, deps | 5 |
| 69–70 profiles, modes | 13 |
| 71–78, 159 manifests, setup, plans, rollback | 12 |
| 79–93, 152–153 Quick Apps/Commands, runner | 2, 7 |
| 94–102 editors, shortcuts, actions, DB tools | 4, 5, 8, 10 |
| 103–104, 141–142 env vars, secrets | 4, 5 |
| 105–106 workers, scheduler | 13 |
| 107–109 supervisor, ports | 2 |
| 112–116 diagnostics, repair, health | 8, 11, 15, 17, AI → 19 |
| 117–121 logs, notifications, tray, startup | 1, 8 |
| 122–125 palette, search, context menu, git | 15, 17 |
| 126, 128–132 import, offline, resources, backup | 5, 13, 15, 17 |
| 133–135 plugins | 16 |
| 136–137 CLI, API | 12, 17 |
| 138–140, 143 security, telemetry | throughout, 7, 14, 17 |
| 144–151 OSS, updater, layout, arch, schema | 0, 1, 17, decision 8 |
| 155–158 examples, cloning | 7, 8, 13 |
| 160–163 testing, perf, reliability | decisions 5–7, every stage; k6 load testing → 18 |
| 164–167 releases | stage grouping |
| 168 future | **excluded** (Docker, WSL, orchestration, etc.), except AI diagnostics → 19 |

## Risks / open items
- **Redis on Windows**: there is no official build. Choose between the Memurai dev edition and a maintained community port in Stage 5.
- **pgAdmin silent per-user install**: verify the installer flags. If a per-user install isn't possible, use a one-time
  UAC install through the helper, still only after the user confirms.
- **Firefox trust**: NSS store needs separate handling.
- **AI answers**: models can be wrong. Nothing an answer proposes runs without approval, and proposals are limited
  to allowlisted commands; small local models may be too weak for config help (per-feature provider choice).
- **Tunnel providers**: the adapters parse each provider's output for the public address; a provider changing its
  output format breaks detection (the tunnel still runs; the address shows in its log).
- **UAC prompt fatigue**: per-operation helper in 0.1. Consider an optional installed helper service later.
- **Ports 80/443** may be taken by IIS or Skype. The PortManager reports the owner and offers alternate ports.
- **License**: pick at Stage 0. HeidiSQL (GPL) and pgAdmin (PostgreSQL licence) are downloaded, not bundled, so
  OLS's own license is unaffected.

## Verification (per stage)
- `cargo test --workspace`, `cargo test --features integration` (real binaries, temp home, test ports),
  `pnpm vitest`, and tauri-driver E2E from Stage 8.
- Each stage's **Exit** criterion must pass before the next stage starts.
- Release gates:
  - 0.1: §155 Laravel flow on a clean Windows VM.
  - 0.3: `git clone && ols setup`.
  - 1.0: full §160 E2E on Windows.
  - 2.0: the same on Linux (Ubuntu LTS, Fedora).
