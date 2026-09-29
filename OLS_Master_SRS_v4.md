# OLS --- Master Software Requirements Specification

**Document Version:** 4.0\
**Status:** Final Consolidated Master SRS\
**Product:** OLS\
**Product Type:** Free, open-source, cross-platform local development
environment platform\
**Target Platforms:** Windows, macOS, Linux\
**Recommended Desktop Stack:** Tauri 2 + Rust + TypeScript +
React/Vue/Svelte + SQLite

------------------------------------------------------------------------

# 1. Executive Summary

OLS is a free and open-source desktop development environment
manager inspired by tools such as XAMPP and Laragon, but designed as a
much broader and more extensible platform.

OLS provides one place to install, configure, run, diagnose, and
reproduce complete local development environments.

It manages:

-   PHP and multiple PHP versions
-   Per-project PHP versions
-   PHP extensions
-   Xdebug
-   Composer
-   Node.js and multiple Node versions
-   npm, pnpm, and yarn
-   Python and multiple Python versions
-   Python virtual environments
-   Nginx
-   Apache
-   Caddy
-   MySQL
-   MariaDB
-   PostgreSQL
-   MongoDB
-   SQLite
-   Redis
-   Mailpit
-   Custom services
-   Local domains
-   Subdomains
-   Wildcard domains
-   Local DNS
-   Hosts-file management
-   Local Certificate Authority
-   Trusted local HTTPS
-   Wildcard certificates
-   Reverse proxy
-   Editable web-server configuration
-   Site enable/disable workflows
-   Public internet tunneling
-   Queue workers
-   Schedulers
-   Environment variables
-   Project manifests
-   Environment lock files
-   Quick Apps
-   Quick Commands
-   Custom command catalogs
-   External editors such as Notepad++
-   Git
-   Project diagnostics
-   Automatic repair
-   Logs
-   Process supervision
-   Snapshots and backups
-   CLI automation
-   Plugins
-   Runtime/package catalogs

The central product principle is:

> **A project declares the environment it needs, and OLS makes that
> environment available locally.**

------------------------------------------------------------------------

# 2. Vision

OLS should make this workflow simple:

``` text
Clone Project
      ↓
Open in OLS
      ↓
Detect Requirements
      ↓
Resolve Environment
      ↓
Install Missing Components
      ↓
Configure Services
      ↓
Create Database
      ↓
Create Domain
      ↓
Create HTTPS Certificate
      ↓
Configure Web Server
      ↓
Configure Mailpit
      ↓
Start Environment
      ↓
Run Diagnostics
      ↓
Open Application
      ↓
Optionally Start Public Tunnel
```

A developer should be able to go from a newly cloned project to a
working development environment without manually assembling dozens of
tools.

Advanced developers must still be able to control every important part
manually.

------------------------------------------------------------------------

# 3. Product Goals

OLS shall:

1.  Be free to use.
2.  Be open source.
3.  Support Windows, macOS, and Linux.
4.  Manage multiple versions of programming runtimes.
5.  Allow runtime versions to be assigned per project.
6.  Manage multiple versions of databases where practical.
7.  Support PHP, Node.js, Python, and future runtimes.
8.  Support MySQL, MariaDB, PostgreSQL, MongoDB, SQLite, Redis, and
    future services.
9.  Support Nginx, Apache, and Caddy.
10. Provide editable web-server configurations.
11. Provide site enable/disable workflows.
12. Provide local domains and subdomains.
13. Provide wildcard domains.
14. Provide local DNS and hosts-file management.
15. Provide trusted local HTTPS.
16. Provide a local Certificate Authority.
17. Provide wildcard certificates.
18. Provide reverse-proxy management.
19. Provide public internet tunneling.
20. Provide local email testing through Mailpit.
21. Provide Quick Apps.
22. Provide Quick Commands.
23. Provide an editable Quick App/Command catalog.
24. Provide external-editor integration, including Notepad++ detection.
25. Provide automatic project requirement detection.
26. Provide reproducible environment manifests.
27. Provide environment lock files.
28. Provide process supervision and crash recovery.
29. Provide diagnostics and automatic repair.
30. Provide CLI automation.
31. Provide plugin extensibility.
32. Protect users from configuration mistakes.
33. Avoid permanently running the main application with
    administrator/root privileges.

------------------------------------------------------------------------

# 4. Non-Goals

OLS is not intended to:

-   Replace a full IDE.
-   Replace Git.
-   Replace production hosting.
-   Become a production CDN.
-   Become a general-purpose VPN.
-   Replace Kubernetes.
-   Replace every possible container platform.
-   Automatically deploy production infrastructure.
-   Bundle every runtime and service into the initial installer.
-   Operate a mandatory proprietary public tunnel network.

Tunneling is intended primarily for:

-   Development
-   Testing
-   Webhooks
-   OAuth callbacks
-   Payment callbacks
-   Remote QA
-   Temporary demos
-   Mobile testing

------------------------------------------------------------------------

# 5. Target Users

## 5.1 PHP Developers

-   Laravel
-   Symfony
-   WordPress
-   Drupal
-   Magento
-   CodeIgniter
-   Custom PHP

## 5.2 JavaScript/TypeScript Developers

-   Node.js
-   React
-   Vue
-   Angular
-   Next.js
-   Nuxt
-   NestJS
-   Express
-   Vite

## 5.3 Python Developers

-   Django
-   Flask
-   FastAPI
-   Custom Python applications

## 5.4 General Developers

-   Go
-   Ruby
-   Java
-   Static sites
-   APIs
-   Microservices
-   Custom applications

------------------------------------------------------------------------

# 6. Recommended Technology Stack

## Desktop

Tauri 2

## Backend

Rust

## Frontend

TypeScript with React, Vue, or Svelte.

## Metadata Database

SQLite

## Configuration

-   YAML
-   JSON
-   Native external-service configuration files

## Initial Managed Components

``` text
PHP
Node.js
Python

Nginx
Apache
Caddy

MySQL
MariaDB
PostgreSQL
MongoDB
SQLite
Redis

Mailpit

Composer
npm
pnpm
yarn
Git
Xdebug
```

------------------------------------------------------------------------

# 7. Architecture

``` text
                         OLS
                            │
               ┌────────────┴────────────┐
               │                         │
              GUI                       CLI
               │                         │
               └────────────┬────────────┘
                            │
                            ▼
                   Application Core
                            │
      ┌─────────────────────┼─────────────────────┐
      │                     │                     │
   Projects             Runtimes              Services
      │                     │                     │
   Quick Apps           PHP/Node/Python      Databases
      │                     │                     │
   Commands             Extensions           Mailpit
      │                     │                     │
   Web Config          Environment           Workers
      │                     │                     │
   Domains              Resolver              Scheduler
      │                     │                     │
   DNS/SSL              Packages              Processes
      │                     │                     │
   Tunnels              Plugins              Diagnostics
      └─────────────────────┼─────────────────────┘
                            │
                     Platform Layer
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
           Windows        macOS         Linux
```

------------------------------------------------------------------------

# 8. Core Architecture Principles

## 8.1 Shared Application Core

The GUI, CLI, and future local API must use the same application core.

``` text
GUI ──┐
CLI ──┼──> Application Core
API ──┘
```

## 8.2 No Direct Process Control From UI

The frontend must never directly execute arbitrary system commands.

Correct:

``` text
UI
 ↓
Application Service
 ↓
Command/Process Manager
 ↓
Managed Process
```

## 8.3 Declarative Configuration

Projects, runtimes, services, domains, certificates, tunnels, and Quick
Apps should be represented declaratively wherever practical.

## 8.4 Platform Abstraction

OS-specific behavior must be isolated behind platform interfaces.

## 8.5 Privilege Separation

The main application must not permanently run elevated.

Sensitive operations should use a dedicated privileged helper.

------------------------------------------------------------------------

# 9. Runtime Management

Runtime management is a core OLS feature.

Supported initial runtimes:

``` text
PHP
Node.js
Python
```

Future runtimes:

``` text
Go
Ruby
Java
Bun
.NET
Other runtimes
```

------------------------------------------------------------------------

# 10. PHP Manager

Features:

-   Multiple PHP versions
-   Per-project PHP versions
-   Global PHP version
-   PHP executable management
-   php.ini management
-   Extension management
-   Xdebug
-   Composer
-   Compatibility checks
-   Requirement detection

Example:

``` text
PHP Versions

8.1
8.2
8.3
8.4
8.5
```

