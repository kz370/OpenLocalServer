# OpenLocalServer (OLS) — Comprehensive User Manual & Operations Guide

> **Version:** 2.10.0
> **Platform:** Windows 10 / Windows 11 (64-bit)  
> **License:** GPL-3.0-only  
> **CLI Binary:** `olsc`  
> **Desktop Binary:** `OLS.exe`  

---

## Table of Contents

1. [Overview & Core Architecture](#1-overview--core-architecture)
2. [First Run & Quick Start](#2-first-run--quick-start)
3. [Sites & Virtual Hosts Management](#3-sites--virtual-hosts-management)
4. [Domains, Networking & Remote Access](#4-domains-networking--remote-access)
5. [Web Servers (Nginx, Apache, Caddy)](#5-web-servers-nginx-apache-caddy)
6. [Quick Apps (1-Click Deployment)](#6-quick-apps-1-click-deployment)
7. [Commands & Developer Tools](#7-commands--developer-tools)
8. [Runtimes & Extensions (PHP, Node, Python)](#8-runtimes--extensions-php-node-python)
9. [Databases & Infrastructure Services](#9-databases--infrastructure-services)
10. [Public Tunnels & Webhook Inspection](#10-public-tunnels--webhook-inspection)
11. [Backups, Snapshots & Encrypted Site Bundles](#11-backups-snapshots--encrypted-site-bundles)
12. [Monitoring, Logs & Background Tasks](#12-monitoring-logs--background-tasks)
13. [Diagnostics, Health & Doctor Auto-Repair](#13-diagnostics-health--doctor-auto-repair)
14. [AI Assistant & Model Context Protocol (MCP)](#14-ai-assistant--model-context-protocol-mcp)
15. [Command Line Interface (CLI — `olsc`)](#15-command-line-interface-cli--olsc)
16. [Keyboard Shortcuts, Settings, Themes & Documentation](#16-keyboard-shortcuts-settings-themes--documentation)
17. [Version 2.10.0 Specific Notes & What's New](#17-version-2100-specific-notes--whats-new)

---

## 1. Overview & Core Architecture

OpenLocalServer (OLS) is a fast, privacy-first local web development environment built specifically for Windows. It provides a complete alternative to traditional AMP/LEMP stacks, container sprawl, and bloated local dev tooling.

### Key Architectural Pillars
- **Zero-Config `.test` Domains:** Automatic local DNS resolution for `.test` domains without manually editing the Windows `C:\Windows\System32\drivers\etc\hosts` file for every site.
- **Local Certificate Authority (CA):** Seamless HTTPS with trusted local certificates generated on-demand.
- **Multi-Server Flexibility:** Switch between **Nginx**, **Apache**, and **Caddy** with one click.
- **Process Supervision:** Lightweight, non-blocking process management for PHP-FPM pools, database engines, Mailpit, Redis, tunnels, and workers.
- **Privacy & Security:** Zero analytics, zero telemetry. Outbound network traffic occurs only for user-initiated package downloads, catalog updates, tunnels, or configured AI providers.

---

## 2. First Run & Quick Start

Follow these 4 simple steps to set up your environment:

1. **Install a PHP Runtime:**
   - Navigate to the **Runtimes** page from the sidebar.
   - Click **Install** on your desired PHP version (e.g., PHP 8.2 or 8.3).
   - OLS downloads official binaries, verifies the SHA-256 checksum, and sets up isolated configuration.

2. **Trust the Local Certificate Authority:**
   - Go to **Web server** → **Certificates** tab.
   - Click **Trust Certificate Authority**.
   - Windows will prompt for administrative permission once. After approval, all `.test` HTTPS sites will load in Chrome, Edge, and Firefox without security warnings.

3. **Create or Link Your First Site:**
   - Go to the **Sites** page.
   - Click **Add Site / Link Project**.
   - Enter your domain (e.g. `mysite.test`) and choose your project folder.
   - If the folder does not exist, OLS automatically creates the directory and generates an initial welcome `index.html` or `index.php`.

4. **Start the Web Server & Services:**
   - Click the green **Start All** button in the top titlebar or sidebar.
   - Visit `http://mysite.test` or `https://mysite.test` in your browser!

---

## 3. Sites & Virtual Hosts Management

The **Sites** page is your central command hub for managing websites and web applications.

### Adding & Configuring Sites
- **Site Types:**
  - **Static:** Serves HTML, CSS, client-side JS, images directly via Nginx, Apache, or Caddy.
  - **Plain PHP / Framework:** Directs requests to the appropriate PHP-FPM fastcgi pool.
  - **Reverse Proxy:** Forwards traffic to an internal port (e.g. Node.js on port 3000, Vite on 5173, Python FastAPI on 8000).
- **Automatic Folder & Welcome Page Creation (v2.3.0):**
  - If you enter a project folder path that does not yet exist on disk, OLS will automatically create the folder (`std::fs::create_dir_all`).
  - If no index file exists in the folder, OLS automatically generates a beautiful welcome page (`index.html` for static sites, or `index.php` with an interactive PHP Info inspector for PHP sites).
- **Document Root Customization:**
  - Supports modern framework conventions such as `/public` (Laravel, Symfony) or `/dist` (Vite, React, Vue).
- **Domain Aliases:**
  - Add multiple aliases for a single site (e.g. `admin.mysite.test`, `api.mysite.test`).
  - Safety confirmations prevent accidental deletion.
- **Site Logos & Offline Icons:**
  - Assign custom icons or choose from built-in Font Awesome SVGs with custom hex colors.
  - Upload custom images (automatically converted client-side to WebP and saved in the project root as `ols_site_icon.webp`).
- **Missing Folder Detection & "Folder not found" Tab (v2.5.0):**
  - Isolates projects with deleted folders or offline external drives into the dedicated **Folder not found** tab.
  - Keeps the **Enabled** and **Disabled** tabs strictly operational.
  - Provides a 1-click **Locate** action directly in the row to relocate moved folders.
- **Multi-Domain Project Grouping (v2.5.0):**
  - Consolidates multiple domains sharing the same project folder into a single unified row with primary domain focus.
  - Extra domains appear as interactive pill badges with real-time status dots; clicking any pill opens its configuration dialog.
  - Bulk actions and deletion apply cleanly across all domains in the project row.
- **Right-Click Context Menu (v2.5.0):**
  - Right-clicking any site row opens the cursor-positioned actions menu with smart viewport bounds clamping and smooth internal scrolling.

---

## 4. Domains, Networking & Remote Access

OLS provides enterprise-grade local and remote networking capabilities:

### Local DNS & Multi-Interface Web Binding (v2.3.0)
- **Local Resolution:** Resolves all `*.test` domains to `127.0.0.1` locally.
- **Multi-Interface Web Binding (`0.0.0.0`):**
  - In v2.3.0, Web servers bind to `0.0.0.0`, allowing mobile devices, tablets, and other computers on your local WiFi/LAN to access your dev sites via your LAN IP.
- **Host Header Routing Fix:**
  - Public and LAN host headers properly route to virtual hosts across all web servers without falling through to default 404 pages.

### Dynamic DNS (DDNS) & Site Broadcast Controls
- **Site Broadcast Controls:**
  - Directly from the Site Domains panel, toggle site broadcasting and copy public URLs with a single click.
- **Dynamic DNS (12+ DNS Providers):**
  - Synchronizes your custom domain with your live external IP address through Cloudflare, DuckDNS, No-IP, Porkbun, Dynu, Namecheap, FreeDNS, DigitalOcean, Hetzner, and other popular DNS providers.
  - Automatic collision resolution, TTL management, and router port forwarding guidance.

---

## 5. Web Servers (Nginx, Apache, Caddy)

OLS gives you full control over your web server layer:

- **Switching Servers:**
  - Open **Web server** in the sidebar.
  - Switch between **Nginx**, **Apache**, and **Caddy** at any time. Configurations are automatically regenerated and reloaded without port conflicts.
- **Configuration Editing & Syntax Testing:**
  - Edit web server configs directly in the built-in syntax-highlighted editor.
  - Click **Test Config** before applying to verify syntax validity and prevent server crashes.
- **Apache `.htaccess` Support:**
  - Apache sites fully respect local `.htaccess` files.
  - Includes a built-in `.htaccess` editor with AI-assisted review and diff previews.

---

## 6. Quick Apps (1-Click Deployment)

The **Quick Apps** page allows you to spin up production-ready web frameworks in seconds:

### Supported Recipes
- **PHP:** WordPress, Laravel, Symfony, Plain PHP.
- **Node.js / Frontend:** React (Vite), Vue (Vite), Next.js, Express API.
- **Python:** Django, FastAPI, Flask.
- **Custom App:** Reverse proxy or arbitrary scaffold recipes.

### How Quick Apps Work
1. Select a recipe from **Quick Apps**.
2. Specify the project name and domain (e.g. `myblog.test`).
3. OLS creates the site record, provisions databases, executes composer/npm scaffolding, and opens the site ready for development.

---

## 7. Commands & Developer Tools

The **Commands** page provides quick access to framework and project CLI actions:
- **Artisan (Laravel):** `migrate`, `db:seed`, `route:list`, `cache:clear`, `queue:work`.
- **Composer:** `install`, `update`, `dump-autoload`.
- **npm / yarn / pnpm / bun:** `install`, `run dev`, `run build`.
- **WP-CLI (WordPress):** Core update, plugin install, database export.
- **Interactive Terminal:** Open a built-in terminal or launch Windows Terminal / PowerShell with the project environment variables pre-loaded.

---

## 8. Runtimes & Extensions (PHP, Node, Python)

OLS isolates development runtimes without polluting your Windows system `PATH`:

- **PHP Management:**
  - Supports multiple concurrent PHP versions (7.4, 8.0, 8.1, 8.2, 8.3, 8.4, and 8.5+).
  - **PHP 8.5+ Built-in OPcache & Windows ASLR Fix:** In PHP 8.5+, OPcache is built into the PHP core rather than an external `php_opcache.dll`. OLS automatically configures `opcache.file_cache` and `opcache.file_cache_fallback = 1` for Windows FastCGI workers, preventing Address Space Layout Randomization (ASLR) opcode handler collision crash-loops. OPcache can be toggled in the UI extension dialog (explicitly setting `opcache.enable = 0` and `opcache.enable_cli = 0` when disabled).
  - **User-Defined `php.ini` Directives (`overrides.ini` & `conf.d/`):** Custom PHP configuration directives now survive automatic `php.ini` regenerations across restarts. Users can add custom settings to `<data_dir>/services/php/<version>/overrides.ini` (which OLS automatically scaffolds with common directives like `upload_max_filesize`, `post_max_size`, `memory_limit`, `max_execution_time`) or drop modular `.ini` snippets into `<version>/conf.d/`. These are automatically appended to the generated `php.ini` whenever runtimes start or apply.
  - Enable/disable extensions with a single toggle: `curl`, `gd`, `intl`, `mbstring`, `mysqli`, `pdo_mysql`, `pdo_pgsql`, `pdo_sqlite`, `redis`, `sodium`, `zip`, `opcache`.
  - **Xdebug Integration:** One-click toggle with pre-configured port `9003` for VS Code, PhpStorm, and Sublime Text.
- **Node.js & Package Managers:**
  - Install LTS or Current Node.js runtimes.
  - Built-in support for npm, yarn, pnpm, and bun.
- **Python:**
  - Python venv virtual environments isolated per project with pip package management.
  - Run arbitrary Python scripts or start FastAPI, Flask, and Django apps with process supervision.

---

## 9. Databases & Infrastructure Services

Manage all backend services from the **Databases** and **Services** pages:

- **Relational Databases:**
  - **MySQL / MariaDB:** Default port `3306`, user `root`, no password by default.
  - **PostgreSQL:** Default port `5432`, user `postgres`.
  - **SQLite:** Dedicated browser, visual manager, and file-based database support.
- **In-Memory Caches & NoSQL:**
  - **Redis:** Port `6379`.
  - **Memcached:** Port `11211`.
  - **MongoDB:** Port `27017`.
- **Mailpit (Email Testing):**
  - Catches all outgoing emails sent via PHP `mail()` or SMTP (`127.0.0.1:1025`).
  - View, inspect, and test emails via the Mailpit web UI at `http://127.0.0.1:8025`.
- **Database Explorers & Query Tools:**
  - Built-in integration with Adminer, phpMyAdmin, HeidiSQL, pgAdmin, and custom `.exe` database tools.
  - Direct database table listing and SQL query execution tools available in CLI (`olsc db tables`, `olsc db query`) and MCP (`ols_list_tables`, `ols_execute_sql`).
- **Dashboard Services Card Grouping (v2.5.0):**
  - Built-in and custom services are automatically organized into 5 canonical categories: **Web servers**, **Databases**, **Cache & queues**, **Mail**, and **Other**.
  - Collapsible section headers feature live running service counters, active green dots, rotating chevrons, and thin divider borders.
  - Collapsed state persists across restarts via backend settings (`dashboard.services.collapsed_groups`) and browser storage.
  - Direct, color-coded action buttons (start: emerald, stop: destructive, restart: amber, reload: sky, logs: violet) with layout-stable states.

---

## 10. Public Tunnels & Webhook Inspection

Share your local work with clients, remote teams, or external webhooks:

- **Tunnel Providers:**
  - **Cloudflare Tunnel:** Zero configuration required, generates secure HTTPS URL (`https://*.trycloudflare.com`).
  - **Ngrok:** Connect with your auth token.
  - **Localtunnel:** Free open-source alternative.
- **Traffic Inspector:**
  - Inspect inbound HTTP requests in real-time.
  - View headers, JSON payloads, and response status codes.
  - Replay requests or export them as **k6 load test scripts**.

---

## 11. Backups, Snapshots & Encrypted Site Bundles

Protect your work with built-in backup and export tools:

- **Snapshots:**
  - Create instant, point-in-time snapshots of site configuration, database dumps, and `.env` files before running updates.
- **Encrypted Site Bundles (`.olsbundle`):**
  - Export entire sites (code, settings, database dumps, `.env` files) into a portable package.
  - **Password Protection:** Sealed using **Argon2id** key derivation and **ChaCha20-Poly1305** authenticated encryption.
  - **Import & Clone:** Import bundles onto any computer with automatic conflict resolution and safety snapshots.

---

## 12. Monitoring, Logs & Background Tasks

- **Real-Time Logs:**
  - Unified viewer for Web server access logs, error logs, PHP-FPM logs, service journals, and application logs.
- **Process Supervisor:**
  - Monitor CPU and memory usage of running workers, database engines, and web servers.
- **Background Tasks:**
  - Track long-running database dumps, large site exports, and catalog sync operations with real-time step progress.

---

## 13. Diagnostics, Health & Doctor Auto-Repair

When something is not working, OLS diagnoses and fixes it:

- **Doctor (`olsc doctor`):**
  - Inspects port bindings, firewall rules, hosts entries, SSL certificates, runtime health, and service states.
  - Reports issues using the clear format: **Problem**, **Cause**, and **Fix**.
- **Auto-Repair:**
  - One-click automated resolution for port conflicts, broken configs, and missing certificates.
- **Consolidated Missing Folder Alerts & Deduplication (v2.5.0):**
  - Consolidates all missing directories and offline drives into a single collapsible summary card on the Dashboard, eliminating duplicate warning rows.
  - 1-click **Manage in Sites** action opens the Sites table with the `Folder not found` filter pre-selected.
  - 1-click **Ignore all** / **Unignore all** batch toggle to silence or restore alerts in bulk.
  - Expandable details list displays individual paths, offline drive status tags, and per-item ignore controls.
- **Intelligent Disconnected Drive Detection (v2.5.0):**
  - Inspects drive root prefixes (`C:\`, `D:\`, UNC shares) to distinguish missing project folders from unplugged storage drives.
  - Reports *"The drive for <name> is disconnected"* with instructions to reconnect the drive rather than claiming the project was deleted.
- **Support Bundle Generation:**
  - Exports a safe, redacted ZIP bundle containing sanitized diagnostics for bug reporting.

---

## 14. AI Assistant & Model Context Protocol (MCP)

OLS provides private, opt-in AI tools and native Model Context Protocol (MCP) support:

### Supported AI Providers
- **Local (100% Free & Private):**
  - **LM Studio:** `http://localhost:1234/v1`
  - **Ollama:** `http://localhost:11434/v1`
- **Cloud (Bring Your Own Key):**
  - **OpenRouter:** `https://openrouter.ai/api/v1`
  - **Hugging Face:** `https://router.huggingface.co/v1`
  - **Custom API:** Any OpenAI-compatible endpoint.

### Core Capabilities
- **Ask AI (Sidebar):** Ask any question about using OLS, configuring sites, troubleshooting, and best practices.
- **Explain & Fix:** Explains error messages and proposes safe, executable action plans.
- **Config & .htaccess Review:** Drafts and verifies web server rules and environment manifests.
- **Ask the Logs:** Analyzes recent log lines to pinpoint 500 errors or failed queries.
- **Commit Messages:** Drafts concise Git commit messages from staged changes.

### Model Context Protocol (MCP) Server (`ols-mcp`)
- Connect AI host applications (Claude Desktop, Cursor, VS Code, Goose) directly to OLS via stdio JSON-RPC 2.0.
- Exposes tools to inspect and mutate domains, restart web servers, apply SSL certificates, manage PHP/Node/Python runtimes, list database tables, execute SQL queries, and inspect logs.
- Supports bearer token authentication stored securely in the Windows Credential Manager.

---

## 15. Command Line Interface (CLI — `olsc`)

OLS includes the companion CLI tool `olsc` for terminal lovers:

```bash
# Check status of web server and services
olsc status

# Start / Stop / Restart
olsc start
olsc stop
olsc restart

# Run diagnostic health check
olsc doctor

# Auto-repair issues
olsc repair

# Setup environment from .openlocalserver/environment.yaml
olsc setup

# Ask the AI assistant about the app or an error
olsc ai ask "How do I add a custom domain and SSL?"

# Print or view the full user manual
olsc guide
```

---

## 16. Keyboard Shortcuts, Settings, Themes & Documentation

### Shortcuts
- **Ctrl + K / Ctrl + Shift + P:** Open Command Palette & Search.
- **F5:** Refresh current page while preserving active tabs and sub-panels.
- **Esc:** Close active dialog or modal.

### Privacy & System Security
- **No telemetry, no tracking, no analytics.**
- Secrets are stored in the secure Windows Credential Manager / OS keyring.
- Secrets (`.env` passwords, API keys) are automatically redacted before sending any prompt to AI providers.
- **Direct Win32 Administrator Elevation & PATH Independence (v2.5.0):**
  - Administrative operations (such as installing the background helper or modifying the hosts file) elevate directly via Win32 `ShellExecuteExW` with the `runas` verb, avoiding PowerShell and removing any dependency on system `PATH`.
  - System binaries (`powershell.exe`, `cmd.exe`) are resolved directly from `%SystemRoot%\System32` via `GetSystemDirectoryW`, preventing launcher environment corruption.

### Appearance & Theme Customization (v2.5.0)
- **Settings → Appearance:** Choose between System, Light, and Dark modes.
- **Curated Theme Presets:** 4 built-in presets: Default, High Contrast, Midnight Slate, and Warm Amber.
- **Contrast-Safe Color Engine:** Automatically computes WCAG 2.1 contrast ratios and readable foreground text colors.
- **Custom Accent Colors:** Pick curated swatches or enter any custom hex/CSS color with live contrast warning badges.
- **Icon Style:** Toggle between monochrome subtle icons or vibrant multi-color SVG gradient icons.
- **Background Styles:** Toggle solid colors or ambient gradients with a 360° direction angle slider and live preview strip.

### In-App Documentation & Modernized Release Notes Viewer (v2.5.0)
- **Settings → General → Documentation & Guides:** Toggle between the full in-app **User Manual** and **Release Notes**.
- **Real-Time Text Search:** Filter topics and sections instantly with query highlighting and clear buttons.
- **1-Click Copy Markdown:** Export raw Markdown to your clipboard with visual confirmation.
- **Redesigned Release Notes UI:** Inspired by Linear, Raycast, and GitHub Desktop, featuring dedicated release headers, version metadata badges, categorized feature cards with contextual Lucide icons, expandable technical details, and distraction-free typography.

---

## 17. Version 2.10.0 Specific Notes & What's New

### What's New in v2.10.0
- **Docker page:** a new **Docker** item under **Sites** in the sidebar shows whether Docker is running. When it is stopped, not installed or unreachable, one card explains why and offers **Start Docker**, **Refresh**, **Retry** and **View details**; there are no empty tabs. **Edit compose files without Docker** opens the Compose tab on its own (actions that need Docker say "Start Docker first"). Docker is optional: OLS does not install it, and nothing changes if you don't use it.
- **Containers:** search your containers, filter them, and see Compose projects as collapsible groups (open when something runs; **Collapse all / Expand all**). Start, stop, restart or pause a whole group at once, or each container on its own. Click a port to open it in your browser, **Logs** to read its output, or **Shell** to open a terminal inside a running container (bash when it has it, otherwise sh).
- **Container settings:** the gear button opens a container's details (copy its ID, see ports, volumes, networks and settings; passwords stay hidden). On **Settings** you can rename it and change its restart behaviour and memory or CPU limits without restarting it. On **Environment**, **Edit** changes its variables: OLS recreates the container (a few seconds of downtime), keeps its volumes, and restores the old one if anything fails. Hidden passwords stay unless you type a new value. For a Compose container, use **Edit compose file** and press Up instead. **Remove container** asks first and keeps your data volumes.
- **Images and Volumes:** list images and volumes with sizes and what uses them, pull and run images, remove images and unused volumes. **Reclaim space** shows what a cleanup would remove and how much space it frees before it does anything; unused volumes are off by default because deleting them deletes data. Search, filters and page controls keep long lists easy to use.
- **Compose:** the **Compose** tab lists compose files in your projects and stacks Docker runs. Use Up, Stop, Restart, Down (keeps data), Pull and Build, and watch the output live.
- **Edit and check compose files:** **Edit file** opens the built-in editor. **Check** points out mistakes and asks Docker too; **Save** backs up the old file first.
- **New compose file:** pick services (databases, caches, Mailpit, Adminer, PHP, Node and more). OLS picks free ports and keeps passwords in the Windows credential store; **Passwords** on the stack shows them.
- **Connect to OLS:** on a stack, **Connect to OLS** lets its containers open your OLS sites by name, trust OLS's HTTPS certificate, and use OLS's databases and Mailpit (the dialog shows each address). It applies the next time you press Up or Restart, and your compose file is not changed.
- **Your own presets:** in **New compose file**, **New preset** saves your own service (image, versions, ports, environment, volumes, command). It appears under **My presets**; edit or delete it there. **Save as preset** turns an image you typed into a preset. Variables marked **Secret** get a generated password stored in the Windows credential store.
- **Create site:** give any container a local domain with HTTPS. The site follows the container if its port changes, and turning the site off or on in Sites stops or starts the container.
- **AI assistant (MCP):** new tools for Docker containers, logs, Compose stacks, the compose builder and creating sites, plus one to bring back a hidden site.
- **Shorter sidebar:** Settings → **Sidebar** hides pages you don't use. Dashboard, Projects, Sites, Web server, Databases, Services, Runtimes and Settings can't be hidden. Hidden pages still open from search (Ctrl K) and from links.
- **No slowdown:** nothing Docker-related runs when OLS starts; the Docker page only talks to Docker while it is open, and its lists load from a cache and refresh in the background.

### Coming next
- **Command line for Docker:** manage containers, images, volumes and Compose stacks with `olsc docker`.
- **Warnings for Docker sites:** Diagnostics tells you when a site's container is stopped or missing.
- **Site bundles keep Docker details:** exports keep a site's Docker link and compose file, but never its passwords.
- **Dockerize a project:** generate a Dockerfile and compose file for a project in one click.

### Previous Version Notes (v2.9.0)
- **Import databases from several places at once:** your installed OLS MariaDB versions, other MySQL/MariaDB installs, or any data folder, in the order you choose.
- **Duplicate databases:** choose skip, replace (backed up first), or import under a new name. Interrupted imports are recovered on the next start.
- **Explorer menu:** right-click a folder → Show more options → Open Local Server to add, set up, or ignore a project. Turn it on in Settings.
- **Hide only a site:** an `.ols_ignore_site` file hides a project's site but keeps the project. Use **Unignore site** on Projects to bring it back.
- **More icons:** over 400 icons for sites and projects, in clear categories.
- **Easier to use:** a simpler Import databases window, per-mode backgrounds, Editor settings in their own section, and pages that no longer jump while loading.
- **Faster auto-fix:** safe problems are fixed right away when something changes, and Diagnose on the Repair page fixes what it safely can.
- **Fixes:** project commands use the right PHP version, a site's Repair page shows only its own problems, faster saving, Add Site picks the right folder, correct tray service count, and a more reliable Explorer menu.

### Previous Version Notes (v2.8.0)
- Improved project and site selection and Quick App search; added WordPress core repair, safer database maintenance, retained task history, and staged site health checks.

### Previous Version Notes (v2.6.0)
1. **Installed Software UI Overhaul:**
   - **Accurate Status Display**: Separated installation status from update availability. Accurately communicates "Up to date" (green indicator), "Update available" (sky indicator with non-link neutral text), and "Not installed".
   - **Multi-Version Tracking**: Shows total installed counts (e.g. "2 installed") without mistaking older uninstalled versions for pending updates. Displays the "Default" badge only where relevant.
   - **Standardized Action Column**: Every single row features a uniform right-aligned "Manage" button (`h-8`, `min-w-[90px]`, `text-xs`). When an update exists, the "Update to <version>" button renders directly to its left with identical margins.
   - **WCAG AA Contrast & Typography**: Secondary metadata ("X versions available", "Latest: X", "X installed") is elevated to 12px with 5.8:1 (light) and 9.2:1 (dark) contrast ratios.
   - **Consistent Cells & Compact Layout**: Uniform 4-column layout with em-dash ("—") fallbacks for unversioned tools. Compact fixed column widths reduce eye-travel on wide monitors, paired with subtle row hover highlights and consistent 56px row height.
   - **Filter Toolbar & "Update all" Action**: Added "All", "Updates available", and "Installed" filter tabs above the table, and a bulk "Update all" action with pre-flight major version confirmation.

2. **Semver Major Upgrade Protection:**
   - **Major Boundary Detection**: Upgrades crossing a major semver boundary (e.g. MongoDB 8.x to 9.0) display a "Major" warning badge and amber warning-styled button.
   - **Confirmation Dialog**: Clicking a major upgrade opens a confirmation modal detailing potential breaking changes, configuration deprecations, and data compatibility before downloading. Minor and patch updates remain one-click.

3. **Prerelease Filtering & Channel Selector:**
   - **Stable Default Updates**: Alpha, beta, rc, preview, canary, and dev builds are excluded from default update recommendations.
   - **Node.js LTS Targeting**: Node.js update recommendations strictly target the newest even-numbered LTS line (e.g. 22.x, 20.x).
   - **"Show pre-release versions" Toggle**: Added a channel toggle in the Manage dialog with distinct "Pre-release" badges and secondary button styling, never primary.

4. **Dynamic Upstream SHA-256 Sidecar Resolution:**
   - **Automatic Sidecar Verification**: Resolves vendor in-place archive rebuilds across all runtimes (Apache Lounge, GitHub assets, official mirrors). If an archive fails static catalog verification, OLS automatically queries the upstream provider's live sidecar or API feed. If verified, the checksum updates in-memory and extraction succeeds safely without compromising security.

5. **Scoped Process Supervisor Deletion Guards:**
   - **Process-Level Version Scoping**: Removing an idle or unselected runtime version (e.g. Mailpit 1.32.0 or PHP 8.3) is now permitted even when another version (e.g. Mailpit 1.31.3 or PHP 8.4) is actively running. Only active processes executing from that specific version directory block deletion.

6. **Registered Dynamic Runtime Binaries:**
   - Added executable mappings for Mailpit (`mailpit.exe`), Caddy (`caddy.exe`), SQLite (`sqlite3.exe`), Git (`cmd/git.exe`), and k6 (`k6.exe`), ensuring dynamic online installs are detected on disk.

7. **Port Checker Accuracy:**
   - Filtered PID 0 and TIME_WAIT sockets from netstat output, preventing in-use ports from falsely displaying as "System Idle Process". Added PowerShell `Get-NetTCPConnection` fallback for bound sockets and standard port hints (3306, 5432, 6379, 80, 443).

8. **Instant Persistent UI Caching:**
   - Synchronous 0ms page rendering backed by persistent storage across app restarts; instant optimistic updates for sites, databases, and runtimes without manual page refresh.

9. **Unified Notifications & State Banners:**
   - Centralized toasts with semantic tokens, smart pause/resume on blur/hover, deduplication, auto-resolving condition alerts (`Resolved` history state and automatic toast dismissal when a service recovers), startup grace period, live progress for background tasks and runtimes, and a persistent notification history dropdown.
   - Serialized per-service start/stop operations with port readiness verification and intent tracking so manual stops and slow starts never trigger false failure or crash alerts.
   - Persistent inline banners for active conditions (unapplied web server changes, port collisions); replaced noisy confirmation dialogs for reversible actions with instant Undo toasts.

10. **Dashboard Layout & Service Controls:**
    - Streamlined responsive two-column layout supporting wide content areas up to 1680px for maximized windows, eliminating side gutters.
    - Dedicated full-width Web Server status strip featuring inline configuration attributes and 1-click home page navigation.
    - Borderless service row action buttons with 18px icons, rounded hover tints, and WCAG-compliant ≥ 3:1 contrast tokens.
    - Independent per-service pending states with transitional status indicators, preventing other service rows from dimming while one service starts or stops.

11. **Unified External Database Tools Gallery & Visual Arguments Editor:**
    - **External Tools Gallery**: Consolidated database GUI explorer detection and management into `Databases > External tools`. Auto-detects HeidiSQL, DBeaver, Letos DB Browser, DB Browser for SQLite, pgAdmin 4, NoSQLBooster, and Tiny RDM on disk.
    - **Visual Argument Editor (`ToolArgsEditor`)**: Replaced raw space-delimited text boxes with individual argument inputs, eliminating brittle shell quoting and escaping issues.
    - **Multi-Engine & Per-Engine Arguments**: Allows specifying different launch flags and driver parameters per database engine (e.g. MariaDB, PostgreSQL, SQLite for DBeaver) with a simple mode toggle.
    - **Interactive Placeholder Chips & Command Preview**: One-click chips (`{host}`, `{port}`, `{database}`, `{path}`, `{uri}`) insert placeholders into active argument inputs, paired with a live "Runs as: program ..." command preview.
    - **JSON Import/Export**: Export tools to portable JSON (`schemaVersion: 1`) without machine-specific paths; import tools with drag-and-drop and duplicate conflict resolution (Skip, Overwrite, Rename).
    - **Reliable Windows Launching**: Launches tools with their install folder as working directory (allowing DLLs and plugins to resolve) and passes arguments verbatim to prevent Delphi/HeidiSQL host truncation.

---

### Previous Version Notes (v2.5.0)
1. **Consolidated Missing Folder Alerts & Batch Remediation:**
   - Consolidates all missing directories and offline drives into a single, clean collapsible summary card on the Dashboard.
   - Provides a 1-click **Manage in Sites** action that switches straight to the Sites table with the `Folder not found` filter pre-selected.
   - Provides a 1-click **Ignore all** / **Unignore all** batch toggle to silence or restore alerts in bulk.
   - Expandable details list displays individual paths, offline drive status tags, and per-item ignore controls.
2. **Intelligent Disconnected Drive Detection:**
   - Checks volume root prefixes (`C:\`, `D:\`, UNC shares) to distinguish missing project folders from unplugged storage drives.
   - When an external or network drive is offline, OLS reports *"The drive for <name> is disconnected"* with instructions to reconnect the drive rather than claiming the project was deleted.
3. **Physical Path Deduplication:**
   - Cross-references sites and registered projects by physical disk path, emitting exactly one unified alert per folder (e.g. `chatbot-ta.local (project chatbot-TA)`) and eliminating duplicate Error + Warning rows on the Dashboard.
4. **"Folder not found" Site Management Tab:**
   - Adds a dedicated filter tab (`Folder not found`) isolating sites whose project folder is missing (`status === 'missing'`) or drive is offline (`status === 'offline'`).
   - Ensures the "Enabled" and "Disabled" tabs represent strictly operational projects.
   - Provides a 1-click **Locate** action directly in the row to relocate projects to their new disk location.
5. **Right-Click Context Menu for Sites:**
   - Right-clicking any site row immediately opens the actions dropdown positioned at the mouse cursor.
   - Shares a single unified menu definition with the row's Actions button, keeping all actions 100% in sync.
6. **Smart Viewport Clamping & Internal Scrolling:**
   - Floating menus automatically calculate available viewport space, flip upward or leftward near edges, and enforce clearance below the 36px titlebar (`minTop = 44px`, `z-[95]`).
   - Constrains maximum dropdown height (`calc(100vh - 52px)`) with smooth internal scrolling (`overflow-y-auto`) for small windows and laptop displays, ignoring internal scroll events to prevent accidental closure.
7. **Live System Tray Tooltip Synchronization:**
   - Real-time tray icon tooltip (`OpenLocalServer — X of Y running`) updates immediately when sites or services start, stop, or change health status.
   - Thread-safe tooltip synchronization with cached string deduplication (`TOOLTIP_SHOWS`) decoupled from Win32 menu rebuilds.
8. **Vibrant Navigation & Aurora Themes:**
   - High-contrast, multi-color SVG gradient icons across sidebar and settings navigation.
   - Active neon glowing indicators and fluid Dark Aurora / Light Aurora ambient background meshes.
9. **Token-Based Appearance Customization & Themes:**
   - Dedicated `Settings → Appearance` tab with 4 curated presets (Default, High Contrast, Midnight Slate, Warm Amber).
   - Live OS theme synchronization and customizable dark/light modes.
   - Contrast-safe color engine with WCAG 2.1 contrast math and auto-readable foregrounds.
   - Custom accent color picker with swatches, presets, and hex inputs.
   - Monochrome vs colorful sidebar icon toggles.
   - Solid vs ambient gradient backgrounds with 360° angle sliders and preset directions.
   - Real-time interactive preview strip.
10. **Dashboard Services Card Grouping & Action States:**
    - Redesigned Services card with fluid responsive grid.
    - Clear, fixed ordering of all 9 built-in services organized across Web servers, Databases, Cache & queues, Mail, and Other.
    - Color-coded action buttons with subtle opacity (start: emerald, stop: destructive, restart: amber, reload: sky, logs: violet).
    - Flattened styling for currently unavailable actions to prevent layout shifts.
11. **Multi-Domain Project Grouping:**
    - Multiple hostnames sharing a physical project directory are unified into a single project row.
    - Primary domain leads with interactive pill badges and status dots for secondary/alias domains.
    - Direct 1-click modal navigation to any domain's settings directly from the pill badges.
    - Bulk enable/disable and delete operations apply cleanly across all domains in the project row.
12. **PHP 8.5+ Built-in OPcache & Windows ASLR Fix:**
    - Detects OPcache as built-in for PHP ≥ 8.5 without requiring `ext/php_opcache.dll`.
    - Automatically configures `opcache.file_cache` and `opcache.file_cache_fallback = 1` for Windows FastCGI workers, preventing Address Space Layout Randomization (ASLR) opcode handler collision crash-loops.
    - Exposes OPcache as a toggle in the UI extensions dialog; explicitly emits `opcache.enable = 0` and `opcache.enable_cli = 0` when toggled off.
13. **User-Defined `php.ini` Directives (`overrides.ini` & `conf.d/`):**
    - Custom PHP configuration directives now survive automatic `php.ini` regenerations across restarts.
    - Automatically creates an `overrides.ini` template in `<data_dir>/services/php/<version>/overrides.ini`.
    - Appends all custom settings from `overrides.ini` and any `.ini` files in `<version>/conf.d/` to the bottom of the generated `php.ini`.
14. **Direct Win32 Administrator Elevation & PATH Independence:**
    - Replaced PowerShell `Start-Process -Verb RunAs` elevation with direct Win32 `ShellExecuteExW` (`runas` verb), removing the dependency on `powershell.exe` on system PATH.
    - System executables (`powershell.exe`, `cmd.exe`) are now resolved directly from `%SystemRoot%\System32` via `GetSystemDirectoryW`, ensuring the built-in terminal and background tools run reliably even with missing or corrupted PATH environments.
15. **Modernized In-App Release Notes & Documentation Viewer:**
    - Completely overhauled the Release Notes interface in `Settings → General` with a native application design inspired by Linear, Raycast, and GitHub Desktop.
    - Added dedicated release header with version badge, release date, platform, license, and concise lead summary.
    - Replaced dense markdown lists with visually organized feature cards featuring topic-specific Lucide icons, short descriptions, and collapsible technical deep-dives.
    - Added real-time text search filter with result counts and 1-click raw Markdown copy.
