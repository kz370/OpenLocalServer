# OpenLocalServer — Status and Missing Features

As of 2026-09-26. The staged plan is in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md); section numbers (§)
refer to [DevForge_Master_SRS_v4.md](../DevForge_Master_SRS_v4.md).

## Where we are

**Stage 11 of 18, in release 0.2.** The features of release 0.1 and most of 0.2 are built, but 0.1 has not been
released: its gate (a clean Windows VM going from install to a Laravel site over trusted HTTPS) has not been run,
and some 0.1 items are still missing (listed below).

```
Release 0.1   Stages 0–8    ██████████████████░░  built, not released
Release 0.2   Stages 9–11   ████████████████░░░░  9–10 done, 11 in progress
Release 0.3   Stages 12–15  ░░░░░░░░░░░░░░░░░░░░  not started
Release 1.0   Stages 16–18  ██░░░░░░░░░░░░░░░░░░  one item done early (XAMPP/Laragon import)
```

## What works today

- **Runtimes:** PHP, Node, Composer, MySQL, MariaDB, MongoDB, Mailpit, Nginx, Apache, Caddy, SQLite downloaded
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
- **Databases:** create databases and users, connection details, SQLite management, external GUI tools
  (HeidiSQL, pgAdmin, custom), and import from Laragon / XAMPP / WampServer without an SQL file.
- **Quick Apps and Quick Commands:** 13 built-in recipes (including Reverse Proxy), import/export with trust,
  command history.
- **Everyday:** dashboard, environment health, logs (search, filter, export, clear), processes with CPU/RAM,
  system monitor with per-site usage, tray, notifications, start with Windows, leftover-server cleanup.

## Missing features

### Release 0.1 gaps (should come first)

| Feature | § | Stage | Notes |
|---|---|---|---|
| README, LICENSE, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, CHANGELOG | 144 | 0 | License still to be chosen |
| CI on GitHub Actions (build, tests, clippy, lint) | 144 | 0 | Tests exist and pass locally only |
| `.env` editor: add/edit/delete, import/export/compare, hidden secrets, validation | 103 | 4 | |
| Interactive terminal with the project's runtimes on PATH | 19 | 4 | Today: one-shot commands with streamed output |
| "Open with" menu and file shortcuts (project, public/, config/, .env, logs, server configs) | 97, 100 | 4 | "Open folder in editor" exists |
| PostgreSQL service | 31 | 5 | pgAdmin detection exists; the database server does not |
| Redis service | 31 | 5 | Needs a Windows build choice (Memurai vs community port) |
| Custom services UI: name, exe, args, port, health check | 67 | 5 | |
| MySQL / MariaDB backup and restore | 32, 34 | 5 | SQLite backup/restore exists |
| Mailpit `.env` integration (write `MAIL_*` after showing a diff) | 63 | 5 | Laravel Quick App sets it on create only |
| Mail diagnostics checklist and test send | 66 | 5 | |
| Operation journal: rollback and interrupted-operation detection | 78, 163 | 1 | |
| Installer (NSIS/MSI) verified, E2E tests, clean-VM release gate | 155, 160 | 8 | Bundling is configured but untested |

### Release 0.2 — Stage 11 (current)

| Feature | § | Notes |
|---|---|---|
| Xdebug: modes, port, client host, IDE config, per project | 13 | The extension can already be downloaded and enabled |
| Full Composer commands | 14 | Common ones exist as Quick Commands |
| pnpm / yarn through corepack | 15 | |
| Python virtual environments: create, detect, activate, recreate, install | 17 | Python runtime itself not managed yet |
| `DiagnosticEngine` v1: Problem / Cause / Fix / [Fix] / [Ignore] / [Details] | 112 | Errors already carry problem/cause/fix text |

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

- **Helper service install:** the pipe protocol is tested; the one-time UAC install has not been run for real.
- **Database import:** reading and dumping from real Laragon data is tested; loading into our MySQL/MariaDB is not.
- **UI:** new screens pass type-checking and lint but have not been checked visually in the running app.

## Technical debt

- State is stored as JSON files rather than SQLite with migrations (plan decision 1).
- Process trees are stopped with `taskkill /T` instead of Windows Job Objects, so children of a killed app survive
  until the next start cleans them up.
- The plan's separate `platform`, `cli` and `catalog` crates don't exist yet; that code lives in `ols-core`.

## Suggested order from here

1. Close the 0.1 gaps that block a release: repo files and CI, installer check, clean-VM run.
2. Finish Stage 11 (Xdebug, venv, corepack, DiagnosticEngine).
3. The remaining 0.1 features (`.env` editor, terminal, PostgreSQL, Redis, custom services, DB backup).
4. Then release 0.3, starting with the CLI and manifests.
