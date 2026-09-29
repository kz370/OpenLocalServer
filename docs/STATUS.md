# OLS — Status and Missing Features

As of 2026-09-26. The staged plan is in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md); section numbers (§)
refer to [OLS_Master_SRS_v4.md](../OLS_Master_SRS_v4.md).

## Where we are

**Stage 15 of 18 is built, so release 0.3 is feature-complete.** Releases 0.1–0.3 are built, but none is
released: the 0.1 gate (a clean Windows VM going from install to a Laravel site over trusted HTTPS) and the 0.3 gate
(`git clone && ols setup` on a clean machine) have not been run, and some 0.1 items are still missing (listed below).

```
Release 0.1   Stages 0–8    ██████████████████░░  built, not released
Release 0.2   Stages 9–11   ████████████████████  built, not released
Release 0.3   Stages 12–15  ████████████████████  built, not released
Release 1.0   Stages 16–17  ██░░░░░░░░░░░░░░░░░░  Windows; one item done early (XAMPP/Laragon import)
Release 1.1   Stages 18–19  ████████████████████  k6 load testing and the AI assistant, built, not released
Release 2.0   Stage 20      ░░░░░░░░░░░░░░░░░░░░  Linux, last
```

OLS targets Windows; Linux comes last (release 2.0). macOS is not planned.

## What works today

- **Runtimes:** PHP, Node, Composer, MariaDB, PostgreSQL, MongoDB, Redis, Mailpit, Nginx, Apache, Caddy, SQLite downloaded
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
- **Databases:** create databases and users, connection details, backup and restore of MariaDB /
  PostgreSQL, SQLite management, external GUI tools
  (HeidiSQL, pgAdmin, custom), and import from Laragon / XAMPP / WampServer without an SQL file.
- **Quick Apps and Quick Commands:** 13 built-in recipes (including Reverse Proxy), import/export with trust,
  command history.
- **Everyday:** dashboard, environment health, logs (search, filter, export, clear), processes with CPU/RAM,
  system monitor with per-site usage, tray, notifications, start with Windows, leftover-server cleanup.
- **Reproducible environments (0.3):** `.openlocalserver/` manifests (environment, services, commands, lock file),
  a setup plan with conflicts, dry run and apply with rollback, profiles, modes (Development / Testing / Debugging /
  Demo), queue workers, a scheduler, snapshots, settings backups, environment export/import and cloning.
- **Command line:** `ols setup`, `ols doctor`, `ols repair`, `ols status`, `ols start|stop`, and `project`, `runtime`,
  `service`, `tunnel`, `worker`, `snapshot`, `quick-command`, `search`. It drives the open app, or starts a
  background daemon when the app is closed.
- **Tunnels:** Cloudflare, ngrok and LocalTunnel with a first-exposure confirmation, optional
  password, a public badge and one-click stop; traffic inspector (secrets redacted), replay and webhook tester.
- **Power tools:** command palette and global search (Ctrl+Shift+P / Ctrl+K), doctor, project repair, a Git
  manager (status, stage/commit, branches, pull/push, history, remotes, stash, clone), resource limits.

## Missing features

### Release 0.1 gaps (should come first)

Everything else in release 0.1 is now written. What is left needs a person or a machine, not more code:

| Item | § | Notes |
|---|---|---|
| LICENSE file | 144 | GPL-3.0-only LICENSE added; copyright holder line still needs the owner name |
| Installer (NSIS/MSI) verified, E2E tests, clean-VM release gate | 155, 160 | Bundling is configured but never run |
| Full rollback of operations | 78 | The journal finds interrupted operations and offers to run them again; undoing an operation is a written hint, not a button |

### Release 0.2 — Stage 11 (built)

Xdebug, full Composer commands, pnpm/yarn through corepack, Python virtual environments, the `.env` editor and
`DiagnosticEngine` v1 are built. The Python runtime itself is not managed yet, only venvs.

### Release 0.3 (built 2026-09-26)