------------------------------------------------------------------------

# 11. Per-Project PHP Version

This is mandatory.

Example:

``` yaml
runtime:
  php: "8.1"
```

Another project:

``` yaml
runtime:
  php: "8.4"
```

Both must be able to coexist.

Global PHP settings must not silently override an explicitly configured
project version.

------------------------------------------------------------------------

# 12. PHP Extensions

The extension manager should display:

``` text
PHP 8.4

✓ curl
✓ mbstring
✓ openssl
✓ PDO
✓ pdo_mysql
✓ pdo_pgsql
✓ zip
○ imagick
○ redis
○ xdebug
```

Features:

-   Install
-   Enable
-   Disable
-   Remove
-   Verify
-   Detect missing extensions

Extensions must be associated with the correct PHP version.

------------------------------------------------------------------------

# 13. Xdebug

Support:

-   Enable/disable
-   Debug mode
-   Develop mode
-   Port
-   Client host
-   IDE configuration
-   Project-specific settings

------------------------------------------------------------------------

# 14. Composer

Support:

``` text
composer install
composer update
composer require
composer remove
composer dump-autoload
```

Composer should be associated with the selected PHP environment.

------------------------------------------------------------------------

# 15. Node.js Manager

Support:

-   Multiple Node versions
-   Per-project Node versions
-   Global default
-   npm
-   pnpm
-   yarn
-   Version detection
-   `package.json` requirement detection
-   Package-manager detection

------------------------------------------------------------------------

# 16. Python Manager

Support:

-   Multiple Python versions
-   Global version
-   Per-project version
-   pip
-   Virtual environments
-   Requirement detection
-   Python executable discovery

------------------------------------------------------------------------

# 17. Python Virtual Environments

Support:

``` text
project/
└── .venv/
```

Features:

-   Create
-   Detect
-   Activate
-   Recreate
-   Associate with project
-   Install dependencies

------------------------------------------------------------------------

# 18. Runtime Resolution

Resolution order:

``` text
Project-specific version
        ↓
Profile/environment version
        ↓
Global default
```

Example:

``` text
Global:
PHP 8.4

Project A:
PHP 8.1

Project B:
PHP 8.4

Project C:
PHP 8.3
```

------------------------------------------------------------------------

# 19. Runtime-Aware Terminal

A terminal opened from an OLS project must expose the project's
selected runtime.

Example:

``` text
Project A
PHP 8.1
Node 20
```

Running:

``` text
php -v
```

must use the project-selected PHP.

------------------------------------------------------------------------

# 20. Runtime Package Catalog

Runtime packages must use manifests.

Example:

``` json
{
  "type": "runtime",
  "name": "php",
  "version": "8.4.12",
  "platform": "windows",
  "architecture": "x64",
  "download": {
    "url": "...",
    "sha256": "..."
  },
  "binary": "php.exe"
}
```

The catalog must support:

-   Version
-   Platform
-   Architecture
-   Download
-   Hash
-   Signature
-   Dependencies
-   Installation layout
-   Executables
-   Configuration templates

------------------------------------------------------------------------

# 21. Package Integrity

Downloaded software must be verified before execution.

Minimum:

-   HTTPS
-   SHA-256

Where available:

-   Digital signatures

Failed verification must abort installation.

------------------------------------------------------------------------

# 22. Web Server Manager

Initial servers:

-   Nginx
-   Apache
-   Caddy

Each should support:

-   Installation
-   Multiple versions where practical
-   Start
-   Stop
-   Restart
-   Status
-   Logs
-   Health checks
-   Port management
-   Configuration
-   Validation
-   TLS
-   Project association

------------------------------------------------------------------------

# 23. Web-Server Site Configuration

OLS must expose project-specific web-server configuration as a
first-class feature.

For Nginx:

``` text
server blocks
upstreams
locations
FastCGI
proxy settings
TLS
redirects
headers
includes
```

For Apache:

``` text
VirtualHosts
Directory directives
Rewrite rules
Proxy settings
TLS
Modules
Includes
```

For Caddy:

``` text
Caddyfile
site blocks
reverse proxy
TLS
headers
routing
```

------------------------------------------------------------------------

# 24. Site Enable / Disable

OLS should provide a GUI equivalent to traditional
site-enable/site-disable workflows.

Example:

``` text
Sites

shop.test
    ● Enabled

api.shop.test
    ● Enabled

legacy.test
    ○ Disabled
```

Actions:

-   Enable
-   Disable
-   Edit
-   Validate
-   Reload
-   Duplicate
-   Delete
-   Open configuration directory

------------------------------------------------------------------------

# 25. Editable Web Configuration

Users must be able to directly edit generated configuration.

Editor capabilities:

-   Syntax highlighting
-   Search
-   Replace
-   Save
-   Revert
-   Diff
-   Validate
-   Reload
-   Backup
-   Restore
-   Open containing folder

------------------------------------------------------------------------

# 26. Generated vs Manual Configuration

OLS must track whether a configuration is:

``` text
Managed
Advanced
Manual
```

### Managed

OLS owns the configuration.

### Advanced

OLS generates the base configuration but permits manual changes.

### Manual

The user owns the configuration and OLS must not overwrite it
without explicit confirmation.

------------------------------------------------------------------------

# 27. Configuration Protection

If the user has manually changed a generated configuration:

``` text
This configuration contains manual changes.

Regenerating it may overwrite those changes.

[View Diff]
[Regenerate]
[Cancel]
```

OLS must not silently destroy manual configuration.

------------------------------------------------------------------------

# 28. Web Configuration Validation

Before applying a changed configuration:

``` text
Edit
 ↓
Validate
 ↓
If valid:
    Backup
    Apply
    Reload
    Health check

If invalid:
    Keep existing configuration
    Display error
```

Nginx should use its configuration test mechanism.

Apache should use its configuration test mechanism.

Caddy should use its native validation mechanism.

------------------------------------------------------------------------

# 29. Configuration History

OLS should optionally retain configuration versions.

Example:

``` text
Nginx History

19:40
Changed proxy target

18:21
Enabled HTTPS

Yesterday
Added api.shop.test
```

Users may:

-   Compare
-   Restore
-   Export

------------------------------------------------------------------------

# 30. Reverse Proxy

Support mappings such as:

``` text
api.shop.test
    ↓
127.0.0.1:8000

frontend.shop.test
    ↓
127.0.0.1:5173

admin.shop.test
    ↓
127.0.0.1:3000
```

The GUI should support common proxy configurations without preventing
advanced manual editing.

------------------------------------------------------------------------

# 31. Database Manager

Initial database/service support:

-   MySQL
-   MariaDB
-   PostgreSQL
-   MongoDB
-   SQLite
-   Redis

------------------------------------------------------------------------

# 32. MySQL

Support:

-   Multiple versions
-   Install
-   Remove
-   Start
-   Stop
-   Restart
-   Database creation
-   User creation
-   Backup
-   Restore
-   Logs
-   Health checks
-   Project association

------------------------------------------------------------------------

# 33. MariaDB

Support:

-   Multiple versions where practical
-   Installation
-   Service management
-   Database creation
-   Users
-   Backup
-   Restore
-   Health checks

------------------------------------------------------------------------

# 34. PostgreSQL

Support:

-   Multiple versions
-   Installation
-   Service management
-   Database creation
-   Roles/users
-   Backup
-   Restore
-   Health checks
-   Project association

------------------------------------------------------------------------

# 35. MongoDB

MongoDB is a first-class database.

Features:

-   Multiple versions where practical
-   Install
-   Remove
-   Start
-   Stop
-   Restart
-   Connection information
-   Logs
-   Health checks
-   Project association

------------------------------------------------------------------------

# 36. SQLite

SQLite requires no database server process.

Support:

-   Create
-   Detect
-   Associate
-   Show path
-   Backup
-   Restore
-   Integrity check
-   Open database

------------------------------------------------------------------------

# 37. Redis

Support:

-   Multiple versions where practical
-   Install
-   Start
-   Stop
-   Restart
-   Port management
-   Health checks
-   Logs
-   Project association

------------------------------------------------------------------------

# 38. Database Version Isolation

Projects may use different versions.

Example:

``` text
Project A → MySQL 8.0
Project B → MySQL 8.4
Project C → PostgreSQL 17
Project D → MongoDB 8
```

If instances cannot safely share a port/data directory, OLS must
create separate instances.

