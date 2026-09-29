# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- The app is now **OLS** (Open Local Server) everywhere a user sees the name: window title, sidebar,
  settings, dialogs, the built-in welcome site, the assistant's system prompt, and the documentation.
  Display only — nothing about behaviour, install layout, project manifests, or stored data changed.
- The requirements document is now `OLS_Master_SRS_v4.md`.
- Product identity guidance, including the identifiers that are deliberately frozen and why, is in
  [`docs/BRAND.md`](./docs/BRAND.md).

### Not changed
These keep their existing names on purpose. Each is read back from a user's machine on a later run, and
renaming it would orphan live state with no way to recover it. See `docs/BRAND.md` §4.3 for the full table.
- Install folder (`C:\OpenLocalServer`), executable name, Inno Setup `AppId`.
- The helper Windows service and its named pipe.
- The keyring service name — it is the addressing key for every stored secret, and the old name cannot be
  written to without the old name, so a rename would destroy every stored password and token.
- The delimited block in the hosts file, and the NRPT rule comment.
- The CA common name, which is already in users' certificate trust stores.
- The `.openlocalserver/` manifest folder, which is committed to users' own repositories.

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
  only after you approve them. `ols ai status|on|off|test|ask|explain`.
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
