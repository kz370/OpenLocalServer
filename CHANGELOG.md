# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- PostgreSQL and Redis services (Redis from the community `redis-windows` build), with databases, users and
  connection details for PostgreSQL.
- Backup and restore of MySQL, MariaDB and PostgreSQL databases, with a safety backup before every restore.
- Custom services: run any program with arguments, port and a health check.
- Mailpit: point a project's `.env` at Mailpit after seeing the change, a mail checklist, and a test message.
- "Open with" menu and shortcuts to a project's folder, public/, config/, `.env`, logs and site config.
- Interactive terminal in a project with its runtimes on PATH.
- Operation journal: operations that were interrupted are reported on the next start.
- Continuous integration on GitHub Actions and the repository documents.
- Runtimes: PHP, Node, Composer, MySQL, MariaDB, MongoDB, Mailpit, Nginx, Apache, Caddy and SQLite, downloaded and
  SHA-256 verified. PHP extensions per version, including PECL downloads.
- Projects: framework detection, per-project runtimes, `.env` editor, Composer and Node package manager commands
  (pnpm and yarn through corepack), Python virtual environments.
- Sites: any domain name, trusted local HTTPS, wildcard subdomains, reverse proxy, automatic `<folder>.test` domains.
- Web servers: generated Nginx, Apache and Caddy configs with drift detection, history and restore.
- Databases: create databases and users, SQLite management, import from Laragon, XAMPP and WampServer.
- Xdebug: modes, port, client host and IDE configuration per PHP version.
- Diagnostics: problem, cause and fix findings on the dashboard, with fix, ignore and details actions.
- Quick Apps and Quick Commands, logs, process and system monitor, tray, start with Windows.