------------------------------------------------------------------------

# 39. Database Configuration

Example:

``` yaml
database:
  engine: mysql
  version: "8.4"
  host: 127.0.0.1
  port: 3306
  database: shop
  username: shop
```

Secrets must be securely stored.

------------------------------------------------------------------------

# 40. Project Manager

Projects are the primary environment unit.

A project can define:

``` text
Runtime
Web server
Database
Services
Domain
Subdomains
HTTPS
Environment variables
Workers
Scheduler
Mail
Tunnel
Quick Commands
```

------------------------------------------------------------------------

# 41. Project Creation Wizard

Fields should include:

``` text
Project name
Path
Project type
PHP version
Node version
Python version
Web server
Database engine
Database version
Redis
Mailpit
Domain
HTTPS
Wildcard
Queue
Scheduler
Public tunnel
```

------------------------------------------------------------------------

# 42. Framework Detection

Detect:

``` text
composer.json
package.json
artisan
symfony.lock
wp-config.php
manage.py
pyproject.toml
requirements.txt
go.mod
```

Initial framework detection:

-   Laravel
-   Symfony
-   WordPress
-   Generic PHP
-   Node.js
-   Django
-   Flask
-   FastAPI
-   Generic Python

------------------------------------------------------------------------

# 43. Automatic Requirement Detection

Detect:

### PHP

-   PHP version
-   Composer requirements
-   PHP extensions

### Node

-   Node version
-   npm/pnpm/yarn
-   package manager
-   package requirements

### Python

-   Python version
-   pyproject.toml
-   requirements.txt
-   virtual environment

### Databases

-   `.env`
-   framework configuration
-   project manifests

------------------------------------------------------------------------

# 44. Local Domain Management

Support:

-   Domains
-   Subdomains
-   Wildcards
-   Project mappings
-   Port mappings
-   Reverse proxy mappings
-   Hosts-file entries
-   Local DNS
-   Conflict detection

Recommended default:

``` text
project.test
```

------------------------------------------------------------------------

# 45. Subdomains

Example:

``` text
shop.test
api.shop.test
admin.shop.test
dashboard.shop.test
```

Each may have independent routing.

------------------------------------------------------------------------

# 46. Wildcard Domains

Support:

``` text
*.shop.test
```

Example:

``` text
tenant1.shop.test
tenant2.shop.test
api.shop.test
admin.shop.test
```

------------------------------------------------------------------------

# 47. Local DNS

Simple mode:

``` text
127.0.0.1 shop.test
127.0.0.1 api.shop.test
```

Advanced mode:

-   Local DNS resolver
-   Dynamic OLS domains
-   Wildcard resolution

------------------------------------------------------------------------

# 48. Domain Templates

Support:

``` text
{project}.test
api.{project}.test
admin.{project}.test
```

For `shop`:

``` text
shop.test
api.shop.test
admin.shop.test
```

------------------------------------------------------------------------

# 49. Local HTTPS

HTTPS is a core feature.

Example:

``` text
https://shop.test
https://api.shop.test
https://admin.shop.test
```

------------------------------------------------------------------------

# 50. Local Certificate Authority

OLS should provide a local development CA.

``` text
OpenLocalServer Local CA
       │
       ├── shop.test
       ├── *.shop.test
       ├── api.test
       └── *.api.test
```

The CA should be trusted through the supported OS trust mechanism after
user authorization.

------------------------------------------------------------------------

# 51. Certificate Manager

Support:

-   Generate
-   Renew
-   Revoke
-   Regenerate
-   Expiration checks
-   Trust checks
-   TLS health checks
-   Domain certificates
-   Subdomain certificates
-   Wildcard certificates
-   Project association

Display:

``` text
Issuer
Domains
Issue date
Expiration
Trust
Project
Certificate path
Key path
```

------------------------------------------------------------------------

# 52. HTTP to HTTPS

Projects should optionally enforce:

``` text
HTTP → HTTPS
```

The user must be able to disable this for projects that require plain
HTTP.

------------------------------------------------------------------------

# 53. HTTPS Health Checks

Validate:

``` text
DNS
 ↓
TCP
 ↓
TLS
 ↓
Certificate
 ↓
Trust
 ↓
HTTP
```

------------------------------------------------------------------------

# 54. Public Internet Tunneling

Tunneling is a first-class feature.

Example:

``` text
https://shop.test
       ↓
OLS Tunnel
       ↓
Public Internet
       ↓
https://public-example.example
```

Use cases:

-   Webhooks
-   OAuth callbacks
-   Payment callbacks
-   Mobile testing
-   Remote QA
-   Demos
-   Temporary previews
-   External API integrations

------------------------------------------------------------------------

# 55. Tunnel Provider Architecture

OLS must use a provider abstraction.

Potential adapters may support services such as:

-   Cloudflare Tunnel
-   ngrok
-   LocalTunnel
-   Tailscale Funnel
-   Other compatible providers

Provider availability and features may vary.

------------------------------------------------------------------------

# 56. Tunnel Provider Interface

Conceptually:

``` text
TunnelProvider

authenticate()
createTunnel()
startTunnel()
stopTunnel()
getStatus()
getPublicUrl()
getLogs()
```

------------------------------------------------------------------------

# 57. Tunnel Configuration

Example:

``` yaml
tunnel:
  enabled: true
  provider: cloudflare
  target: https://shop.test
```

Or:

``` yaml
tunnel:
  enabled: true
  provider: ngrok
  target: http://127.0.0.1:8000
```

------------------------------------------------------------------------

# 58. Tunnel UI

Example:

``` text
Public Tunnel

Status:
● Connected

Provider:
Cloudflare

Local Target:
https://shop.test

Public URL:
https://public-example.example

[Start]
[Stop]
[Copy URL]
[Open]
[Logs]
```

------------------------------------------------------------------------

# 59. Tunnel Security

OLS must:

-   Clearly show when a project is public.
-   Show the public URL.
-   Provide a prominent stop action.
-   Never silently start a public tunnel.
-   Warn before first exposure.
-   Never expose databases by default.
-   Never expose internal admin tools by default.
-   Redact tunnel credentials from logs.
-   Support provider authentication.
-   Provide optional access controls when supported.

Default:

``` yaml
tunnel:
  autostart: false
```

------------------------------------------------------------------------

# 60. Tunnel Health

Display:

``` text
Tunnel:
✓ Connected

Public URL:
https://public-example.example

Target:
https://shop.test

Latency:
...

Last request:
...
```

------------------------------------------------------------------------

# 61. Mail Development --- Mailpit

Mailpit is a mandatory first-class OLS service.

OLS should manage Mailpit like other local services.

------------------------------------------------------------------------

# 62. Mailpit Service Management

Support:

-   Install
-   Start
-   Stop
-   Restart
-   Status
-   Port configuration
-   Logs
-   Health checks
-   Open web interface
-   Project association

Typical defaults:

``` text
SMTP: 1025
Web UI: 8025
```

Ports must be configurable.

------------------------------------------------------------------------

# 63. Mailpit Project Integration

Example:

``` yaml
mail:
  provider: mailpit
  enabled: true
```

OLS may automatically configure framework variables such as:

``` text
MAIL_HOST=127.0.0.1
MAIL_PORT=1025
```

The exact environment variables depend on the project/framework.

------------------------------------------------------------------------

# 64. Mailpit UI Integration

Project dashboard:

``` text
Mail

Mailpit
● Running

Messages:
12

[Open Mailpit]
[Restart]
[Logs]
```

------------------------------------------------------------------------

# 65. Mail Testing

Workflow:

``` text
Application
     ↓
SMTP
     ↓
Mailpit
     ↓
Captured Message
     ↓
Mailpit Web UI
```

Normal local email should not be sent to real external recipients.

------------------------------------------------------------------------

# 66. Mail Diagnostics

Verify:

``` text
✓ Mailpit process
✓ SMTP port
✓ Web UI
✓ Project mail settings
✓ Test SMTP connection
```

------------------------------------------------------------------------

# 67. Custom Services

Users can define custom managed services.

Example:

``` text
Name:
Meilisearch

Executable:
meilisearch

Arguments:
--http-addr 127.0.0.1:7700

Port:
7700
```

Custom services use the same process supervision and health-check
architecture.

------------------------------------------------------------------------

# 68. Service Dependencies

Example:

``` text
Application
├── PHP
├── Nginx
├── MySQL
├── Redis
└── Mailpit
```

