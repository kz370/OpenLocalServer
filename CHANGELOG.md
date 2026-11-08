# Changelog

All notable changes are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [2.11.0] — 2026-11-08

Feature release. Full notes: [`release-notes/v2.11.0.md`](./release-notes/v2.11.0.md).

### Added
- Docker page updates live while it is open: container starts, stops and recreates refresh the list and move linked sites to the new port (no polling; one event stream that stops with the page).
- `olsc docker`: status, containers (start, stop, restart, pause, unpause, logs), images, volumes, disk usage, pull, Compose stacks (list, up, down, stop, start, restart, pull, build, check) and `prune --dry-run`. Nothing is removed from the command line.
- Diagnostics notices for Docker sites: container stopped (with a Start button), container missing, and Docker not running. Nothing is asked of Docker when no site is linked, and none of these is fixed automatically.
- Site bundles keep a site's Docker link and compose file, never its passwords; the import preview adds `docker_links` and `compose_files_need_passwords` and shows a notice.
- Dockerize a project: Dockerfile, compose file and `.dockerignore` from what OLS detects (PHP, Node, Python, Go, Rust, Ruby, Java, .NET, static). The project is copied into the image and Development mode uses `docker compose watch` (new `watch` compose action); Production mode builds multi-stage non-root images. Detected services (database, cache, queue, mail, search) are added as pinned images with generated passwords, health checks and app connection settings; queue workers, scheduler, Sidekiq and Celery run from the app image. Package manager, framework, language version and system packages are detected from fixed tables. MCP `ols_docker_dockerize` takes `mode` and `services`; fixed notice that they are a starting point, never overwrites; Compose → From a project…, `Dockerize…` in the Projects and Sites right-click menus (setting `ui.docker.dockerize_menu`, hidden when the Docker page is hidden), MCP `ols_docker_dockerize`.

## [2.10.0] — 2026-11-07

Feature release. Full notes: [`release-notes/v2.10.0.md`](./release-notes/v2.10.0.md).

### Added
- Docker page under Sites: containers grouped by Compose project (search, filters, start/stop/restart/pause, ports, logs, shell, settings and environment editing). Docker stays optional.
- Images and Volumes tabs with Reclaim space (preview first).
- Compose tab, built-in compose editor with Check, and a compose builder with your own presets; passwords stay in the Windows credential store.
- Connect to OLS and Create site: containers reach OLS sites and services, and any container can get a local HTTPS site.
- MCP tools for Docker and `ols_unignore_site`.
- Settings → Sidebar: hide pages you don't use.
- Projects: select several projects, then Create sites, Hide or Remove from list.
- Projects and Sites: group or sort by root folder.
- Import databases: a message when the import finishes, the selection is cleared, and an import report (imported, skipped or failed per database, with the reason) that you can copy or save as a text file. No passwords in the report.
- Dismiss all button under the notifications; notifications for running tasks stay.

### Changed & Improved
- Docker does nothing until its page is open, and its lists load from a cache.
- Sites and Projects load faster and show a spinner while loading. Remove from list is instant and works when the project folder is gone. The Locate button no longer touches the settings gear.

### Security
- DDNS credentials are kept in the Windows credential store (moved on first start; not in snapshots or site bundles).
- PECL extensions install only when OLS knows the build's SHA-256 (apcu, redis, igbinary, imagick, mongodb, xdebug, pcov, yaml; PHP 8.1–8.5).
- The compose editor saves only stacks OLS knows; site bundle imports can't write outside their folders.
- The MCP server can no longer read stored passwords, confirm a tunnel, or overwrite a compose file.
- Updated rustls, rustls-webpki, webpki-roots, ctutils and zip (RUSTSEC-2026-0346/0347/0348/0349/0381/0382/0385/0367/0368/0369/0429).

