# OpenLocalServer

A local development environment manager for Windows. It downloads and runs PHP, Node, databases and web servers,
gives every project its own runtimes and its own domain, and serves sites over trusted local HTTPS, without
editing your system by hand.

> Status: pre-release. See [docs/STATUS.md](docs/STATUS.md) for what works and what is missing.

## Features

- **Runtimes:** PHP (with extensions and Xdebug), Node, Composer, MySQL, MariaDB, MongoDB, Mailpit, Nginx, Apache,
  Caddy and SQLite. Downloads are SHA-256 verified, or register an existing Laragon or XAMPP install.
- **Sites:** any domain name, trusted local HTTPS, PHP version per site, static sites, reverse proxy, wildcard
  subdomains.
- **Projects:** framework detection, `.env` editor, Composer, npm/pnpm/yarn and Python virtual environments.
- **Databases:** create databases and users, SQLite tools, import from Laragon, XAMPP and WampServer.
- **Everyday:** dashboard with diagnostics, logs, process and system monitor, tray, start with Windows.

## Run from source

```
dev.bat
```

Needs Rust (stable), Node 22 and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).
See [CONTRIBUTING.md](CONTRIBUTING.md).

## Documentation

- [Implementation plan](docs/IMPLEMENTATION_PLAN.md)
- [Status and missing features](docs/STATUS.md)
- [Security policy](SECURITY.md)
