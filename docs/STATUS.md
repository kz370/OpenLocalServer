# OpenLocalServer — Status and Missing Features

As of 2026-09-26. The staged plan is in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md); section numbers (§)
refer to [DevForge_Master_SRS_v4.md](../DevForge_Master_SRS_v4.md).

## Where we are

**Stage 11 of 18 is built, so release 0.2 is feature-complete.** The features of releases 0.1 and 0.2 are built,
but 0.1 has not been released: its gate (a clean Windows VM going from install to a Laravel site over trusted HTTPS) has not been run,
and some 0.1 items are still missing (listed below).

```
Release 0.1   Stages 0–8    ██████████████████░░  built, not released
Release 0.2   Stages 9–11   ████████████████████  built, not released
Release 0.3   Stages 12–15  ░░░░░░░░░░░░░░░░░░░░  not started
Release 1.0   Stages 16–18  ██░░░░░░░░░░░░░░░░░░  one item done early (XAMPP/Laragon import)
```

## What works today

- **Runtimes:** PHP, Node, Composer, MySQL, MariaDB, PostgreSQL, MongoDB, Redis, Mailpit, Nginx, Apache, Caddy, SQLite downloaded
  and SHA-256 verified; existing installs (Laragon, XAMPP) can be registered instead. PHP extensions per version,
  with PECL downloads.
- **Projects:** register or scan folders, framework and version detection, per-project runtime resolution,
  running commands with the project's runtimes.
- **Sites:** domains with any name, trusted local HTTPS, PHP per site version, static sites, reverse proxy to any
  host and port, wildcard subdomains, Laragon-style automatic `<folder>.test` domains, rename, open in editor.
- **Web servers:** Nginx, Apache and Caddy with generated configs, Managed/Advanced/Manual ownership, drift
  detection, history with diff and restore, structured editing of common blocks.
- **Name resolution without repeated admin prompts:** built-in DNS for `.test` / `.localhost` / `.internal`, and
  an optional helper service for everything else.
- **Databases:** create databases and users, connection details, backup and restore of MySQL / MariaDB /
  PostgreSQL, SQLite management, external GUI tools
  (HeidiSQL, pgAdmin, custom), and import from Laragon / XAMPP / WampServer without an SQL file.
- **Quick Apps and Quick Commands:** 13 built-in recipes (including Reverse Proxy), import/export with trust,
  command history.
- **Everyday:** dashboard, environment health, logs (search, filter, export, clear), processes with CPU/RAM,
  system monitor with per-site usage, tray, notifications, start with Windows, leftover-server cleanup.

## Missing features

### Release 0.1 gaps (should come first)

Everything else in release 0.1 is now written. What is left needs a person or a machine, not more code:

| Item | § | Notes |
|---|---|---|
| LICENSE file | 144 | The workspace says MIT, but the copyright holder and the final choice are yours to make |
| Installer (NSIS/MSI) verified, E2E tests, clean-VM release gate | 155, 160 | Bundling is configured but never run |
| Full rollback of operations | 78 | The journal finds interrupted operations and offers to run them again; undoing an operation is a written hint, not a button |

### Release 0.2 — Stage 11 (built)

Xdebug, full Composer commands, pnpm/yarn through corepack, Python virtual environments, the `.env` editor and
`DiagnosticEngine` v1 are built. The Python runtime itself is not managed yet, only venvs.

### Release 0.3

| Feature | § | Stage |
|---|---|---|
| Command-line tool over the app's control channel | 136 | 12 |
| Project manifests, lock file, `setup`, `--dry-run`, plan preview, rollback | 71–78, 159 | 12 |
| Full conflict detection including file ownership | 75 | 12 |
| Profiles and project modes | 69–70 | 13 |
| Queue workers and a GUI scheduler | 105–106 | 13 |
| Snapshots, backups, import/export, environment cloning | 130–132, 158 | 13 |
| Tunnels (Cloudflare, ngrok, LocalTunnel, Tailscale) with safety rails | 54–60, 140 | 14 |
| Request/traffic inspector and webhook tester | 110–111 | 14 |
| Command palette and global search | 122–123 | 15 |
| Automatic repair, explained diagnostics, `doctor` | 113–115 | 15 |
| Git repository manager: clone, status, branches, commit, pull/push, diff, log, remotes (added 2026-09-26) | 125 | 15 |
| Resource controls | 129 | 15 |

### Release 1.0

| Feature | § | Stage |
|---|---|---|
| macOS and Linux (helper, trust store, DNS, packages, CI matrix) | — | 16 |
| Plugins (declarative, then sandboxed WASM) and more runtimes (Go, Ruby, Java, Bun, .NET) | 9, 133–135 | 17 |
| Signed remote catalogs and Git/company Quick App catalogs | 87–88 | 17 |
| Signed auto-updater | 145 | 18 |
| Local HTTP API with a token | 137 | 18 |
| Explorer context menu, offline indicators, telemetry (off by default) | 124, 128, 143 | 18 |
| Full docs and a security review against §138 | 138, 144 | 18 |

## Built but not yet verified end to end

- **Everything added on 2026-09-26 after Stage 11** was written without compiling or running anything: PostgreSQL
  and Redis services, database backup and restore, custom services, Mailpit `.env` integration and mail
  checks, "Open with" and file shortcuts, the interactive terminal, and the operation journal. Expect a round of
  compile fixes. The UI needs `npm install` (new packages: `@xterm/xterm`, `@xterm/addon-fit`) and the core
  needs `cargo build` (new crate: `portable-pty`) before the lock files are current.
- **Redis** is the community `redis-windows` build (there is no official Windows build).

- **Helper service install:** the pipe protocol is tested; the one-time UAC install has not been run for real.
- **Database import:** reading and dumping from real Laragon data is tested; loading into our MySQL/MariaDB is not.
- **UI:** new screens (including the Stage 11 ones: Xdebug dialog, project tools, `.env` editor, diagnostics card)
  pass type-checking and lint but have not been checked visually in the running app.

## Technical debt

- State is stored as JSON files rather than SQLite with migrations (plan decision 1).
- Process trees are stopped with `taskkill /T` instead of Windows Job Objects, so children of a killed app survive
  until the next start cleans them up.
- The plan's separate `platform`, `cli` and `catalog` crates don't exist yet; that code lives in `ols-core`.

## Suggested order from here

1. Build, fix compile errors, run the tests, and look at every new screen in the running app.
2. Choose the license, verify the installer, run the clean-VM gate, and release 0.1.
3. Then release 0.3, starting with the CLI and manifests.