### Fixed
- Compose Check no longer reports a port conflict for TCP + UDP or separate addresses.
- Turning Docker sites on or off handles every container, even when one fails.
- Starting a tunnel twice no longer runs two copies.
- Text like `<name>` in release notes and the manual is visible again.
- Processes → Usage by site: the table header matches the card.
- Right-click no longer opens the browser's menu (text fields keep Cut, Copy and Paste).

## [2.9.0] — 2026-11-05

Feature release. Full notes: [`release-notes/v2.9.0.md`](./release-notes/v2.9.0.md).

### Added
- Import databases from several places at once, in the order you choose.
- Choose what happens with duplicate databases: skip, replace (backed up first), or import under a new name.
- Explorer menu to add, set up, or ignore a project from a right-click.
- `.ols_ignore_site` hides only a project's site; Projects offers Unignore site.
- Over 400 icons for sites and projects, in clear categories.

### Changed & Improved
- Simpler Import databases window; per-mode backgrounds; Editor settings in their own section.
- Steadier Sites, Projects, Runtimes and Databases pages; paged Background Tasks.
- Projects update right away; no needless administrator helper reinstall on upgrade.
- Faster, easier-to-browse icon pickers.
- Automatic fixes happen right away when something changes; Diagnose on the Repair page fixes what it safely can.

### Fixed
- Add Site picks the right folder; correct tray service count; long tooltip paths; import counts; Explorer menu reliability.
- Project commands use the right PHP version; a site's Repair page shows only its own problems; faster saving in Settings and on sites.

## [2.8.0] — 2026-11-03

Feature release. Full notes: [`release-notes/v2.8.0.md`](./release-notes/v2.8.0.md).

### Highlights
- Projects stay standalone by default and can be connected to a site later; Quick App selection is searchable.
- Internal database improvement: read-only SQLite views make project, site, and activity records easier to query; indexes speed up common lookups.
- WordPress core repair restores missing files with checksum validation and offline cache support.
- Automatic operation-history retention and repeated-failure cleanup.
- Sites offer Hide (the only action with Undo), Remove from list, and Safe delete; editor menus use configured editors and migrate existing editor settings once.
- Background Tasks retains recent site-operation history across restarts and groups repeats.
- Site Health Check shows each DNS, TCP, TLS, certificate, trust, and HTTP stage as it completes.

## [2.7.0] — 2026-11-02

Feature release. Full notes: [`release-notes/v2.7.0.md`](./release-notes/v2.7.0.md).

### Highlights
- **Projects Management**: Dedicated Projects page to browse, search, and manage local project folders with or without virtual host domains.
- **Responsive Type Filter Bar**: Single-line filter bar that shows only types with projects, collapses overflow into a "+N more" popover with search, and remembers selected tabs.
- **Custom Project Types**: Create, rename, delete, and drag-to-reorder custom project types, with pinning to the filter bar.
- **Framework Presets & Tauri**: Bundled presets for 17 frameworks and languages, plus native Tauri project auto-detection.
- **Folder Drag-and-Drop & Explorer Menu**: Drop folders directly onto the Projects page or right-click any folder in Windows Explorer to add it to OLS.
- **Multi-Editor Configuration**: Broad auto-detection for IDEs (VS Code, Cursor, Windsurf, Zed, JetBrains, Sublime, Notepad++), with custom editor paths and visibility toggles.
- **System Tray Submenu**: Right-click tray menu includes a categorized Projects submenu to open project directories or launch editors in 1 click.
- **Favorite Quick Apps in Tray**: Create Quick Apps directly from favorites in the tray; Browse all remains available.
- **Theme Contrast & Icons**: High-contrast `--on-accent` styling for active chips and buttons, currentColor icons, and dual contrast warnings in theme settings.
- **MCP Server Tools**: 9 new Model Context Protocol tools for AI assistants to inspect, add, update, and manage projects and editors.
- **Safe Project Removal**: Guards projects connected to active sites against accidental deletion while keeping scanned project skips in history.
- **Shared Save Bar**: One fixed Save/Discard bar for unsaved settings and site changes, with Ctrl+S support.

