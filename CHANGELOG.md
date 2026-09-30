# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.2.0] — 2026-09-30

Feature release. Full notes: [`release-notes/v1.2.0.md`](./release-notes/v1.2.0.md).

### Added
- WordPress admin sign-in with no password. A project whose folder holds a WordPress install
  (`wp-config.php`, `wp-admin/index.php`, or `wp-includes` + `wp-content`, checked on disk) gets a
  **WP Admin** action in its Sites row menu. It writes one temporary must-use plugin carrying a
  128-bit token and a five-minute window; WordPress turns the token into a session cookie for the
  chosen administrator and the helper deletes itself. The link works once, from loopback only. No
  password is read, asked for, shown or stored, `wp-config.php` is never opened, and the site
  database is never touched.
- "Cancel pending WP sign-in" removes a link that was made but never used; a helper left behind by a
  crash is swept on the next listing, along with any helper whose expiry cannot be read.
- The WordPress sign-in screen uses the app's own theme tokens (light and dark) and follows the
  colourway the app is showing rather than the operating system's.
- New commands `list_wp_projects`, `wp_sign_in{project_id,hostname,theme}`, `wp_sign_in_revoke{project_id}`.
  All three are local-only: absent from the HTTP API allow-list and the AI command allow-list.
- `olsc repair --dry-run` prints the plan and changes nothing — findings as before, then a count of
  what would run (safe, destructive, manual), with nothing dispatched.
- `scripts\upload-release.bat` gains `full-force-tag` (option 3): `full`, plus re-pointing the release
  tag at the newest commit on `master`. Only the tag moves, never branch history.

## [1.1.1] — 2026-09-30

Fix release. Full notes: [`release-notes/v1.1.1.md`](./release-notes/v1.1.1.md).

### Fixed
- The command line runs through the app executable. `ols.exe` and `OLS.exe` are the same file on
  Windows, so the Explorer right-click entry opened the app instead of adding the folder and
  `ols --version` printed nothing and exited 0. The CLI is now `olsc.exe`, looked up by name and
  refused if it is the running image; the `ols.cmd` shim is gone, because within one directory
  cmd resolves by `PATHEXT` and `.EXE` comes before `.CMD`.
- CLI output was doubled or swallowed in GUI builds: the parent console is attached for the child
  process and the line buffer cleared between runs.
- Clearing a folder from the deletion history did not clear it. The entry was compared as a raw
  string while the stored value was the canonicalized path, so a `..` segment, a short name or a
  junction — and always a GitHub runner, whose `%TEMP%` is a short name — left the entry in place
  and the folder skipped. Both sides are now resolved before comparing. The same defect appended
  a duplicate history entry when one folder was deleted again through another spelling.
- The setup referenced a CLI package name the `olsc` rename had already removed.

### Changed
- Releases are built and published on GitHub: one workflow runs the same
  `scripts\build-installer.bat` as a local release (same stages, same SHA-256 verification) and
  publishes the setup exe, the portable zip, `OLS-<version>-SHA256SUMS.txt` and the signed
  `latest.json`. Nothing runs on a push to `master`; a build runs on request, a release on a
  `v*` tag.

## [1.1.0] — 2026-09-30

Incremental release. Product as described in 1.0.0 is unchanged. Full notes:
[`release-notes/v1.1.0.md`](./release-notes/v1.1.0.md).

### Added
- Bulk site management on the Sites page: select with checkboxes, then enable/disable, add auto domains, move to
  another web server, or delete. Backed by `bulk_set_domains_enabled`, `bulk_set_domain_server` and
  `bulk_remove_domains`, which return one `CoreResponse::Bulk` result naming what changed and what was skipped.
- Pagination on the Sites list.

### Fixed
- Explorer right-click entries opened OLS instead of adding the folder, because `ols.exe` and `OLS.exe` are the
  same file on Windows. The CLI is now `olsc.exe`, looked up by name and refused if it is the running app.
- The `ols` command opened the app instead of running the command line. Within one folder cmd resolves by `PATHEXT`
  and `.EXE` comes before `.CMD`, so the `ols.cmd` shim could never win against `OLS.exe` — `olsc --version` printed
  nothing and exited 0. There is no shim any more: the app hands any argument that is not its own to `olsc.exe`
  before it starts and exits with the CLI's exit code, so `ols` works with the GUI closed and nothing has to be on
  PATH beyond the install folder.
- The shell menu card no longer reports "installed" while every entry points at the app; it checks the command
  it actually wrote.
