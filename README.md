# OpenLocalServer

> A modern local development environment manager for Windows — runtimes, sites, databases, and trusted HTTPS, without touching your system by hand.

[![Build](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/openlocalserver/openlocalserver/actions)
[![Version](https://img.shields.io/badge/version-0.3.0--pre-blue)](./CHANGELOG.md)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D6)](./docs/STATUS.md)
[![License](https://img.shields.io/badge/license-MIT-lightgrey)](./LICENSE)
[![Tauri](https://img.shields.io/badge/shell-Tauri_2-orange)](./src-tauri/tauri.conf.json)
[![Rust](https://img.shields.io/badge/backend-Rust-red)](./crates/ols-core)

**OpenLocalServer** is the successor to XAMPP / Laragon with broader runtime coverage and full extensibility. Every project gets isolated runtimes, its own `.test` domain, and trusted local HTTPS. One core engine (`ols-core`) powers three front doors: desktop GUI, CLI (`ols`), and local HTTP API.

> **Status:** pre-release — Stage 15/18 built, release 0.3 feature-complete but not yet verified end-to-end. See [docs/STATUS.md](./docs/STATUS.md) for what works and what's missing.

---

## 📸 Demo / Screenshots

> Screenshots live in `./assets/`. Drop your PNGs there with matching filenames.

<p align="center">
  <img width="700" src="./assets/dashboard.png" alt="Dashboard Overview — environment health, services, and diagnostics">
  <br><em>Dashboard — environment health, services, and diagnostics</em>
</p>

<p align="center">
  <img width="700" src="./assets/sites.png" alt="Sites and Projects — domains, PHP per site, HTTPS, reverse proxy">
  <br><em>Sites & Projects — domains, per-site PHP, HTTPS, reverse proxy</em>
</p>

<p align="center">
  <img width="700" src="./assets/terminal-git.png" alt="Terminal, Git manager, workers, and tunnel inspector">
  <br><em>Terminal, Git manager, workers, and tunnel inspector</em>
</p>

---

## ✨ Features

### Runtimes & Toolchain
- **Managed runtimes:** PHP NTS 8.1–8.5 (+ Xdebug, per-version extensions, PECL), Node 22/24 (+ corepack npm/pnpm/yarn), Composer, Python venvs, portable Git, k6
- **SHA-256 verified downloads**, PATH probe cache, or register existing Laragon / XAMPP installs
- **Per-project runtime resolution** — manifest > detected > global

### Sites & Web Servers
- Any domain name, wildcard subdomains, Laragon-style automatic `<folder>.test`
- **Trusted local HTTPS** via local CA (rcgen, 397-day leaves, auto-renew < 30d)
- **Per-site PHP version**, static sites, reverse proxy to any host:port
- Nginx 1.28, Apache 2.4, Caddy 2.11 with generated configs, Managed/Advanced/Manual modes, drift detection, history with diff + restore
- Built-in DNS for `.test` / `.localhost` / `.internal` + optional elevated helper for hosts/NRPT (no repeated UAC prompts)

### Projects & Reproducible Environments
- Framework + version detection (`composer.json`, `package.json`, `manage.py`, …)
- `.openlocalserver/*.yaml` manifests (environment, services, commands, lock file)
- 14-step setup pipeline: plan → conflicts → dry-run → apply with journal + rollback
- Profiles & modes (Development / Testing / Debugging / Demo), snapshots, cloning, export/import
- `.env` editor, Composer/npm/pnpm/yarn/Python venv runners, Quick Apps (13 built-in recipes) + Quick Commands

### Databases & Services
- MariaDB 11.4, PostgreSQL, MongoDB, Redis (`redis-windows`), Mailpit, SQLite
- Create DBs/users, connection details, backup/restore, SQLite tools
- One-click external GUIs: HeidiSQL, pgAdmin, NoSQLBooster, Tiny RDM
- Import from Laragon / XAMPP / WampServer without SQL dumps

### Everyday Workflow
- Dashboard with diagnostics, log viewer (search/filter/export/clear), process + system monitor, per-site usage
- Command palette (`Ctrl+Shift+P`) + global search (`Ctrl+K`), doctor with safe auto-repair
- Git manager (status, stage/commit, branches, pull/push, remotes, stash, clone)
- Queue workers (max 16), scheduler (cron, 1-min tick), interactive terminal (portable-pty, max 8)
- Tunnels (Cloudflare / ngrok / LocalTunnel) with exposure confirmation, password, badge, one-click stop + traffic inspector (redacted, replay, webhook tester)
- Tray icon, notifications, start with Windows, leftover-server cleanup, resource limits
- CLI: `ols setup | doctor | repair | status | start | stop` + `project | runtime | service | tunnel | worker | snapshot | quick-command | search`, daemon mode when GUI closed
- Power tools: k6 load testing, AI assistant (LM Studio / Hugging Face / OpenRouter / OpenAI-compatible)

---

## 🚀 Getting Started

### Prerequisites

| Requirement | Version | Notes |
|---|---|---|
| Windows | 10 / 11 (64-bit) | Linux planned (2.0); macOS not planned |
| Rust | stable | `rustup` toolchain |
| Node.js | 22+ | UI build |
| Tauri prerequisites | — | WebView2, MSVC, WiX — see [Tauri prerequisites](https://tauri.app/start/prerequisites/) |

### Install dependencies

```powershell
# Rust toolchain (if missing)
winget install Rustlang.Rustup

# Node 22 (if missing)
winget install OpenJS.NodeJS.LTS --version 22

# UI dependencies
cd ui
npm install
```

### Run locally (dev)

```powershell
# From repo root — starts Tauri + Vite + Rust core
.\dev.bat
```

This launches the desktop app with hot-reload (Vite on `http://localhost:1420`).

### Build for production

```powershell
# Core + helper checks
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper

# UI lint + build
cd ui
npm run lint
npm run build

# Tauri bundle (NSIS/MSI — configured, not yet verified, see docs/STATUS.md)
cd ..
cargo tauri build
```

---

## 💻 Usage

### Register a project and go live

```powershell
# Clone any PHP / Node / Python project
git clone https://github.com/example/my-laravel-app.git
cd my-laravel-app

# Full environment setup: runtimes → DB → domain → DNS → SSL → mail → workers
ols setup

# Preview plan without applying
ols setup --dry-run

# Check health, auto-fix safe issues
ols doctor
ols repair
```

### Daily commands

```powershell
ols status                  # services, sites, runtimes
ols start                   # start all enabled services
ols stop                    # stop all

ols project list            # registered projects
ols service list            # MariaDB, Postgres, Redis, Mailpit…
ols runtime list            # installed PHP/Node versions

ols tunnel start --provider cloudflared --port 443
ols worker run --queue default
ols snapshot create --name "before-upgrade"
ols search "mailpit"        # global search
```

### Typical flow in GUI

1. **Add project** — register or scan folder → framework auto-detected
2. **Setup** — review 14-step plan → Apply (rollback on failure)
3. **Open site** — `https://myapp.test` with trusted cert, per-site PHP
4. **Develop** — `.env` editor, terminal, Git tab, logs, Mailpit for mail
5. **Share** — tunnel with confirmation → inspector → one-click stop

> All errors surface as `Diagnostic{ problem, cause, fix }` — what broke, why, and how to fix it.

---

## 📁 Project Structure

```text
OpenLocalServer/
├── crates/
│   ├── ols-core/        # All logic — CoreCommand dispatcher (~150-200 variants),
│   │                    # managers: Runtime, Service, Web, Domain, Certs, Project,
│   │                    # ProcessSupervisor, Workers, Scheduler, Tunnel, Mail,
│   │                    # Diagnostics, QuickApp, Plugin
│   ├── ols-helper/      # Elevated helper — hosts/NRPT/service via UAC or
│   │                    # LocalSystem named pipe (closed validated set)
│   └── ols-cli/         # `ols` CLI — drives app or background daemon
├── src-tauri/           # Tauri 2 shell — window, tray, single IPC `run_command`
├── ui/src/              # React 19 + Vite 8 + Tailwind 4 + shadcn/ui,
│   │                    # CodeMirror 6, xterm.js, `core.ts` IPC wrapper
│   ├── pages/           # Dashboard, Sites, Environment, Git, Workers,
│   │                    # Snapshots, Tunnels, Profiles, Repair, Doctor…
│   └── components/      # shadcn/ui primitives, dialogs, editors
├── data/                # Portable JSON state (SQLite migration planned),
│                        # OLS_HOME env overrides all paths
├── docs/                # STATUS.md, IMPLEMENTATION_PLAN.md, AI_ASSISTANT.md…
├── specs/               # Single source of truth — architecture, catalog,
│                        # relationships, data models, API reference, diagrams
├── installer/ scripts/  # NSIS/MSI bundling, dev.bat, build-installer.bat
├── assets/              # App icons + README screenshots (dashboard.png,
│                        # sites.png, terminal-git.png)
└── OpenLocalServer_Master_SRS_v4.md  # Full requirements spec (v4)
```

**Architecture (one core, many front doors):**

```text
GUI (React) ──┐
CLI (ols) ────┼──> Core::dispatch(CoreCommand) -> Inner (Arc-shared state)
HTTP API ─────┘         |
                        v
              Managers: Runtime, Service, Web, Domain, Certs,
              Project, ProcessSupervisor, Workers, Scheduler,
              Tunnel, Mail, Diagnostics, QuickApp, Plugin
```

---

## 🤝 Contributing

Contributions welcome. Please read [CONTRIBUTING.md](./CONTRIBUTING.md) and the [Code of Conduct](./CODE_OF_CONDUCT.md) first.

```powershell
# Quality gates — must pass before PR
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper
cd ui; npm run lint; npm run build
```

- **Specs-sync mandate:** `specs/` is source of truth. Any behavior change must update matching `specs/` files in same PR + append log line to `specs/runtime.md`. Behavior PR without spec update gets rejected. See [AGENTS.md](./AGENTS.md).
- **Safety:** user errors use `Diagnostic{problem,cause,fix}` · downloads verify SHA-256 · secrets in OS keyring only, never logs/files/bundles.
- **Commits:** Conventional Commits (`feat:`, `fix:`, `docs:` …).

---

## 📄 License

MIT (workspace-declared) — copyright holder TBD, license file pending (see `docs/STATUS.md` 0.1 gaps). See [SECURITY.md](./SECURITY.md) for reporting policy.

---

<p align="center">
  Built with Rust · Tauri 2 · React 19 · Tokio
  <br>
  <a href="./docs/STATUS.md">Status</a> ·
  <a href="./docs/IMPLEMENTATION_PLAN.md">Implementation Plan</a> ·
  <a href="./OpenLocalServer_Master_SRS_v4.md">Master SRS v4</a> ·
  <a href="./CONTRIBUTING.md">Contributing</a>
</p>