Everything planned for 0.3 is built (stages 12–15). Limits worth knowing:

| Item | Notes |
|---|---|
| Tunnels | Provider programs (cloudflared, ngrok) are found or located by hand, not downloaded. WebSockets and streamed responses don't pass through the traffic inspector yet |
| Scheduler | Runs while the app or `ols daemon` is open; there is no system-level scheduled task |
| Resource limits | Memory limits apply at the next service start; there is no CPU limit (Windows has no simple per-program cap) |
| Control channel | Windows named pipe only until the Linux stage (20) |

### Release 1.0 (Windows)

| Feature | § | Stage |
|---|---|---|
| Plugins (declarative, then sandboxed WASM) and more runtimes (Go, Ruby, Java, Bun, .NET) | 9, 133–135 | 16 |
| Signed remote catalogs and Git/company Quick App catalogs | 87–88 | 16 |
| Signed auto-updater | 145 | 17 |
| Local HTTP API with a token | 137 | 17 |
| Explorer context menu, offline indicators, telemetry (off by default) | 124, 128, 143 | 17 |
| Full docs and a security review against §138 | 138, 144 | 17 |

### Release 1.1

| Feature | § | Stage |
|---|---|---|
| Load testing with k6: per-project scripts, live results, thresholds, CLI | 160–161 | 18 |
| AI assistant: LM Studio (local), Hugging Face, OpenRouter or any OpenAI-compatible server; explains problems and proposes fixes you approve ([AI_ASSISTANT.md](AI_ASSISTANT.md)) | 168 | 19 (built; never run against a real model) |

### Release 2.0 (last)

| Feature | § | Stage |
|---|---|---|
| Linux: helper, trust store, DNS, packages, control socket, CI; macOS is not planned | — | 20 |

## Built but not yet verified end to end

- **Everything added on 2026-09-26 after Stage 11** was written without compiling or running anything: PostgreSQL
  and Redis services, database backup and restore, custom services, Mailpit `.env` integration and mail
  checks, "Open with" and file shortcuts, the interactive terminal, and the operation journal. Expect a round of
  compile fixes. The UI needs `npm install` (new packages: `@xterm/xterm`, `@xterm/addon-fit`) and the core
  needs `cargo build` (new crate: `portable-pty`) before the lock files are current.
- **Redis** is the community `redis-windows` build (there is no official Windows build).

- **Helper service install:** the pipe protocol is tested; the one-time UAC install has not been run for real.
- **Database import:** reading and dumping from real Laragon data is tested; loading into our MariaDB is not.
- **UI:** new screens (including the Stage 11 ones: Xdebug dialog, project tools, `.env` editor, diagnostics card)
  pass type-checking and lint but have not been checked visually in the running app.
- **Stages 12–15:** 268 core tests pass (setup plans and rollback, snapshots and clones, cron parsing, the
  inspector proxy end to end, tunnel safety, repair, a real-git round trip, the control channel). The CLI was run
  against a temporary home. The new screens (Environment, Git, Workers, Snapshots, Repair tabs; Tunnels and
  Profiles pages; command palette; doctor) pass type-checking but haven't been checked visually. Real tunnel
  providers, `ols daemon` hand-over to the app, and a setup that installs runtimes have not been run end to end.

## Technical debt

- State is stored as JSON files rather than SQLite with migrations (plan decision 1).
- Process trees are stopped with `taskkill /T` instead of Windows Job Objects, so children of a killed app survive
  until the next start cleans them up.
- The plan's separate `platform` and `catalog` crates don't exist yet; that code lives in `ols-core`.

## Suggested order from here

1. Look at every new screen in the running app; try a tunnel with cloudflared and `ols setup` on a real project.
2. Verify the installer (bundle `ols.exe` beside the app), run the clean-VM gate, and release 0.1.
3. Release 1.0 work: plugins, updater, Linux. (The Sites page now holds projects too, aaPanel style, built 2026-09-26 and only type-checked; the web server and certificates moved to a "Web server" page.)