## [2.6.0] — 2026-11-02

Feature and UI release. Full notes: [`release-notes/v2.6.0.md`](./release-notes/v2.6.0.md).

### Highlights
- **Unified External Database Tools Gallery**: Consolidated database GUI client management into `Databases > External tools`. Features system auto-detection for popular explorers (HeidiSQL, DBeaver, Letos DB Browser, DB Browser for SQLite, pgAdmin 4, NoSQLBooster, Tiny RDM).
- **Visual Arguments Editor (`ToolArgsEditor`)**: Replaced brittle single-line argument strings with dedicated per-argument input boxes. Add or remove arguments without worrying about space quoting or pipe syntax.
- **Multi-Engine & Per-Engine Arguments**: Configure different arguments per database engine (e.g. DBeaver's `mariaDB`, `postgres-jdbc`, and `sqlite_jdbc` connection drivers) with a simple "Same for every engine" vs "Different per engine" toggle.
- **Interactive Placeholder Chips & Command Preview**: Insert dynamic placeholders (`{host}`, `{port}`, `{database}`, `{path}`, `{uri}`) directly into the active argument box, with a live "Runs as: program ..." preview of the final command.
- **JSON Import & Export**: Export external tool configurations (`schemaVersion: 1`) without local machine paths; import configurations with drag-and-drop and duplicate conflict resolution (Skip, Overwrite, Rename).
- **Robust Windows GUI Launching**: Executes tools with their parent directory set as the working directory, ensuring DLLs and plugins load properly. Emits verbatim raw arguments on Windows to prevent Delphi and HeidiSQL dot-truncation bugs.
- **Persistent UI Caching**: Instantaneous 0ms page rendering across application restarts; optimistic live updates for sites, databases, and runtimes without manual page refresh.
- **Dashboard Layout & Controls**: Responsive container (up to 1680px content area), full-width Web server status strip, clean borderless action buttons, and isolated per-service pending states.
- **Unified Notification System**: Centralized toasts with semantic tokens, smart pause/resume on blur/hover, deduplication, auto-resolving condition alerts, startup grace period, live progress for background tasks and runtimes, and a persistent notification history dropdown.
- **State Banners & Clean Confirmations**: Persistent inline banners for active conditions (unapplied web server changes, port collisions); replaced noisy confirmation dialogs for reversible actions with instant Undo toasts.
- **Runtime Updates UI**: Separated installation status from update availability, standardized action buttons, filter tabs, and bulk "Update all".
- **Major Upgrade Protection**: Semver major boundary detection with breaking change confirmation modals.
- **Prerelease Filtering**: Excluded pre-releases from default updates; added channel toggle in Manage panel; Node.js strictly targets LTS.
- **Dynamic Checksum Resolution**: Automatically verifies upstream SHA-256 sidecars on vendor in-place archive rebuilds.
- **Process & Port Fixes**: Serialized per-service start/stop operations with port readiness verification and intent tracking; scoped runtime version deletion guard; fixed Port Checker false positives showing PID 0 as "System Idle Process".

## [2.5.0] — 2026-10-31

Feature release. Full notes: [`release-notes/v2.5.0.md`](./release-notes/v2.5.0.md).

### Highlights
- **Consolidated Missing Folder Alerts**: Replaced repetitive, noisy dashboard diagnostic rows with a single, clean collapsible summary card for all missing directories and offline drives.
- **Batch Actions (`Manage in Sites` & `Ignore all`)**: 1-click **Manage in Sites** button instantly navigates to the Sites table with the `Folder not found` filter pre-selected; 1-click **Ignore all** / **Unignore all** silences or restores alerts in bulk.
- **Intelligent Disconnected Drive Detection**: Detects when an external drive (e.g. `D:\`), USB drive, or network share is offline, accurately reporting *"The drive for <name> is disconnected"* rather than claiming the project was deleted.
- **Physical Path Deduplication**: Cross-references sites and projects by their real disk path, emitting exactly one unified alert (e.g. `chatbot-ta.local (project chatbot-TA)`) instead of duplicate Error + Warning rows.
- **"Folder not found" Sites Tab**: Sites with missing directories or offline external drives are isolated into a dedicated filter tab (`Folder not found`), keeping the Enabled tab strictly operational and providing a 1-click **Locate** button to re-point moved folders.
- **Multi-Domain Project Grouping**: Multiple hostnames sharing a physical project directory are unified into a single project row with primary domain focus and interactive pill badges with status dots for extra domains.
- **Right-Click Actions Menu**: Right-clicking any site row opens the cursor-positioned actions menu, sharing a single unified menu definition with the row's Actions button.
- **Smart Viewport Clamping & Scrolling**: Floating menus automatically detect screen bounds, flip upwards/leftwards when near edges, clear the 36px titlebar, and scroll smoothly inside compact windows without closing on mouse wheel scroll.
- **Token-Based Appearance Customization & Themes**: Dedicated `Settings → Appearance` tab with 4 curated presets (Default, High Contrast, Midnight Slate, Warm Amber), WCAG 2.1 contrast math with auto-readable foregrounds, customizable accent color with swatch/hex picker, monochrome vs colorful sidebar icons, and solid vs ambient gradient backgrounds with 360° angle sliders.
- **Vibrant Navigation & Aurora Themes**: Redesigned sidebar and settings navigation with radiant multi-color SVG gradient icons, active neon indicator bars, and flowing Dark Aurora / Light Aurora ambient backgrounds.
- **Dashboard Services Card Grouping**: Built-in services displayed in a clean, consistent order with color-coded action buttons (start emerald, stop destructive, restart amber, reload sky, logs violet) and flattened unavailable actions to prevent layout shifting.
- **Live System Tray Tooltip**: The Windows notification tray tooltip (`OpenLocalServer — X of Y running`) updates dynamically in real-time as sites and services start, stop, or change health, with matching tray/taskbar status mark indicators.
- **PHP 8.5+ Built-in OPcache & Windows ASLR Fix**: Fixed PHP 8.5 FastCGI worker crash-loop (`Opcode handlers are unusable due to ASLR`) on Windows by configuring `opcache.file_cache` and `opcache.file_cache_fallback=1`. Recognized OPcache as a built-in extension in PHP ≥ 8.5 without requiring `ext/php_opcache.dll`, exposed it to the UI extension toggles, and explicitly set `opcache.enable=0` if toggled off.
- **User-Defined `php.ini` Directives (`overrides.ini` & `conf.d/`)**: Custom PHP settings now persist across restarts. An `overrides.ini` template is automatically created in each PHP version directory (`services/php/<version>/overrides.ini`), and any directives placed in it or inside `conf.d/*.ini` are appended to `php.ini` on every start, surviving automatic ini regenerations.
- **Direct Win32 Administrator Elevation & PATH Independence**: Replaced PowerShell `Start-Process -Verb RunAs` with direct Win32 `ShellExecuteExW` (`runas` verb) for admin helper installation, eliminating intermediate powershell.exe processes, quoting discrepancies, and PATH lookup failures. Resolved built-in terminal and tools failing when `%SystemRoot%\System32` was absent from process PATH.

## [2.4.0] — 2026-10-25

Feature release. Full notes: [`release-notes/v2.4.0.md`](./release-notes/v2.4.0.md).

### Highlights
- **Python Support**: Detects system Python (3.7–3.15) and local virtual environments (`.venv`, `venv`).
- **MCP Server**: Stdio Model Context Protocol integration with tools for domains, web servers, databases, and services.
- **Database Tools**: Table listing and SQL execution for MariaDB, PostgreSQL, and SQLite.
- **User Manual**: Built-in documentation and release notes viewer in `Settings → User Manual`.
- **Updates UI**: Compact in-button loading indicator and lightweight status feedback.
- **Branding**: Dark circular icons for application, installer, and taskbar.


## [2.3.0] — 2026-10-23

Feature release. Full notes: [`release-notes/v2.3.0.md`](./release-notes/v2.3.0.md).

### Added
- **Site Broadcast Controls**: Live broadcast status, one-click public URL copying, and start/stop controls integrated into the Site Domains panel.
- **Dynamic DNS (DDNS)**: Multi-provider DNS synchronization for custom domains (with Cloudflare verified and tested), automated conflict resolution, and router troubleshooting.
- **Automatic Welcome Page**: Automatically generates document root and index file (`index.html` or `index.php`) when adding new sites if none exists.

### Changed & Improved
- **Multi-Interface Web Binding**: Web servers (Nginx, Apache, Caddy) bind to `0.0.0.0` to support LAN requests and router port forwarding.
- **Tunnel Constraints**: Enforced single-provider tunnel instance rules per site and automated duplicate pruning.

### Fixed
- **Host Header Routing**: Forwarded public host headers now properly route to virtual hosts across all web servers without falling through to 404.
- **Domain Alias Confirmation**: Added safety confirmation dialog before deleting domain aliases.

## [2.2.0] — 2026-10-19

Feature release. Full notes: [`release-notes/v2.2.0.md`](./release-notes/v2.2.0.md).

### Added
- In-app updater in Settings (`Settings → Updates`) supporting checking, downloading, and installing verified releases.
- Site logo and icon support in Sites page table rows with direct click-to-edit integration.
- Custom image upload with automatic scaling and local WebP optimization.
- Bundled offline Font Awesome icon catalog with full color customization (swatch picker, color presets, custom hex/CSS values).
- Dedicated "Site logo / icon" tab in Site Settings dialog with live preview.
- Page refresh (F5) preservation: keeps the user on their active page and subtabs instead of resetting to the Dashboard.

### Changed & Improved
- Borderless window styling with rounded corners that seamlessly adapt when maximized or in full screen.
- Streamlined Updates card UX with clear status banners, direct release notes links, and helper service update support.

### Fixed
- Fixed API documentation link in Settings → Local API to point to the master branch.
- Eliminated modal layout shifts when saving site logo settings.

## [2.1.0] — 2026-10-18

Feature release. Full notes: [`release-notes/v2.1.0.md`](./release-notes/v2.1.0.md).

### Added
- Deleting a site or project now moves its folder into an `ols_excluded_sites` subfolder in the same directory (with automated collision resolution) instead of permanent deletion.
- Configurable ignored folders (`projects.ignored_folders`, default `["ols_excluded_sites"]`) in Settings. `ols_excluded_sites` is required and cannot be renamed or deleted; other ignored folders can be added, removed, and renamed (renaming updates disk directories across project roots).
- Renamed the existing skip-list delete method to **Hide/Exclude** in Sites and Settings.
- Subtle `FolderCode` icon for projects with no domain yet instead of the HTML5 brand logo.
- The Sites page opens on **Enabled** and adds a **Disabled** tab, followed by site-type filters and **All**. Status counts follow search, and site status changes move rows to the corresponding tab immediately.
- Enabled, Disabled, and All tabs have distinct colored icons for quick recognition.

### Fixed
- Caddy no longer depends on the shared fixed admin port `127.0.0.1:20019`. It uses a stable localhost admin address derived from its install path for startup, reload, and stop, avoiding conflicts with other Caddy instances.

## [2.0.2] — 2026-10-10

Patch release. Full notes: [`release-notes/v2.0.2.md`](./release-notes/v2.0.2.md).

### Fixed
- Fixed Apache HTTP Server installation failures (checksum mismatch on 2.4.68 and download path resolution error on 2.4.69).

## [2.0.1] — 2026-10-01

Patch release. Full notes: [`release-notes/v2.0.1.md`](./release-notes/v2.0.1.md). No new commands, settings or
data models — this is the bundle import of 2.0.0 made trustworthy on a real site.

### Fixed
- Importing a bundle under the name a site already has no longer creates a second site: rewriting the project slug
  inside its own hostname came out unchanged and fell through to the branch meant for a hostname carrying no
  project name, so `worpress-test` also arrived as `worpress-test.worpress-test`.
- A database the import is about to load into is kept as `<name>_bkp` (`_bkp-2`, `_bkp-3` if those are taken)
  instead of being merged into, and the rename is reported in the site's own report. `ALTER DATABASE ... RENAME TO`
  on PostgreSQL; on MariaDB, which has no `RENAME DATABASE`, a dump-and-copy.
- A WordPress site's database is found from `wp-config.php` (`DB_NAME`, and `DB_HOST`'s port for the engine), so a
  bundle of a WordPress project carries data instead of configuration only.
- Database names may contain `-` and `.`, which is what the WordPress catalog produces from a project slug; quote
  characters, backslashes and spaces are still refused.
- Import and export progress counts what is actually being copied — zip entries while writing, files and dumps
  while reading, plus bytes — so the bar moves instead of sitting still and jumping.
- Database service cards no longer wrap their status pill and push the connection URI past the card edge.

### Changed
- Background tasks have their own page under Monitor; Processes is services and processes only.
- A site can state what a backup of it copies, and a plan that would back up nothing is refused.
- An external DB explorer is recognised for every engine it was registered for.

## [2.0.0] — 2026-10-01

Feature release. Full notes: [`release-notes/v2.0.0.md`](./release-notes/v2.0.0.md).

### Added
- Site bundles: export one or many sites to a single `*.olsbundle.zip` and import it back, with independent
  switches for settings, `.env` files, databases and project files, and an optional password that seals the
  `.env` files and the database dumps. Sites sharing a project travel as one entry. An import previews which
  hostnames clash before anything is created, and `update` takes a "Before import" snapshot first, so it is
  reversible; `rename` adjusts hostname, database and port through the same machinery a clone uses.
- Background tasks: `export_sites` and `import_sites` answer with a task id instead of blocking. The Processes
  page gains a Background tasks tab with per-step progress, cancel, and a per-item report. New `task-event`
  broadcast; new commands `list_tasks`, `cancel_task`, `dismiss_task`, `clear_finished_tasks`,
  `preview_site_bundle`.
- Automatic backups: project snapshots and database dumps on a schedule with a keep count (`auto_backup`,
  `auto_backup_state`). App-wide or chosen-sites scope; under chosen sites each site sets its own period, keep
  count and copy set. Retention only deletes paths a pass recorded, so a manual backup is never removed.
  New commands `get_auto_backup`, `set_auto_backup`, `set_site_auto_backup`, `run_auto_backup`.
- Desktop notifications for Windows: app-owned toasts for crashed processes, finished installs, finished Quick
  Apps, completed auto-fixes and hidden launches. Off by default (`notifications.enabled`).
- `TechIcon`/`ServiceMark`: one shared service mark across the Dashboard, Services and Databases banner.

### Changed
- The project page holding snapshots is Backups, not Snapshots — a database dump belongs next to the snapshot
  that records its database.
- A site's backup on/off switch is shown greyed with the way out while the app-wide plan covers everything,
  rather than reporting success and changing nothing.
- Tray service rows carry state as a `●`/`○` glyph in the label (Win32 renders a menu bitmap over the item text).

### Fixed
- Backup and Restore spinners are keyed per row, so backing up one database no longer spins every sibling's
  button.
- Desktop alerts wear the app's AppUserModelID and icon instead of PowerShell's.

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