OLS should calculate startup order and wait for dependencies to
become healthy.

------------------------------------------------------------------------

# 69. Profiles

Profiles are reusable environment definitions.

Examples:

``` text
Laravel Standard
Laravel + Redis
Laravel + MySQL + Mailpit
Node API
Python API
Full Stack
WordPress
Custom
```

------------------------------------------------------------------------

# 70. Project Modes

Projects may define:

``` text
Development
Testing
Debugging
Demo
```

Example:

``` text
Debugging Mode
    ↓
Enable Xdebug
Enable verbose logging
Start Mailpit
Start queue workers
```

------------------------------------------------------------------------

# 71. Environment Manifests

Projects may contain:

``` text
.openlocalserver/
    environment.yaml
    services.yaml
    commands.yaml
```

Example:

``` yaml
name: shop

runtime:
  php: "8.4"
  node: "22"

web:
  server: nginx

domain:
  hostname: shop.test
  https: true
  wildcard: true

database:
  engine: mysql
  version: "8.4"

services:
  redis: true
  mailpit: true

workers:
  queue: true

scheduler: true

tunnel:
  enabled: false
```

------------------------------------------------------------------------

# 72. Environment Lock File

Support:

``` text
.openlocalserver/environment.lock
```

Example:

``` yaml
php: 8.4.12
node: 22.15.0
mysql: 8.4.x
redis: 7.x
```

This improves reproducibility.

------------------------------------------------------------------------

# 73. Environment Reconstruction

Command:

``` text
ols setup
```

must be able to:

1.  Read the manifest.
2.  Resolve versions.
3.  Install missing components.
4.  Configure services.
5.  Create databases.
6.  Configure domains.
7.  Generate certificates.
8.  Configure HTTPS.
9.  Configure Mailpit.
10. Configure web server.
11. Start dependencies.
12. Start tunnel only when explicitly configured.
13. Run health checks.

------------------------------------------------------------------------

# 74. Environment Resolver

Resolution flow:

``` text
Project
 ↓
Framework
 ↓
Runtime requirements
 ↓
Extensions
 ↓
Package managers
 ↓
Database requirements
 ↓
Services
 ↓
Domains
 ↓
DNS
 ↓
SSL
 ↓
Mail
 ↓
Workers
 ↓
Tunnel
```

------------------------------------------------------------------------

# 75. Conflict Detection

OLS must detect:

-   Port conflicts
-   Runtime conflicts
-   Database instance conflicts
-   Domain conflicts
-   Certificate conflicts
-   Service conflicts
-   File ownership conflicts

It must propose safe resolutions instead of silently killing unrelated
processes.

------------------------------------------------------------------------

# 76. Environment Plans

Before significant changes:

``` text
Environment Plan

Install:
  PHP 8.4
  Node 22
  Redis
  Mailpit

Create:
  MySQL database "shop"

Configure:
  Nginx
  shop.test
  *.shop.test
  HTTPS

Start:
  Nginx
  MySQL
  Redis
  Mailpit

Tunnel:
  Disabled

[Apply] [Cancel]
```

------------------------------------------------------------------------

# 77. Dry Run

Support:

``` text
ols setup --dry-run
```

No changes should be applied.

------------------------------------------------------------------------

# 78. Rollback

Operations should be tracked.

Example:

``` text
Step 1 ✓
Step 2 ✓
Step 3 ✓
Step 4 ✗
```

Safe changes should be rolled back when possible.

------------------------------------------------------------------------

# 79. Quick Apps

Quick Apps are a first-class feature for rapidly creating projects and
environments.

A Quick App is an editable recipe that can define:

-   Project creation
-   Required runtimes
-   Required versions
-   Services
-   Databases
-   Domains
-   HTTPS
-   Mailpit
-   Environment variables
-   Commands
-   Pre-install steps
-   Post-install steps
-   Health checks
-   Tunnel configuration
-   Project templates

------------------------------------------------------------------------

# 80. Quick Apps UI

Example:

``` text
Quick Apps
────────────────────────────

+ New Quick App

Laravel
Symfony
WordPress
React + Vite
Vue + Vite
Next.js
Express API
FastAPI
Django
Plain PHP
Static HTML
Custom App
```

Users can:

-   Search
-   Filter
-   Favorite
-   Duplicate
-   Edit
-   Delete
-   Import
-   Export
-   Create

------------------------------------------------------------------------

# 81. Quick App Wizard

Example:

``` text
Create Laravel Application

Project Name:
[ my-shop                 ]

PHP:
[ 8.4 ▼ ]

Node:
[ 22 ▼ ]

Database:
[ MySQL ▼ ]

Redis:
[ ✓ ]

Mailpit:
[ ✓ ]

HTTPS:
[ ✓ ]

Domain:
[ my-shop.test            ]

Public Tunnel:
[ No ▼ ]

[Create App]
```

------------------------------------------------------------------------

# 82. Quick App Catalog

Quick Apps should be stored in editable catalog files.

Suggested structure:

``` text
quick-apps/
├── laravel.yaml
├── symfony.yaml
├── wordpress.yaml
├── react.yaml
├── vue.yaml
├── nextjs.yaml
├── fastapi.yaml
├── django.yaml
└── custom.yaml
```

------------------------------------------------------------------------

# 83. Quick App Definition

Example:

``` yaml
id: laravel-app
name: Laravel Application
description: Create a new Laravel application

requirements:
  php: "8.4"
  node: "22"
  mysql: "8.4"
  redis: true
  mailpit: true

commands:
  - composer create-project laravel/laravel "{{project_name}}"

variables:
  project_name:
    label: Project Name
    type: text
    required: true

  php_version:
    label: PHP Version
    type: select
    options:
      - "8.2"
      - "8.3"
      - "8.4"
```

------------------------------------------------------------------------

# 84. Quick App Variables

Supported variable types:

``` text
text
number
boolean
select
multiselect
path
directory
file
password
secret
port
domain
runtime-version
database-version
```

Variables may be:

-   Required
-   Optional
-   Defaulted
-   Validated
-   Conditional

------------------------------------------------------------------------

# 85. Quick App Conditional Logic

Example:

``` yaml
conditions:
  - if: "database == mysql"
    commands:
      - configure-mysql

  - if: "redis == true"
    commands:
      - enable-redis
```

------------------------------------------------------------------------

# 86. Quick App Pre/Post Commands

Support:

``` text
pre_create
post_create
pre_install
post_install
pre_start
post_start
```

Commands must execute through the controlled Command Runner.

------------------------------------------------------------------------

# 87. Quick App Import/Export

Support:

``` text
Export Quick App
Import Quick App
Duplicate Quick App
Edit Quick App
```

Possible sources:

``` text
Local file
Local folder
Git repository
Private company catalog
Community catalog
```

------------------------------------------------------------------------

# 88. Quick App Catalog Sources

OLS should support:

``` text
Built-in Catalog
Local Catalog
Git Catalog
Company Catalog
Community Catalog
```

Catalog sources must be explicitly trusted before executing commands.

------------------------------------------------------------------------

# 89. Quick Commands

Quick Commands are reusable developer commands that do not necessarily
create a project.

Examples:

``` text
Clear Laravel Cache
Run Migrations
Install Dependencies
Start Queue Worker
Run Tests
Build Frontend
Open Mailpit
Start Tunnel
Restart Project
Open Nginx Config
Open Apache Config
```

------------------------------------------------------------------------

# 90. Command Runner

The Command Runner is a core subsystem.

It should support:

-   Command
-   Arguments
-   Working directory
-   Environment variables
-   Runtime selection
-   Shell selection
-   Timeout
-   Exit code
-   stdout
-   stderr
-   Interactive terminal
-   Background execution
-   Cancellation
-   Logging

------------------------------------------------------------------------

# 91. Command Execution Modes

``` text
Interactive
Background
Detached
Service
Quick Command
Quick App step
```

------------------------------------------------------------------------

# 92. Command Security

Commands must be treated as potentially dangerous.

Requirements:

-   Clearly display commands before first execution when originating
    from an untrusted catalog.
-   Show required permissions.
-   Do not silently execute imported commands.
-   Provide confirmation for elevated operations.
-   Redact secrets from logs.
-   Support trusted catalog sources.
-   Provide command history.

------------------------------------------------------------------------

# 93. Command History

Example:

``` text
Recent Commands

php artisan migrate
npm install
composer install
npm run build
ols tunnel start
```