- Explorer entries use the app's `icon.ico` instead of falling back to the console icon.
- Folders on network shares can be added from the right-click menu. Explorer passes `UNC\server\share` and the
  core produced that same unusable spelling; both are fixed, and a local folder named `UNC…` is left alone.
- The Setup window's "Read the docs" link opens externally via `open_url`.
- Legacy Start menu entries are cleaned up on upgrade.
- Daemon shutdown logs a service the OS refuses to stop, instead of dropping it.

## [1.0.0] — 2026-09-29

First public release. Full notes: [`release-notes/v1.0.0.md`](./release-notes/v1.0.0.md).

### Added
- CPU limits in Settings > Resources, applied through the `cpulimit` utility, which is vendored in
  `vendor/cpulimit/` and installed next to the app. The limit is a share of the whole CPU or a thread
  count (mutually exclusive, converted against the machine's logical cores, rounded up), and a cap that
  cannot be applied is refused by name rather than silently dropped. Applies to databases, caches, mail,
  custom services and all three web servers.
- `CoreCommand::StopAll`: one definition of "Stop all" (tunnels, workers, web stack and PHP pools,
  services, then a 10s reap) shared by the tray, the Dashboard and the HTTP `operate` allow-list, naming
  any process the OS refuses to kill.
- An `ExcludedSitesCard` for excluded sites and project folders.
- SSH key detection, listing and selection in project imports.
- App icons built from master images by script; brand marks for pgAdmin, HeidiSQL, Memcached and Mailpit;
  theme-aware logo handling.
- Per-service log sources, so a service opens on its own log.
- AI assistant, off by default and bring your own model: LM Studio, Ollama, Hugging Face, OpenRouter or any
  OpenAI-compatible server, with keys in the credential store. Explain and fix problems, draft a manifest or a
  k6 script, ask the logs, explain a webhook and write a handler, suggest commit messages, and describe a setup
  in plain words. Secrets are hidden before anything is sent, a prompt preview shows what goes out, a provider
  outside this computer needs a confirmation per request, and proposed steps are limited to an allowlist and run
  only after you approve them. `olsc ai status|on|off|test|ask|explain`.
- Project manifests in `.openlocalserver/` (environment, services, commands and a lock file), and an environment
  setup with a plan, conflict detection, a dry run and rollback of safe changes on failure.
- `ols` command line (`setup`, `doctor`, `repair`, `status`, `start`, `stop`, `project`, `runtime`, `service`,
  `tunnel`, `worker`, `snapshot`, `search`, ...). It drives the open app, or a background daemon.
- Profiles (eight built in, plus your own with import and export) and project modes.
- Queue workers and a scheduler with cron expressions.
- Project snapshots, settings backups, environment export / import, and environment cloning.
- Public tunnels through Cloudflare, ngrok, LocalTunnel or Tailscale Funnel, with a first-exposure confirmation,
  an optional password, a traffic inspector (secrets redacted), replay and a webhook tester.
- Command palette and global search (Ctrl+Shift+P / Ctrl+K), a doctor, project repair, a Git repository manager,
  and resource limits for databases, Node and workers.
- Database import from Laragon, XAMPP or WampServer shows its progress: the current step, bytes copied, exported
  or imported, and which database it is on.
- PostgreSQL and Redis services (Redis from the community `redis-windows` build), with databases, users and
  connection details for PostgreSQL.
- Backup and restore of MariaDB and PostgreSQL databases, with a safety backup before every restore.
- Custom services: run any program with arguments, port and a health check.
- Commands page lists every command a project offers (Artisan, Symfony Console, Composer, package.json scripts,
  Django), with a form built from each command's arguments and options, a live command-line preview, and custom
  commands you can create, edit and save from any command. When a Laravel app can't boot, its commands are read
  from the source files and the page explains why the app failed to start.
- Mailpit: point a project's `.env` at Mailpit after seeing the change, a mail checklist, and a test message.
- "Open with" menu and shortcuts to a project's folder, public/, config/, `.env`, logs and site config.
- Interactive terminal in a project with its runtimes on PATH.
- Operation journal: operations that were interrupted are reported on the next start.
- Continuous integration on GitHub Actions and the repository documents.
- Runtimes: PHP, Node, Composer, MariaDB, MongoDB, Mailpit, Nginx, Apache, Caddy and SQLite, downloaded and
  SHA-256 verified. PHP extensions per version, including PECL downloads.
- Projects: framework detection, per-project runtimes, `.env` editor, Composer and Node package manager commands
  (pnpm and yarn through corepack), Python virtual environments.
- Sites: any domain name, trusted local HTTPS, wildcard subdomains, reverse proxy, automatic `<folder>.test` domains.
- Web servers: generated Nginx, Apache and Caddy configs with drift detection, history and restore.
- Databases: create databases and users, SQLite management, import from Laragon, XAMPP and WampServer.
- Xdebug: modes, port, client host and IDE configuration per PHP version.
- Diagnostics: problem, cause and fix findings on the dashboard, with fix, ignore and details actions.
- Quick Apps and Quick Commands, logs, process and system monitor, tray, start with Windows.

### Fixed
- A `localhost/<prefix>` route sent every request to the site domain, so the route was unreachable on any site
  with TLS and "Redirect to HTTPS" on. Nginx, Apache and Caddy now proxy to the site's HTTPS vhost with SNI and
  send `X-Forwarded-Prefix`.
- The add-site form's localhost path had no autofill; it now follows the project name, like the site folder and
  the domain, and writes nothing for a slug too long to be valid.
- The window could be revealed transparent before the UI painted; the pre-paint period now paints the app background.
- Dashboard and Web server pages crowded their controls between `lg` and `xl`; layouts are fluid and the Services
  card keeps its state, name and five colour-coded actions at every width.
- Two commands named "Stop all" had different scopes, so a stopped stack could still hold PHP pools and leave the
  status mark green.
- A site settings page could hang on "Reading the project…" forever: gating reads now have a deadline, a catch, an
  error state and a Retry.
- A poisoned core mutex kept panicking every later command; a poisoned guard is recovered.
- Database backup refused any database name containing `-`, `.` or a space; only names that cannot become a file
  name are refused.
- Helper service replies can no longer block indefinitely.
- Site removal: the row disappears at once, the domain store is not held across a multi-second apply, the project
  goes with its last site, and the confirm dialog names what it deletes.
- MariaDB's first start failed with "Can't create data directory" when its services folder didn't exist yet.

### Changed
- The app is OpenLocalServer everywhere: the documents no longer use the working name DevForge.
- Multiple web servers, each with its own HTTP/HTTPS ports, instead of a single pair.
- Configurable default TLD with project-name autofill in the domain dialog.
- State storage is SQLite (`data/app.db`), with DB Browser integration.
- MariaDB is the only MySQL-compatible server and runs on the standard port 3306. MySQL is no longer offered:
  Quick Apps, the Databases and Services pages, backups and imports all use MariaDB. Databases from Laragon,
  XAMPP or WampServer MySQL can still be imported into it.
- The Sites list shows only Open and Settings per site; the other actions are in a menu.
- The import dialog's password field sits beside its button.
- The app is now **OLS** (Open Local Server) everywhere a user sees the name: window title, sidebar,
  settings, dialogs, the built-in welcome site, the assistant's system prompt, and the documentation.
- The executable is now **`OLS.exe`**, so the process name in Task Manager, the Start menu entry and
  the desktop shortcut read OLS too. The installer, the portable zip and the SHA256SUMS file follow
  (`OLS-<version>-setup.exe`, `OLS-<version>-portable-win-x64.zip`, `OLS-<version>-SHA256SUMS.txt`).
  An existing install upgrades in place: the installer detects the app under its old name, closes it,
  writes `OLS.exe` and removes the old executable. No settings, sites, secrets, service or
  certificate move, and nothing about how it works changed.
- The requirements document is now `OLS_Master_SRS_v4.md`.
- Product identity guidance, including the identifiers that are deliberately frozen and why, is in
  [`docs/BRAND.md`](./docs/BRAND.md). The exe rename is recorded in §4.5, including the one thing
  that would have broken silently without it — a pre-rename install holding the file being replaced.

### Not changed
These keep their existing names on purpose. Each is read back from a user's machine on a later run, and
renaming it would orphan live state with no way to recover it. See `docs/BRAND.md` §4.3 for the full table.
- Install folder (`C:\OpenLocalServer`), Inno Setup `AppId`, the helper install folder.
- The `openlocalserver.exe` daemon binary, which is not the app and was never renamed.
- The helper Windows service and its named pipe.
- The keyring service name — it is the addressing key for every stored secret, and the old name cannot be
  written to without the old name, so a rename would destroy every stored password and token.
- The delimited block in the hosts file, and the NRPT rule comment.
- The CA common name, which is already in users' certificate trust stores.
- The `.openlocalserver/` manifest folder, which is committed to users' own repositories.
