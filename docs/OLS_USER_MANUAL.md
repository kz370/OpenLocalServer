# OpenLocalServer (OLS) — Comprehensive User Manual & Operations Guide

> **Version:** 2.4.0  
> **Platform:** Windows 10 / Windows 11 (64-bit)  
> **License:** GPL-3.0-only  
> **CLI Tool:** `olsc`  
> **Application:** `OLS.exe`  
> **Repository:** [https://github.com/kz370/OpenLocalServer](https://github.com/kz370/OpenLocalServer)  

---

## Table of Contents

- [1. Executive Overview & System Architecture](#1-executive-overview--system-architecture)
- [2. Installation, First Run & Trusting Local CA](#2-installation-first-run--trusting-local-ca)
- [3. Sites & Virtual Host Management](#3-sites--virtual-host-management)
  - [3.1 Adding Sites: Static, Plain PHP, and Reverse Proxy](#31-adding-sites-static-plain-php-and-reverse-proxy)
  - [3.2 Automatic Folder & Welcome Page Initialization](#32-automatic-folder--welcome-page-initialization)
  - [3.3 Document Roots, Custom Domains & Aliases](#33-document-roots-custom-domains--aliases)
  - [3.4 Site Logos & Offline Font Awesome Icons](#34-site-logos--offline-font-awesome-icons)
- [4. Domains, Networking & Multi-Interface Binding](#4-domains-networking--multi-interface-binding)
  - [4.1 Local DNS & Zero-Config `.test` Domains](#41-local-dns--zero-config-test-domains)
  - [4.2 Multi-Interface Web Binding (`0.0.0.0`) & LAN Access](#42-multi-interface-web-binding-0000--lan-access)
  - [4.3 Dynamic DNS (Cloudflare Integration) & Router Port Forwarding](#43-dynamic-dns-cloudflare-integration--router-port-forwarding)
  - [4.4 Site Broadcast Controls & Public URL Copying](#44-site-broadcast-controls--public-url-copying)
- [5. Web Servers: Nginx, Apache & Caddy](#5-web-servers-nginx-apache--caddy)
  - [5.1 Live Web Server Switching](#51-live-web-server-switching)
  - [5.2 Configuration Editing & Syntax Validation](#52-configuration-editing--syntax-validation)
  - [5.3 Apache `.htaccess` Overrides & AI Review](#53-apache-htaccess-overrides--ai-review)
  - [5.4 Local Certificate Authority & SSL Termination](#54-local-certificate-authority--ssl-termination)
- [6. Quick Apps (1-Click Application Scaffolding)](#6-quick-apps-1-click-application-scaffolding)
  - [6.1 Supported Framework Recipes](#61-supported-framework-recipes)
  - [6.2 Creation Workflow & Automatic Environment Setup](#62-creation-workflow--automatic-environment-setup)
- [7. Commands, CLI & Developer Workflows](#7-commands-cli--developer-workflows)
  - [7.1 Artisan, Composer, npm, pip & WP-CLI](#71-artisan-composer-npm-pip--wp-cli)
  - [7.2 Integrated Terminal & Environment Variables](#72-integrated-terminal--environment-variables)
- [8. Runtimes, Extensions & Debugging](#8-runtimes-extensions--debugging)
  - [8.1 Isolated PHP Management (PHP 7.4 through 8.4)](#81-isolated-php-management-php-74-through-84)
  - [8.2 Extension Toggles & PECL Package Support](#82-extension-toggles--pecl-package-support)
  - [8.3 Xdebug 3 Step-by-Step Configuration](#83-xdebug-3-step-by-step-configuration)
  - [8.4 Node.js, Bun, and Python Virtual Environments](#84-nodejs-bun-and-python-virtual-environments)
- [9. Databases & Infrastructure Services](#9-databases--infrastructure-services)
  - [9.1 MySQL / MariaDB, PostgreSQL & SQLite](#91-mysql--mariadb-postgresql--sqlite)
  - [9.2 Redis, Memcached & MongoDB](#92-redis-memcached--mongodb)
  - [9.3 Mailpit Outgoing Email Catcher & Web Inspector](#93-mailpit-outgoing-email-catcher--web-inspector)
  - [9.4 External Database Explorers (HeidiSQL, Adminer, pgAdmin)](#94-external-database-explorers-heidisql-adminer-pgadmin)
- [10. Public Tunnels & Webhook Traffic Inspector](#10-public-tunnels--webhook-traffic-inspector)
  - [10.1 Cloudflare Tunnel, Ngrok & Localtunnel](#101-cloudflare-tunnel-ngrok--localtunnel)
  - [10.2 Real-Time Request Inspector & Replay](#102-real-time-request-inspector--replay)
  - [10.3 Generating k6 Load Testing Scripts](#103-generating-k6-load-testing-scripts)
- [11. Backups, Snapshots & Encrypted Site Bundles](#11-backups-snapshots--encrypted-site-bundles)
  - [11.1 Point-in-Time Site Snapshots](#111-point-in-time-site-snapshots)
  - [11.2 Argon2id + ChaCha20-Poly1305 Encrypted Bundles](#112-argon2id--chacha20-poly1305-encrypted-bundles)
  - [11.3 Restoring, Cloning & Updating Sites from Bundles](#113-restoring-cloning--updating-sites-from-bundles)
- [12. Monitoring, Live Logs & Process Supervision](#12-monitoring-live-logs--process-supervision)
  - [12.1 Real-Time Multi-Source Log Viewer](#121-real-time-multi-source-log-viewer)
  - [12.2 Background Task Registry & Progress Tracking](#122-background-task-registry--progress-tracking)
- [13. Diagnostics, Health Engine & Doctor Auto-Repair](#13-diagnostics-health-engine--doctor-auto-repair)
  - [13.1 Doctor Diagnostic Architecture (`Problem`, `Cause`, `Fix`)](#131-doctor-diagnostic-architecture-problem-cause-fix)
  - [13.2 Automated One-Click Repair](#132-automated-one-click-repair)
  - [13.3 Redacted Support Bundle Generation](#133-redacted-support-bundle-generation)
- [14. AI Assistant (Private, Opt-In, Bring-Your-Own-Model)](#14-ai-assistant-private-opt-in-bring-your-own-model)
  - [14.1 Supported Local Providers: LM Studio & Ollama](#141-supported-local-providers-lm-studio--ollama)
  - [14.2 Supported Cloud Providers: OpenRouter & Hugging Face](#142-supported-cloud-providers-openrouter--hugging-face)
  - [14.3 Asking the App Assistant & Safety Approvals](#143-asking-the-app-assistant--safety-approvals)
- [15. Command Line Interface Reference (`olsc`)](#15-command-line-interface-reference-olsc)
- [16. Keyboard Shortcuts, Settings & Privacy Guidelines](#16-keyboard-shortcuts-settings--privacy-guidelines)
- [17. What's New in Version 2.3.0](#17-whats-new-in-version-230)

---

## 1. Executive Overview & System Architecture

OpenLocalServer (OLS) is a unified, high-performance web development environment engineered exclusively for Microsoft Windows. Designed as a modern, lightweight successor to legacy WAMP, XAMPP, and cumbersome Docker setups, OLS runs web servers, runtimes, and databases natively on the host OS with zero virtual machine overhead and microsecond I/O performance.

```
+-------------------------------------------------------------------------+
|                          OpenLocalServer (OLS)                          |
+-------------------------------------------------------------------------+
|  User Interface (React 19 + Vite + Tailwind CSS + Tauri 2.0 Webview)   |
+------------------------------------+------------------------------------+
|  Control Channel (Named Pipes IPC) |  CLI Engine (`olsc`)               |
+------------------------------------+------------------------------------+
|                      Rust Core (`ols-core` crate)                       |
|  - Process Supervisor & Heartbeat  |  - Local DNS Engine (`*.test`)     |
|  - Certificate Authority & SSL     |  - RAG Knowledge Base & AI Client  |
|  - Dynamic DNS (Cloudflare Sync)   |  - AES/ChaCha20 Site Bundler       |
+------------------------------------+------------------------------------+
|  Web Servers (Nginx / Apache / Caddy) bound to 0.0.0.0 & 127.0.0.1      |
+------------------------------------+------------------------------------+
|  Runtimes: PHP 7.4-8.4 (FastCGI)   |  Node.js / Bun / Python (Proxied)  |
|  Databases: MySQL, Postgres, Redis |  Mail Service: Mailpit (Port 8025) |
+-------------------------------------------------------------------------+
```

### Architectural Guarantees
- **Native Windows Execution:** No WSL2 Linux filesystem translation latency; node_modules and Composer vendors operate at full NVMe disk speed.
- **Strict Privacy Mandate:** OLS emits **zero telemetry**, **zero analytics**, and **zero background tracking**. Network operations only occur when you explicitly ask to download a runtime, refresh a catalog, initialize a public tunnel, or query your configured AI provider.
- **Port Conflict Protection:** The core detects port collisions (IIS, Skype, other web servers) before startup and suggests automated fixes (`Diagnostic{problem, cause, fix}`).

---

## 2. Installation, First Run & Trusting Local CA

### System Requirements
- **Operating System:** Windows 10 (Build 19041+) or Windows 11 (64-bit).
- **Permissions:** Standard user privileges for daily operation. Administrator elevation is prompted strictly when binding system ports (80/443) or installing the local Certificate Authority into the Windows Certificate Store.

### First Run Checklist

#### Step 1: Install a PHP Runtime
1. Open **Runtimes** from the sidebar.
2. Under the **PHP** section, select your preferred version (e.g. PHP 8.2 or 8.3) and click **Install**.
3. OLS downloads the official binary distribution directly, verifies its SHA-256 hash, and sets up isolated configuration files (`php.ini`).

#### Step 2: Trust the Local Certificate Authority (One-Time Setup)
1. Navigate to **Web server** → **Certificates** tab.
2. Click **Trust Certificate Authority**.
3. Windows User Account Control (UAC) will ask for confirmation to import the root certificate into the `Trusted Root Certification Authorities` store.
4. After approval, all `.test` domains will automatically serve valid, trusted HTTPS with green lock icons in Chrome, Edge, Brave, and Firefox.

#### Step 3: Start Services
- Click **Start All** in the titlebar or run `olsc start` in PowerShell.

---

## 3. Sites & Virtual Host Management

### 3.1 Adding Sites: Static, Plain PHP, and Reverse Proxy
From the **Sites** page, click **Add Site / Link Project**:
- **Static Site:** Serves static assets (HTML, CSS, JavaScript, WebAssembly, images) directly through the active web server.
- **PHP Site:** Configures a dedicated virtual host upstreamed to the appropriate PHP-FPM fastcgi pool.
- **Reverse Proxy:** Forwards inbound HTTP/HTTPS traffic to an internal local port (e.g. `http://127.0.0.1:3000` for Next.js, `http://127.0.0.1:5173` for Vite, or `http://127.0.0.1:8000` for Python FastAPI).

### 3.2 Automatic Folder & Welcome Page Initialization
*Introduced in v2.3.0:*
- When entering a new project folder path that does not yet exist on disk, OLS automatically creates the directory tree (`std::fs::create_dir_all`).
- If no index file (`index.html`, `index.php`, `index.htm`) exists in the directory, OLS creates a welcome page:
  - For **Static Sites**: Generates an `index.html` file highlighting server status and environment links.
  - For **PHP Sites**: Generates an `index.php` welcome page with an interactive PHP Info toggle (`?phpinfo=1`), active SAPI details, and loaded extensions.

### 3.3 Document Roots, Custom Domains & Aliases
- **Custom Document Roots:** Specify subdirectories such as `/public` for Laravel and Symfony, `/dist` for Vite/React/Vue, or `/web` for Drupal.
- **Domain Aliases:** Add supplementary hostnames (e.g. `api.mysite.test`, `cdn.mysite.test`) that route directly to the same document root. Deleting an alias prompts a confirmation dialog to prevent accidental downtime.

### 3.4 Site Logos & Offline Font Awesome Icons
- Customize each site with a dedicated icon in the Sites list:
  - **Upload Photo:** Upload any PNG, JPEG, or WebP image. OLS automatically converts it client-side into an optimized WebP icon saved as `ols_site_icon.webp` in the project root.
  - **Built-in Font Awesome Icons:** Pick from hundreds of offline SVG icons and customize colors using hex codes or interactive color swatches.

---

## 4. Domains, Networking & Multi-Interface Binding

### 4.1 Local DNS & Zero-Config `.test` Domains
OLS embeds a lightweight internal DNS daemon that intercepts and resolves all queries ending in `.test` to `127.0.0.1`. You never have to manually edit the Windows `hosts` file (`C:\Windows\System32\drivers\etc\hosts`) for every new project.

### 4.2 Multi-Interface Web Binding (`0.0.0.0`) & LAN Access
*New in v2.3.0:*
- Web servers (Nginx, Apache, Caddy) bind to `0.0.0.0` (all IPv4 interfaces) in addition to `127.0.0.1`.
- **Testing on Mobile & Other PCs:** Connect your smartphone, tablet, or secondary computer to the same local Wi-Fi or LAN network. Open your mobile browser and enter your PC's local IP (e.g. `http://192.168.1.50`) to access your local development environment directly!
- **Host Header Routing Fix:** OLS correctly routes requests carrying local IP or external host headers to the intended virtual host rather than falling through to a 404 error.

### 4.3 Dynamic DNS (Cloudflare Integration) & Router Port Forwarding
*New in v2.3.0:*
- Synchronize your public IP address with your custom domain using Cloudflare DNS:
  1. Open the site in **Sites** → **Domains & Public Access**.
  2. Enter your Cloudflare API Token (with `Zone:DNS:Edit` permissions) and Zone ID.
  3. OLS tests the token, verifies the DNS A record, and periodically updates the IP when your ISP leases change.
  4. Automatic conflict resolution warns if CGNAT (Carrier-Grade NAT) or router firewall rules block incoming port 80/443 traffic.

### 4.4 Site Broadcast Controls & Public URL Copying
*New in v2.3.0:*
- Directly within the Site Domains panel, view the live broadcast status indicator.
- Start or stop live tunnels with one click, and copy the live public URL (`https://*.trycloudflare.com`) directly to your clipboard.

---

## 5. Web Servers: Nginx, Apache & Caddy

### 5.1 Live Web Server Switching
Switch between web servers instantly from the **Web server** page:
- **Nginx:** High-throughput, event-driven architecture. Ideal for microservices, reverse proxies, and high-concurrency static apps.
- **Apache (httpd):** Full `.htaccess` rewrite rule evaluation, `mod_rewrite`, and legacy CMS compatibility.
- **Caddy:** Modern, memory-safe Go web server with automatic HTTPS and streamlined configuration syntax.

Switching web servers automatically updates port bindings, stops the prior binary, and boots the new server in under 500ms without port collision.

### 5.2 Configuration Editing & Syntax Validation
- Edit primary web server configs directly in the CodeMirror editor.
- Click **Test Config** before applying: OLS executes `nginx -t`, `httpd -t`, or `caddy validate` in a sandbox before reloading the live server, preventing downtime from typos.

### 5.3 Apache `.htaccess` Overrides & AI Review
- Edit `.htaccess` files for any Apache-backed site directly in the Site Settings modal.
- Use the **Ask AI** button inside the editor to detect redirect loops, optimize cache headers, or draft complex rewrite rules with diff comparisons.

---

## 6. Quick Apps (1-Click Application Scaffolding)

The **Quick Apps** catalog allows instant creation of fully configured web applications:

| Category | Frameworks Supported | Automatic Configuration |
|---|---|---|
| **PHP** | WordPress, Laravel, Symfony, Plain PHP | Document root (`/public`), MySQL database, `.env` file, virtual host |
| **Node.js** | React (Vite), Vue (Vite), Next.js, Express API | Proxy port assignment, package installation (`npm install`), dev script |
| **Python** | Django, FastAPI, Flask | Python `venv` creation, pip requirements install, port mapping |
| **Custom** | Custom App Scaffolds | User-defined YAML recipes and scaffold hooks |

---

## 7. Commands, CLI & Developer Workflows

### 7.1 Artisan, Composer, npm, pip & WP-CLI
The **Commands** page lists all executable tools detected within your project folder:
- **Laravel:** `php artisan migrate`, `php artisan db:seed`, `php artisan route:list`, `php artisan cache:clear`.
- **Node.js:** `npm run dev`, `npm run build`, `npm test`.
- **WordPress:** `wp core update`, `wp plugin list`, `wp db export`.

### 7.2 Integrated Terminal & Environment Variables
- Open a project terminal directly from the UI or launch Windows Terminal / PowerShell.
- OLS automatically injects the active PHP version, Composer path, and project-specific `.env` variables into the terminal session's environment.

---

## 8. Runtimes, Extensions & Debugging

### 8.1 Isolated PHP Management (PHP 7.4 through 8.4)
OLS isolates multiple PHP versions concurrently. Site A can run on legacy PHP 7.4 while Site B runs on PHP 8.3 and Site C runs on PHP 8.4. Each pool runs under an independent fastcgi port.

### 8.2 Extension Toggles & PECL Package Support
- Enable or disable extensions instantly without editing `php.ini` manually:
  `curl`, `gd`, `intl`, `mbstring`, `mysqli`, `pdo_mysql`, `pdo_pgsql`, `pdo_sqlite`, `redis`, `sodium`, `zip`, `opcache`.
- Changes reload the PHP-FPM pool instantly.

### 8.3 Xdebug 3 Step-by-Step Configuration
1. Open **Runtimes** → **PHP** → select your PHP version → click **Xdebug**.
2. Toggle **Enable Xdebug** on.
3. Default port is set to `9003` (Xdebug 3 standard).
4. Configure your IDE (`launch.json` for VS Code):
```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Listen for Xdebug",
      "type": "php",
      "request": "launch",
      "port": 9003
    }
  ]
}
```
5. Set breakpoints in your PHP code and trigger requests in your browser.

---

## 9. Databases & Infrastructure Services

### 9.1 Relational & In-Memory Services
- **MySQL / MariaDB:** Default port `3306`, user `root`, blank password.
- **PostgreSQL:** Default port `5432`, user `postgres`.
- **SQLite:** Integrated visual database browser and schema inspector.
- **Redis:** Port `6379` (In-memory cache & queue broker).
- **Memcached:** Port `11211`.
- **MongoDB:** Port `27017`.

### 9.2 Mailpit Outgoing Email Catcher
- Mailpit runs automatically on port `1025` (SMTP) and catches all outgoing emails sent via PHP `mail()` or framework mailers.
- View and debug formatted emails, headers, HTML rendering, and spam score via the Mailpit Web UI at `http://127.0.0.1:8025`.

---

## 10. Public Tunnels & Webhook Traffic Inspector

### 10.1 Cloudflare Tunnel, Ngrok & Localtunnel
- Share local sites with external clients or test webhooks (e.g. Stripe, GitHub, Shopify):
  - **Cloudflare Tunnel:** Requires zero configuration and generates a secure public HTTPS URL (`https://*.trycloudflare.com`).
  - **Ngrok:** Add your auth token for persistent custom subdomains.
- First tunnel start prompts for user confirmation before establishing public connections.

### 10.2 Real-Time Request Inspector & Replay
- Inspect all inbound requests passing through the tunnel.
- View HTTP method, path, headers, request body JSON, and query parameters.
- Click **Replay** to re-send identical requests to your local web server for rapid webhook debugging.

---

## 11. Backups, Snapshots & Encrypted Site Bundles

### 11.1 Point-in-Time Site Snapshots
Create full site backups before updating dependencies or migrating databases. Snapshots capture the database SQL dump, `.env` file, and site configuration in a single snapshot archive.

### 11.2 Argon2id + ChaCha20-Poly1305 Encrypted Bundles
Export portable `.olsbundle` archives:
- **Security:** Passwords and database dumps are encrypted using **Argon2id** key derivation and **ChaCha20-Poly1305** authenticated encryption.
- **Portability:** Move entire sites between different Windows PCs with one-click restore and collision avoidance.

---

## 12. Monitoring, Live Logs & Process Supervision

- **Multi-Source Log Viewer:** Stream web server access logs, error logs, PHP error logs, and service output in real-time with regex filtering and log-level highlighting.
- **Background Task Registry:** Monitor long-running exports, database imports, and catalog downloads with live percentage bars and step indicators.

---

## 13. Diagnostics, Health Engine & Doctor Auto-Repair

- **Doctor (`olsc doctor`):** Evaluates system health across 40+ validation vectors (firewall rules, port bindings, runtime integrity, SSL certificate expiration, hosts file conflicts).
- **Format:** Always reports findings as:
  - **Problem:** What is wrong.
  - **Cause:** Why it happened.
  - **Fix:** Step-by-step instructions or one-click automatic resolution.
- **Support Bundle:** Exports a sanitized, redacted diagnostic zip for troubleshooting without leaking credentials.

---

## 14. AI Assistant (Private, Opt-In, Bring-Your-Own-Model)

OLS includes an AI assistant that follows strict security and privacy rules:
- **No Telemetry / No Proprietary Vendor Lock-in:** Connect to **LM Studio** (`http://localhost:1234/v1`) or **Ollama** (`http://localhost:11434/v1`) on your local machine for 100% free, private AI that never sends data outside your computer.
- **Cloud Models (Bring Your Own Key):** Supports **OpenRouter**, **Hugging Face**, or any custom OpenAI-compatible endpoint.
- **Pre-Prompt Secret Redaction:** API keys, database passwords, and private `.env` values are automatically stripped before generating prompts.
- **Safety Gate:** The AI proposes step-by-step executable actions, but **never executes anything without explicit user approval**.

---

## 15. Command Line Interface Reference (`olsc`)

The companion CLI binary `olsc` allows complete terminal control:

```bash
# Service & Server Lifecycle
olsc status                    # Check status of web servers and services
olsc start                     # Start web server and autostart services
olsc stop                      # Stop all servers, background workers, and services
olsc restart                   # Restart the environment

# Diagnostics & Health
olsc doctor                    # Run full diagnostic inspection
olsc repair                    # Auto-repair detected configuration issues

# Projects & Sites
olsc project list              # List all registered projects
olsc project start <name>      # Start project workers and services

# AI Assistant & Operations Manual
olsc ai ask "How do I setup SSL on custom domains?"   # Query the AI assistant
olsc guide                     # Display this complete User Manual in terminal
```

---

## 16. Keyboard Shortcuts, Settings & Privacy Guidelines

- **Ctrl + K / Ctrl + Shift + P:** Open the Global Command Palette & Quick Search.
- **F5:** Refresh the current page while preserving active tabs and sub-panels.
- **Esc:** Close active dialog, modal, or overlay.

---

## 17. What's New in Version 2.4.0

1. **Python & Virtual Environment Management:**
   - Automatic detection of system Python (3.7–3.15) and project-local virtual environments (`.venv`, `venv`).
   - Run FastAPI (Uvicorn), Django, Flask, Streamlit, and custom Python apps with managed supervisor processes.
2. **Model Context Protocol (MCP) Server:**
   - Built-in Stdio MCP server (`ols-mcp`) enabling AI coding tools to inspect and configure local domains, web servers, databases, and runtimes.
   - Secure OS keyring token authentication with configurable TTL/expiry.
3. **Database Inspection & SQL Execution:**
   - Multi-engine database table inspection and SQL execution tools for MariaDB/MySQL, PostgreSQL, and SQLite.
4. **Proxy Port Collision & Reserved Port Prevention:**
   - Real-time conflict detection preventing duplicate local proxy ports (`127.0.0.1`) and HTTP 502 Bad Gateway errors.
   - Rejection of core reserved application ports (DNS, Web, DB, Redis, Mailpit).
   - "Find free port" helper in Reverse Proxy mode.
5. **Dark Circular Icon Branding:**
   - Modern circular branding across application executable, Inno Setup installer, taskbar, system tray, and site favicons.
6. **In-App User Manual & Release Notes Viewer:**
   - Integrated manual and release notes tab in `Settings → User Manual`.
7. **Updates UI Feedback:**
   - Compact in-button loading spinner and clean SemVer comparison badges.