Users can:

``` text
Run Again
Edit
Save as Quick Command
Copy
Delete History Entry
```

------------------------------------------------------------------------

# 94. External Editor Integration

OLS should **not embed a full code editor such as Notepad++ inside
the application**.

Instead, it should integrate with editors installed on the user's
system.

------------------------------------------------------------------------

# 95. Supported External Editors

Initial integrations:

-   Notepad++
-   Visual Studio Code
-   Cursor
-   PhpStorm
-   IntelliJ IDEA
-   Sublime Text
-   System Default Editor
-   Custom executable

The system should remain extensible.

------------------------------------------------------------------------

# 96. Notepad++ Integration

On Windows, OLS should detect an installed Notepad++ executable.

The user can choose:

``` text
Settings
→ Editors
→ Preferred Editor
→ Notepad++
```

OLS should not require a fixed installation path.

Possible installation locations must be detected.

------------------------------------------------------------------------

# 97. Open With Menu

Project actions:

``` text
Open With
──────────────
Notepad++
VS Code
Cursor
PhpStorm
System Default
Custom Editor
```

------------------------------------------------------------------------

# 98. Editor Use Cases

The selected external editor should be available for:

-   Project files
-   `.env`
-   YAML
-   JSON
-   Nginx configuration
-   Apache configuration
-   Caddyfile
-   PHP files
-   Logs
-   Quick App files
-   Quick Command files
-   Project manifests

------------------------------------------------------------------------

# 99. Editor Command Configuration

Example conceptual configuration:

``` yaml
editor:
  name: notepad++
  executable: "C:/Program Files/Notepad++/notepad++.exe"
  arguments:
    - "{{file}}"
```

OLS should support placeholders such as:

``` text
{{file}}
{{line}}
{{column}}
{{project_path}}
```

where the selected editor supports them.

------------------------------------------------------------------------

# 100. File and Folder Shortcuts

Project actions:

``` text
Open Project
Open public/
Open config/
Open .env
Open Logs
Open Nginx Config
Open Apache Config
Open Caddyfile
Open Terminal Here
Open With Notepad++
```

------------------------------------------------------------------------

# 101. One-Click Project Actions

Each project should provide:

``` text
Start
Stop
Restart
Open Site
Open HTTPS
Open Terminal
Open Project Folder
Open Mailpit
Open Database Tool
Start Tunnel
Stop Tunnel
Copy URL
Diagnose
Repair
Configure
Open Logs
```

------------------------------------------------------------------------

# 102. Database GUI Integration

OLS does not need to become a full database IDE.

Instead, it should support external database tools.

Examples:

``` text
Open MySQL in External Tool
Open PostgreSQL in External Tool
Open MongoDB in External Tool
Open SQLite in External Tool
```

Users can configure their preferred application.

------------------------------------------------------------------------

# 103. Environment Variables

Provide a GUI editor for:

-   Add
-   Edit
-   Delete
-   Import
-   Export
-   Compare
-   Hide secrets
-   Validate

Support:

``` text
.env
.env.local
.env.testing
.env.development
```

Secrets must be protected.

------------------------------------------------------------------------

# 104. Secrets Manager

Sensitive data includes:

-   Database passwords
-   API keys
-   Tunnel tokens
-   SSH keys
-   Certificate private keys
-   Environment secrets

Use native OS credential storage where practical.

------------------------------------------------------------------------

# 105. Queue Workers

Support managed workers such as:

``` text
Laravel Queue
Celery
BullMQ
Custom Worker
```

Configuration:

-   Command
-   Worker count
-   Timeout
-   Retry
-   Memory
-   Restart policy

------------------------------------------------------------------------

# 106. Scheduler

Provide GUI scheduling.

Examples:

``` text
Every minute
Every 5 minutes
Hourly
Daily
Custom cron
```

Commands may be associated with projects.

------------------------------------------------------------------------

# 107. Process Supervisor

Track:

-   PID
-   Parent PID
-   Executable
-   Arguments
-   Working directory
-   Environment
-   Start time
-   Exit code
-   CPU
-   Memory
-   Status

States:

``` text
Starting
Running
Stopping
Stopped
Crashed
Restarting
Failed
Unknown
```

------------------------------------------------------------------------

# 108. Crash Recovery

Optional policy:

``` text
[x] Restart crashed service
Retries: 3
Delay: 5 seconds
```

Relevant logs must be retained.

------------------------------------------------------------------------

# 109. Port Manager

Display:

``` text
80       Nginx
443      Nginx
3306     MySQL
5432     PostgreSQL
6379     Redis
27017    MongoDB
1025     Mailpit SMTP
8025     Mailpit UI
```

OLS must detect conflicts before starting services.

It must not automatically kill unrelated processes.

------------------------------------------------------------------------

# 110. Request and Traffic Inspector

A lightweight development traffic inspector should eventually show:

``` text
GET  /api/users       200
POST /login           302
GET  /dashboard       200
```

Information may include:

-   Method
-   URL
-   Status
-   Duration
-   Headers
-   Request size
-   Response size

Sensitive values must be redacted.

------------------------------------------------------------------------

# 111. Webhook Tester

Because OLS includes tunneling, it should provide a webhook
workflow:

``` text
Public Tunnel
      ↓
Webhook
      ↓
Local Project
      ↓
Traffic Inspector
```

Useful for:

-   Payment callbacks
-   GitHub webhooks
-   OAuth
-   External APIs

------------------------------------------------------------------------

# 112. Project Diagnostics

Example:

``` text
shop

✓ PHP 8.4
✓ Composer
✓ MySQL
✓ Redis
✓ Mailpit
✓ Nginx
✓ DNS
✓ HTTPS
✗ APP_KEY missing
```

Each issue should provide:

``` text
Problem
Cause
Suggested Fix
[Fix]
[Ignore]
[Details]
```

------------------------------------------------------------------------

# 113. Environment Doctor

CLI:

``` text
ols doctor
```

Example:

``` text
OLS Doctor

✓ Operating system supported
✓ PHP available
✓ Node available
✓ Python available
✓ Nginx valid
✓ MySQL valid
✓ PostgreSQL valid
✓ MongoDB valid
✓ Redis valid
✓ Mailpit valid
✓ DNS valid
✓ Local CA trusted

Warnings:
⚠ Xdebug disabled

Errors:
✗ Redis unreachable
```

------------------------------------------------------------------------

# 114. Automatic Repair

A project should have:

``` text
[Repair Environment]
```

OLS should:

1.  Diagnose.
2.  Explain detected issues.
3.  Show proposed changes.
4.  Ask for confirmation when destructive.
5.  Apply safe repairs.
6.  Re-run health checks.

------------------------------------------------------------------------

# 115. Diagnostic Explanations

Instead of:

``` text
Nginx failed.
```

Show:

``` text
Nginx could not start.

Cause:
Port 443 is already in use.

Detected process:
...

Options:

[Inspect Process]
[Change OLS HTTPS Port]
[Retry]
```

------------------------------------------------------------------------

# 116. Environment Health

Display:

``` text
Environment Health

Runtime       ✓
Database      ✓
Redis         ✓
Mailpit       ✓
DNS           ✓
HTTPS         ✓
Web Server    ✓
Tunnel        —
```

Health must represent technical state, not a subjective score.

------------------------------------------------------------------------

# 117. Logging

Centralized logs:

-   OLS
-   Nginx
-   Apache
-   Caddy
-   PHP
-   MySQL
-   PostgreSQL
-   MongoDB
-   Redis
-   Mailpit
-   Workers
-   Schedulers
-   Tunnels
-   Quick App execution
-   Quick Commands

Features:

-   Live tail
-   Search
-   Filter
-   Severity
-   Copy
-   Export

------------------------------------------------------------------------

# 118. Structured Logging

Example:

``` json
{
  "timestamp": "2026-09-21T19:41:02Z",
  "level": "error",
  "component": "ProcessSupervisor",
  "service": "mysql",
  "event": "process_exit",
  "pid": 2312,
  "exitCode": 1
}
```

Secrets must be redacted.

------------------------------------------------------------------------

# 119. Notifications

Notify users about:

-   Service failures
-   Setup completion
-   Port conflicts
-   Certificate expiration
-   Runtime installation
-   Environment failures
-   Tunnel start/stop
-   Public exposure
-   Updates
-   Mailpit failures

------------------------------------------------------------------------

# 120. System Tray

Example:

