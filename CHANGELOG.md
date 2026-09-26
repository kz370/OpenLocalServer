# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- The app is OpenLocalServer everywhere: the documents no longer use the working name DevForge.
- The Sites list shows only Open and Settings per site; the other actions are in a menu.
- The import dialog's password field sits beside its button.
- MariaDB is the only MySQL-compatible server and runs on the standard port 3306. MySQL is no longer offered:
  Quick Apps, the Databases and Services pages, backups and imports all use MariaDB. Databases from Laragon,
  XAMPP or WampServer MySQL can still be imported into it.

### Fixed
- MariaDB's first start failed with "Can't create data directory" when its services folder didn't exist yet.

### Added
- AI assistant (stage 19), off by default and bring your own model: LM Studio, Ollama, Hugging Face, OpenRouter or any
  OpenAI-compatible server, with keys in the credential store. Explain and fix problems, draft a manifest or a k6 script,
  ask the logs, explain a webhook and write a handler, suggest commit messages, and describe a setup in plain words.
  Secrets are hidden before anything is sent, a prompt preview shows what goes out, a provider outside this computer needs
  a confirmation per request, and proposed steps are limited to an allowlist and run only after you approve them.
  `ols ai status|on|off|test|ask|explain`.
- Release 0.3 features (stages 12–15):
  - Project manifests in `.openlocalserver/` (environment, services, commands and a lock file), and an
    environment setup with a plan, conflict detection, a dry run and rollback of safe changes on failure.
  - `ols` command line (`setup`, `doctor`, `repair`, `status`, `start`, `stop`, `project`, `runtime`, `service`,
    `tunnel`, `worker`, `snapshot`, `search`, ...). It drives the open app, or a background daemon.
  - Profiles (eight built in, plus your own with import and export) and project modes.
  - Queue workers and a scheduler with cron expressions.
  - Project snapshots, settings backups, environment export / import, and environment cloning.
  - Public tunnels through Cloudflare, ngrok, LocalTunnel or Tailscale Funnel, with a first-exposure
    confirmation, an optional password, a traffic inspector (secrets redacted), replay and a webhook tester.
  - Command palette and global search (Ctrl+Shift+P / Ctrl+K), a doctor, project repair, a Git repository
    manager, and resource limits for databases, Node and workers.
- Database import from Laragon, XAMPP or WampServer shows its progress: the current step, bytes copied,
  exported or imported, and which database it is on.
- PostgreSQL and Redis services (Redis from the community `redis-windows` build), with databases, users and
  connection details for PostgreSQL.
- Backup and restore of MariaDB and PostgreSQL databases, with a safety backup before every restore.
- Custom services: run any program with arguments, port and a health check.
- Commands page lists every command a project offers (Artisan, Symfony Console, Composer, package.json scripts,
  Django), with a form built from each command's arguments and options, a live command-line preview, and
  custom commands you can create, edit and save from any command. When a Laravel app can't boot, its
  commands are read from the source files and the page explains why the app failed to start.
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
