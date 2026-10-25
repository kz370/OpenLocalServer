# OpenLocalServer (OLS) — Comprehensive User Manual & Operations Guide

> **Version:** 2.4.0  
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
14. [AI Assistant (Local & Cloud Bring-Your-Own-Model)](#14-ai-assistant-local--cloud-bring-your-own-model)
15. [Command Line Interface (CLI — `olsc`)](#15-command-line-interface-cli--olsc)
16. [Keyboard Shortcuts, Settings & Privacy](#16-keyboard-shortcuts-settings--privacy)
17. [Version 2.4.0 Specific Notes & What's New](#17-version-240-specific-notes--whats-new)

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

---

## 4. Domains, Networking & Remote Access

OLS provides enterprise-grade local and remote networking capabilities:

### Local DNS & Multi-Interface Web Binding (v2.3.0)
- **Local Resolution:** Resolves all `*.test` domains to `127.0.0.1` locally.
- **Multi-Interface Web Binding (`0.0.0.0`):**
  - In v2.3.0, Web servers bind to `0.0.0.0`, allowing mobile devices, tablets, and other computers on your local WiFi/LAN to access your dev sites via your LAN IP.
- **Host Header Routing Fix:**
  - Public and LAN host headers properly route to virtual hosts across all web servers without falling through to default 404 pages.

### Dynamic DNS (DDNS) & Site Broadcast Controls (v2.3.0)
- **Site Broadcast Controls:**
  - Directly from the Site Domains panel, toggle site broadcasting and copy public URLs with a single click.
- **Dynamic DNS (Cloudflare Sync):**
  - Synchronizes your custom domain with your live external IP address through Cloudflare DNS API.
  - Automatic collision resolution and router port forwarding guidance.

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
  - Supports multiple concurrent PHP versions (7.4, 8.0, 8.1, 8.2, 8.3, 8.4).
  - Enable/disable extensions with a single toggle: `curl`, `gd`, `intl`, `mbstring`, `mysqli`, `pdo_mysql`, `pdo_pgsql`, `pdo_sqlite`, `redis`, `sodium`, `zip`.
  - **Xdebug Integration:** One-click toggle with pre-configured port `9003` for VS Code, PhpStorm, and Sublime Text.
- **Node.js & Package Managers:**
  - Install LTS or Current Node.js runtimes.
  - Built-in support for npm, yarn, pnpm, and bun.
- **Python:**
  - Python venv virtual environments isolated per project with pip package management.

---

## 9. Databases & Infrastructure Services

Manage all backend services from the **Databases** and **Services** pages:

- **Relational Databases:**
  - **MySQL / MariaDB:** Default port `3306`, user `root`, no password by default.
  - **PostgreSQL:** Default port `5432`, user `postgres`.
  - **SQLite:** Dedicated browser and visual manager.
- **In-Memory Caches & NoSQL:**
  - **Redis:** Port `6379`.
  - **Memcached:** Port `11211`.
  - **MongoDB:** Port `27017`.
- **Mailpit (Email Testing):**
  - Catches all outgoing emails sent via PHP `mail()` or SMTP (`127.0.0.1:1025`).
  - View, inspect, and test emails via the Mailpit web UI at `http://127.0.0.1:8025`.
- **Database Explorers:**
  - Built-in integration with Adminer, phpMyAdmin, HeidiSQL, pgAdmin, and custom `.exe` database tools.

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
- **Support Bundle Generation:**
  - Exports a safe, redacted ZIP bundle containing sanitized diagnostics for bug reporting.

---

## 14. AI Assistant (Local & Cloud Bring-Your-Own-Model)

The built-in AI Assistant is completely private and opt-in:

### Supported Providers
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

## 16. Keyboard Shortcuts, Settings & Privacy

### Shortcuts
- **Ctrl + K / Ctrl + Shift + P:** Open Command Palette & Search.
- **F5:** Refresh current page while preserving active tabs and sub-panels.
- **Esc:** Close active dialog or modal.

### Privacy Guarantee
- **No telemetry, no tracking, no analytics.**
- Secrets are stored in the secure Windows Credential Manager / OS keyring.
- Secrets (`.env` passwords, API keys) are automatically redacted before sending any prompt to AI providers.

---

## 17. Version 2.4.0 Specific Notes & What's New

### New Features in v2.4.0
1. **Python & Virtual Environment Support:**
   - Detects system Python installations (3.7–3.15) and project-local virtual environments (`.venv`, `venv`).
   - Supports direct execution of FastAPI (Uvicorn), Django, Flask, Streamlit, and custom Python scripts with supervisor process management.
2. **Model Context Protocol (MCP) Server:**
   - Built-in Stdio MCP server (`ols-mcp`) enabling AI assistants (Cursor, Claude Desktop, Antigravity) to manage domains, web servers, databases, and runtimes.
   - Secure token authentication stored in Windows Credential Manager with configurable expiration.
3. **Database Inspection & SQL Execution:**
   - Multi-engine database table listing and direct SQL execution tools across MariaDB/MySQL, PostgreSQL, and SQLite.
4. **Proxy Port Collision & Reserved Port Validation:**
   - Real-time detection and prevention of upstream port collisions on `127.0.0.1`.
   - Prevents assignment of ports reserved for core services (web servers, databases, Redis, Mailpit, DNS).
   - "Find free port" helper added to Reverse Proxy mode to locate unused loopback ports.
5. **Dark Circular Icon Branding:**
   - High-resolution circular branding across application executables, Inno Setup installer, taskbar, system tray, and site favicons.
6. **In-App User Manual & Release Notes Viewer:**
   - Dedicated documentation and release notes reader integrated directly in `Settings → User Manual`.
7. **Updates UI Enhancements:**
   - Compact in-button loading indicator with immediate SemVer comparison feedback.