``` text
OLS

● Nginx
● MySQL
● Redis
● Mailpit
○ Tunnel

Open Dashboard
Start All
Stop All
Restart All

Exit
```

------------------------------------------------------------------------

# 121. Startup Behavior

Settings:

``` text
[x] Start OLS with system
[x] Start selected services
[x] Minimize to tray
```

Public tunnels should remain disabled by default at startup.

------------------------------------------------------------------------

# 122. Command Palette

Provide a global command palette.

Suggested shortcut:

``` text
Ctrl + Shift + P
```

Example:

``` text
> Start shop
> Open Mailpit
> Install PHP 8.4
> Create Quick App
> Start Tunnel
> Open Nginx Config
> Run Laravel migrations
> Diagnose Project
> Repair Environment
```

------------------------------------------------------------------------

# 123. Global Project Search

Search across:

-   Projects
-   Services
-   Domains
-   Quick Apps
-   Quick Commands
-   Logs
-   Configurations
-   Runtimes

------------------------------------------------------------------------

# 124. Context Menu Integration

Where supported by the OS:

``` text
Right-click project folder

Open with OLS
Create OLS Environment
Start Project
Open Terminal
```

------------------------------------------------------------------------

# 125. Git Integration

Basic features:

-   Repository detection
-   Branch
-   Status
-   Changed files
-   Untracked files
-   Latest commit
-   Commit
-   Pull
-   Push
-   Branch list
-   Open repository

OLS should not replace Git clients for advanced Git workflows.

------------------------------------------------------------------------

# 126. Import Existing Environments

OLS should eventually detect and import environments from:

``` text
XAMPP
Laragon
Existing PHP installations
Existing MySQL
Existing MariaDB
Existing PostgreSQL
Existing Nginx
Existing Apache
```

The user should be offered:

``` text
Register Existing Installation
Use OLS Managed Installation
Ignore
```

OLS must not modify an existing installation without confirmation.

------------------------------------------------------------------------

# 127. Runtime Cache

Downloaded runtime/service packages should be cached.

Example:

``` text
PHP 8.4 downloaded once
        ↓
Project A
Project B
Project C
```

No unnecessary repeated downloads.

------------------------------------------------------------------------

# 128. Offline Mode

Once components are cached, OLS should support as much offline
operation as practical.

Offline capabilities may include:

-   Starting installed services
-   Existing project management
-   Local HTTPS
-   Local DNS
-   Mailpit
-   Quick Commands
-   Cached Quick Apps
-   Diagnostics
-   Logs

Network-dependent features must clearly indicate when internet access is
required.

------------------------------------------------------------------------

# 129. Resource Controls

Optional resource configuration:

``` text
MySQL memory
PostgreSQL memory
Redis memory
Worker count
Node memory
Process limits
```

The implementation must respect platform limitations.

------------------------------------------------------------------------

# 130. Backups

Support:

-   Database backups
-   Database restores
-   Configuration backups
-   Environment backups
-   Certificate metadata backups
-   OLS settings backups

Destructive operations should offer backup opportunities.

------------------------------------------------------------------------

# 131. Snapshots

A project snapshot may include:

``` text
Project configuration
Runtime selection
Service configuration
Web-server configuration
Domain configuration
Certificate metadata
Database metadata
Mail configuration
Tunnel configuration
Quick Commands
```

Actual source/database data inclusion should be optional.

------------------------------------------------------------------------

# 132. Import / Export

Support:

``` text
Export Project Environment
Import Project Environment
Export Quick App
Import Quick App
Export Profile
Import Profile
```

Imported configurations must be reviewed before destructive operations.

------------------------------------------------------------------------

# 133. Plugin Architecture

Plugins may provide:

-   Runtime definitions
-   Database definitions
-   Service definitions
-   Framework templates
-   Project detection
-   Configuration generators
-   Health checks
-   Commands
-   Quick Apps
-   Quick Commands
-   Tunnel providers
-   UI
-   Diagnostics
-   External tool integrations

------------------------------------------------------------------------

# 134. Plugin Permissions

Plugins may request:

``` text
Read project files
Write project files
Execute processes
Modify hosts file
Install certificates
Access network
Install software
Access secrets
```

Permissions should be explicit.

------------------------------------------------------------------------

# 135. Plugin Manifest

Example:

``` json
{
  "id": "laravel",
  "name": "Laravel",
  "version": "1.0.0",
  "type": "framework",
  "capabilities": [
    "project-template",
    "environment-detection",
    "commands",
    "health-check"
  ]
}
```

------------------------------------------------------------------------

# 136. CLI

Examples:

``` text
ols start
ols stop
ols restart

ols project list
ols project create shop
ols project start shop
ols project stop shop

ols runtime list
ols runtime install php 8.4
ols php use 8.4

ols service list
ols service logs nginx

ols domain list
ols certificate list

ols tunnel list
ols tunnel start shop
ols tunnel stop shop

ols quick-app list
ols quick-app create
ols quick-command run migrate

ols doctor
ols repair shop
ols setup
```

------------------------------------------------------------------------

# 137. Local API

Potential API:

``` text
GET  /api/projects
GET  /api/runtimes
GET  /api/services
GET  /api/domains
GET  /api/certificates
GET  /api/tunnels
GET  /api/mail
GET  /api/quick-apps
GET  /api/logs

POST /api/services/{id}/start
POST /api/services/{id}/stop
POST /api/tunnels/{id}/start
POST /api/tunnels/{id}/stop
POST /api/projects/{id}/repair
```

The API must listen on localhost by default.

State-changing operations must require authorization.

------------------------------------------------------------------------

# 138. Security

OLS controls:

-   Processes
-   Files
-   Ports
-   DNS
-   Hosts files
-   Certificates
-   Public tunnels
-   Installed software
-   External commands

Requirements:

1.  Main application must not permanently run elevated.
2.  Privileged operations must use a helper.
3.  Packages must be verified.
4.  Secrets must not be logged.
5.  Local API must not be exposed to LAN by default.
6.  Plugins require permissions.
7.  Untrusted Quick Apps must not execute silently.
8.  Public tunneling requires explicit action.
9.  Database ports must not be exposed by default.
10. Private keys must be protected.

------------------------------------------------------------------------

# 139. Quick App Security

Quick Apps can execute commands and modify environments.

Therefore:

``` text
Built-in catalog
    → trusted

User-created local catalog
    → trusted by user

Imported Git/community catalog
    → untrusted until approved
```

Before executing an untrusted Quick App:

``` text
Quick App wants to:

Install software
Execute commands
Write files
Create database
Modify DNS
Create certificate

[Review Commands]
[Allow Once]
[Trust Source]
[Cancel]
```

------------------------------------------------------------------------

# 140. Public Exposure Safety

When a tunnel starts:

``` text
This project will become accessible from the public internet.

Public URL:
https://public-example.example

Local Target:
https://shop.test

Do not expose:
- Databases
- Admin interfaces
- Private tools
- Secrets
```

First-time exposure requires explicit confirmation.

------------------------------------------------------------------------

# 141. Secrets

Never expose in normal logs:

-   Database passwords
-   API keys
-   Tunnel tokens
-   SSH keys
-   Certificate private keys
-   Environment secrets

Use platform-native secure credential storage where practical.

------------------------------------------------------------------------

# 142. Certificate Private Keys

Requirements:

-   Restrict filesystem permissions where supported.
-   Never display private keys in normal UI.
-   Never log private keys.
-   Never upload private keys to third-party tunnel providers.
-   Provide secure deletion where practical.

------------------------------------------------------------------------

# 143. Telemetry

Recommended default:

``` text
Disabled.
```

If enabled:

-   Explain what is collected.
-   Provide opt-out.
-   Never collect source code.
-   Never collect `.env`.
-   Never collect database contents.
-   Never collect private keys.
-   Never collect passwords.
-   Never collect tunnel tokens.

------------------------------------------------------------------------

# 144. Open Source

Repository should include:

``` text
README
LICENSE
CONTRIBUTING
CODE_OF_CONDUCT
SECURITY
CHANGELOG
docs/
```

Documentation should cover:

-   Architecture
-   Building
-   Development
-   Plugins
-   Runtime catalog
-   Security
-   Release process
-   Packaging

------------------------------------------------------------------------

# 145. Update System

Application update:

``` text
Check
 ↓
Download
 ↓
Verify
 ↓
Install
 ↓
Restart
```

Runtime/service updates remain independently controlled.

------------------------------------------------------------------------

# 146. File System Layout

Conceptual:

``` text
OLS/
├── app/
├── runtimes/
│   ├── php/
│   ├── node/
│   └── python/
├── services/
│   ├── nginx/
│   ├── apache/
│   ├── caddy/
│   ├── mysql/
│   ├── mariadb/
│   ├── postgresql/
│   ├── mongodb/
│   ├── redis/
│   └── mailpit/
├── projects/
├── certificates/
├── tunnels/
├── quick-apps/
├── quick-commands/
├── plugins/
├── logs/
├── cache/
└── data/
```

Actual paths must follow OS conventions.

------------------------------------------------------------------------

# 147. Rust Architecture

Suggested:

``` text
src/
├── app/
├── commands/
├── domain/
├── project/
├── quick_apps/
├── quick_commands/
├── command_runner/
├── runtime/
├── database/
├── service/
├── process/
├── network/
├── dns/
├── certificates/
├── webserver/
├── web_config/
├── proxy/
├── tunnel/
├── mail/
├── package/
├── plugin/
├── editor/
├── diagnostics/
├── logging/
├── security/
├── storage/
├── backup/
├── updater/
└── cli/
```

------------------------------------------------------------------------

# 148. Frontend Architecture

Suggested:

``` text
src/
├── components/
├── pages/
├── features/
│   ├── dashboard/
│   ├── projects/
│   ├── runtimes/
│   ├── databases/
│   ├── services/
│   ├── domains/
│   ├── certificates/
│   ├── webserver/
│   ├── web-config/
│   ├── tunnels/
│   ├── mail/
│   ├── quick-apps/
│   ├── quick-commands/
│   ├── editors/
│   ├── logs/
│   ├── diagnostics/
│   └── settings/
├── stores/
├── api/
├── types/
└── utils/
```

------------------------------------------------------------------------

# 149. Core Rust Services

Recommended services:

``` text
ProjectManager
EnvironmentResolver
RuntimeManager
DatabaseManager
ServiceManager
ProcessSupervisor
DomainManager
DnsManager
CertificateManager
WebServerManager
WebConfigManager
ReverseProxyManager
TunnelManager
MailManager
PortManager
PackageManager
QuickAppManager
QuickCommandManager
CommandRunner
PluginManager
EditorManager
DiagnosticEngine
SnapshotManager
BackupManager
```

------------------------------------------------------------------------

# 150. Database Schema

Core tables:

``` text
projects
runtimes
runtime_versions
databases
database_instances
services
project_services
domains
certificates
web_configs
web_config_history
tunnels
mail_services
quick_apps
quick_commands
command_history
plugins
external_editors
profiles
snapshots
backups
events
settings
package_sources
```

Example:

``` text
projects
--------
id
name
path
framework
created_at
updated_at

domains
-------
id
project_id
hostname
type
https_enabled
wildcard
target
created_at

runtimes
--------
id
type
version
path
architecture
installed_at

database_instances
------------------
id
engine
version
host
port
data_path
status

web_configs
-----------
id
project_id
server
path
managed
hash
updated_at

tunnels
-------
id
project_id
provider
target
public_url
status
created_at

quick_apps
----------
id
name
source
version
definition_path
trusted

quick_commands
--------------
id
name
command
working_directory
source
trusted
```

------------------------------------------------------------------------

# 151. Project Manifest Example

``` yaml
name: shop

runtime:
  php: "8.4"
  node: "22"

web:
  server: nginx

domain:
  hostname: shop.test
  https: true
  wildcard: true

database:
  engine: mysql
  version: "8.4"

services:
  redis: true
  mailpit: true

workers:
  queue: true

scheduler: true

tunnel:
  enabled: false
  provider: cloudflare
  target: https://shop.test
```

------------------------------------------------------------------------

# 152. Quick App Example

``` yaml
id: laravel-shop
name: Laravel Shop
description: Create a Laravel application with MySQL, Redis and Mailpit

requirements:
  php: "8.4"
  node: "22"
  mysql: "8.4"
  redis: true
  mailpit: true

variables:
  project_name:
    type: text
    label: Project Name
    required: true

  domain:
    type: domain
    label: Domain
    default: "{{project_name}}.test"

commands:
  - composer create-project laravel/laravel "{{project_name}}"

post_create:
  - php artisan key:generate
  - php artisan migrate

environment:
  https: true
  mailpit: true
```

------------------------------------------------------------------------

# 153. Quick Command Example

``` yaml
id: laravel-migrate
name: Run Laravel Migrations
description: Run database migrations for the current project

working_directory: "{{project_path}}"

command:
  executable: php
  arguments:
    - artisan
    - migrate

environment:
  use_project_runtime: true
```

------------------------------------------------------------------------

# 154. Laravel Example Environment

Requirements:

``` text
PHP >= 8.2
Composer
mbstring
openssl
PDO
XML
ctype
Node >= 20
MySQL
Redis
Mailpit
```

OLS should detect and satisfy these automatically where possible.

------------------------------------------------------------------------

# 155. One-Click Laravel Setup

``` text
New Laravel Project

Name:
shop

PHP:
8.4

Node:
22

Database:
MySQL 8.4

Redis:
Yes

Mail:
Mailpit

Domain:
shop.test

HTTPS:
Yes

Wildcard:
Yes

Public Tunnel:
No

[Create App]
```

Result:

``` text
✓ PHP 8.4
✓ Node 22
✓ MySQL 8.4
✓ Redis
✓ Mailpit
✓ Database created
✓ Laravel installed
✓ Environment configured
✓ Nginx configured
✓ shop.test created
✓ Certificate created
✓ HTTPS trusted
✓ Health checks passed

Open:
https://shop.test
```

------------------------------------------------------------------------

# 156. Python Example

``` yaml
name: analytics

runtime:
  python: "3.13"

database:
  engine: postgresql
  version: "17"

services:
  redis: true
  mailpit: true
```

------------------------------------------------------------------------

# 157. Node Example

``` yaml
name: api

runtime:
  node: "22"

database:
  engine: postgresql
  version: "17"

services:
  redis: true
  mailpit: true
```

------------------------------------------------------------------------

# 158. Environment Cloning

Support:

``` text
Clone Project Environment
Clone Infrastructure Only
Clone Project Configuration
Clone Profile
```

Example:

``` text
Project A
    ↓
Clone Environment
    ↓
Project B
```

OLS should adjust:

-   Project name
-   Paths
-   Domains
-   Database names
-   Ports
-   Certificates

as necessary.

------------------------------------------------------------------------

# 159. Environment Reconstruction

A repository can contain:

``` text
.openlocalserver/
├── environment.yaml
├── environment.lock
├── services.yaml
└── commands.yaml
```

New developer:

``` text
git clone ...
cd project
ols setup
```

OLS reconstructs the environment.

------------------------------------------------------------------------

# 160. Testing Strategy

## Unit Tests

Test:

-   Version resolver
-   Manifest parser
-   Runtime manager
-   Port manager
-   Domain manager
-   Certificate manager
-   Web configuration generator
-   Tunnel manager
-   Mail manager
-   Quick App parser
-   Command Runner
-   Project detector
-   Dependency resolver

## Integration Tests

Test:

-   Runtime installation
-   Service startup
-   Database creation
-   Domain creation
-   Certificate generation
-   TLS verification
-   Web-server configuration
-   Reverse proxy
-   Mailpit
-   Tunnel adapters
-   Quick App execution
-   External editor integration
-   Project setup

## End-to-End

``` text
Install OLS
 ↓
Create Quick App
 ↓
Install runtime
 ↓
Create database
 ↓
Create domain
 ↓
Create certificate
 ↓
Configure web server
 ↓
Configure Mailpit
 ↓
Start services
 ↓
HTTPS request
 ↓
Send test email
 ↓
Verify Mailpit capture
 ↓
Start tunnel
 ↓
Verify public URL
 ↓
Open project in external editor
 ↓
Stop tunnel
 ↓
Stop environment
```

------------------------------------------------------------------------

# 161. Test Isolation

Tests must not modify the developer's actual environment.

Use:

-   Temporary directories
-   Dedicated ports
-   Temporary domains
-   Temporary certificates
-   Temporary databases
-   Isolated service instances
-   Mock tunnel providers

------------------------------------------------------------------------

# 162. Performance Requirements

OLS should:

-   Start quickly.
-   Remain responsive.
-   Avoid excessive idle CPU.
-   Avoid excessive memory.
-   Stream logs efficiently.
-   Use asynchronous Rust operations.
-   Avoid aggressive polling.
-   Prefer event-driven monitoring.

------------------------------------------------------------------------

# 163. Reliability Requirements

OLS should:

-   Recover from service crashes.
-   Preserve valid previous configuration.
-   Use atomic writes where practical.
-   Detect incomplete operations.
-   Maintain diagnostic logs.
-   Avoid corrupting runtime/data directories.
-   Support rollback where possible.

------------------------------------------------------------------------

# 164. MVP --- Version 0.1

Recommended initial implementation:

``` text
Tauri 2
Rust
TypeScript
SQLite

Dashboard
Project Manager
PHP Manager
Node Manager
Python Manager
Nginx
MySQL
PostgreSQL
Redis
Mailpit
Local domains
Hosts management
Local CA
HTTPS
Certificate Manager
Process Supervisor
Logs
Terminal
Quick Apps
Quick Commands
External Editor integration
```

------------------------------------------------------------------------

# 165. Version 0.2

Add:

``` text
Apache
Caddy
MariaDB
MongoDB
Multiple database versions
PHP extensions
Xdebug
Composer
npm/pnpm
Python virtual environments
Reverse proxy
Wildcard domains
Wildcard certificates
Project diagnostics
Web-server configuration editor
Configuration validation
Configuration history
External database tools
```

------------------------------------------------------------------------

# 166. Version 0.3

Add:

``` text
Profiles
Environment manifests
Environment lock files
CLI
Environment import/export
Snapshots
Queue workers
Schedulers
Environment plans
Rollback
Public tunnel integrations
Tunnel dashboard
Webhook tester
Request inspector
Command palette
Automatic repair
```

------------------------------------------------------------------------

# 167. Version 1.0

Target:

``` text
Complete cross-platform support
Plugin system
Runtime catalog
Database catalog
Service catalog
Secure updater
Advanced diagnostics
Project environment reconstruction
Complete DNS/domain/SSL system
Web-server configuration management
Public tunneling integrations
Quick App catalogs
Quick Command catalogs
External editor integrations
Strong privilege separation
Comprehensive documentation
```

------------------------------------------------------------------------

# 168. Future Features

Potential future capabilities:

-   Docker integration
-   WSL integration
-   Container orchestration helpers
-   Team environment catalogs
-   Private company catalogs
-   Remote development environments
-   Cloud deployment integrations
-   AI-assisted diagnostics
-   Environment migration
-   More runtimes
-   More databases
-   More tunnel providers
-   More mail-development integrations

------------------------------------------------------------------------

# 169. Product Differentiation

OLS differentiates itself through:

1.  Per-project runtime versions.
2.  Multiple PHP versions simultaneously.
3.  Per-project PHP selection.
4.  Multiple Node versions.
5.  Multiple Python versions.
6.  Python virtual environments.
7.  Multiple database engines.
8.  Multiple database versions.
9.  Automatic requirement detection.
10. Local domains.
11. Subdomains.
12. Wildcard domains.
13. Local trusted Certificate Authority.
14. Wildcard HTTPS.
15. Reverse proxy management.
16. Editable Nginx/Apache/Caddy configurations.
17. Configuration history and rollback.
18. Public internet tunneling.
19. Mailpit integration.
20. Environment manifests.
21. Environment lock files.
22. Reproducible environments.
23. Process supervision.
24. Automatic diagnostics.
25. Automatic environment repair.
26. Quick Apps.
27. Quick Commands.
28. Editable automation catalogs.
29. External editor integration.
30. XAMPP/Laragon migration.
31. Plugin architecture.
32. Cross-platform support.
33. Free and open-source distribution.

------------------------------------------------------------------------

# 170. Critical Product Principle

OLS must not become:

``` text
A GUI that launches random shell scripts.
```

It must be:

``` text
                         OLS
                            │
                 ┌──────────┴──────────┐
                 │                     │
                GUI                   CLI
                 │                     │
                 └──────────┬──────────┘
                            │
                     Application Core
                            │
       ┌────────────────────┼────────────────────┐
       │                    │                    │
   Projects             Runtimes             Databases
       │                    │                    │
   Quick Apps           PHP/Node/Python       MySQL/Postgres
       │                    │                    │
   Commands             Extensions            MongoDB/SQLite
       │                    │                    │
   Web Config          Environment            Redis
       │                    │                    │
   Domains               Resolver             Mailpit
       │                    │                    │
   DNS/SSL               Packages             Workers
       │                    │                    │
   Tunnels               Plugins              Processes
       │                    │                    │
   Editors              Diagnostics           Backups
       └────────────────────┼────────────────────┘
                            │
                      Platform Layer
                            │
                 Windows / macOS / Linux
```

------------------------------------------------------------------------

# 171. Final Recommended Stack

``` text
Desktop:
Tauri 2

Backend:
Rust

Frontend:
TypeScript + React/Vue/Svelte

Metadata:
SQLite

Project configuration:
YAML/JSON

Runtime management:
Rust

Process management:
Rust

DNS:
Rust + platform implementations

SSL:
Rust + local CA subsystem

Web configuration:
Rust configuration manager + frontend editor

Tunneling:
Provider abstraction + provider adapters

Mail:
Mailpit

CLI:
Rust

Quick Apps:
YAML/JSON catalog + GUI editor

Quick Commands:
YAML/JSON catalog + Command Runner

External editors:
System-installed applications
including Notepad++

Initial runtimes:
PHP
Node.js
Python

Initial web servers:
Nginx
Apache
Caddy

Initial databases/services:
MySQL
MariaDB
PostgreSQL
MongoDB
SQLite
Redis
Mailpit
```

------------------------------------------------------------------------

# 172. Final Developer Experience

The ideal workflow:

``` text
Install OLS
        ↓
Open existing project
        ↓
Detect framework
        ↓
Detect requirements
        ↓
Resolve versions
        ↓
Show Environment Plan
        ↓
Install missing runtimes/services
        ↓
Create database
        ↓
Configure Redis
        ↓
Configure Mailpit
        ↓
Create local domain
        ↓
Create local CA certificate
        ↓
Configure Nginx/Apache/Caddy
        ↓
Start services
        ↓
Run diagnostics
        ↓
Open project in Notepad++ / VS Code / preferred editor
        ↓
Open browser
        ↓
Optionally start public tunnel
```

------------------------------------------------------------------------

# 173. Example Complete Dashboard

``` text
┌──────────────────────────────────────────────────────────────┐
│ OLS                                                     │
├──────────────────────────────────────────────────────────────┤
│ Environment: HEALTHY                                         │
│                                                              │
│ Runtime                                                      │
│ PHP       8.4        ● Running                               │
│ Node      22         ● Ready                                 │
│ Python    3.13       ● Ready                                 │
│                                                              │
│ Services                                                     │
│ Nginx     1.28       ● Running                               │
│ MySQL     8.4        ● Running                               │
│ Redis     7          ● Running                               │
│ Mailpit   latest     ● Running                               │
│                                                              │
│ Project: shop                                                │
│                                                              │
│ https://shop.test                                            │
│ https://api.shop.test                                       │
│ https://admin.shop.test                                     │
│                                                              │
│ SSL:        ✓ Trusted                                        │
│ Database:   ✓ Connected                                      │
│ Redis:      ✓ Connected                                      │
│ Mail:       ✓ Mailpit connected                              │
│ Health:     ✓ All checks passed                              │
│                                                              │
│ Public Tunnel: OFF                                           │
│                                                              │
│ [Start] [Terminal] [Open Site] [Mailpit] [Editor]            │
│ [Tunnel] [Diagnose] [Repair] [Configure]                     │
└──────────────────────────────────────────────────────────────┘
```

------------------------------------------------------------------------

# 174. Definition of Success

OLS is successful when a developer can take a new or existing
project and go from:

``` text
"I just cloned this repository."
```

to:

``` text
"The application is running locally over trusted HTTPS,
its database and background services are ready,
local emails are captured by Mailpit,
the correct runtime versions are active,
the project is available in my preferred external editor,
and I can optionally expose it through a public tunnel."
```

The system must remain:

-   Free
-   Open source
-   Cross-platform
-   Extensible
-   Secure
-   Reproducible
-   Developer-focused
-   Maintainable

OLS should feel like a complete local development operating
environment rather than a simple collection of local web servers.
