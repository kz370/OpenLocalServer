# RUNTIME

## Files to Process
Total meaningful source files: 202 (excluding target/, node_modules/, .git/, dist/, data/services/postgres runtime data, icon bulk).

### Rust core (crates/ols-core/src, 73 files)
- src/lib.rs, app.rs, command.rs, control.rs, api.rs, paths.rs, settings.rs, secrets.rs
- src/process.rs, port.rs, exec.rs, error.rs, logging.rs, redact.rs, diagnostics.rs, repair.rs, search.rs, support.rs, network.rs, journal.rs, resources.rs, elevate.rs, test_support.rs
- src/catalog.rs, catalogs.rs, runtime.rs, php.rs, nodepm.rs, composer.rs, venv.rs, custom_install.rs, resolver.rs, detection.rs, plugin.rs, updater.rs
- src/service.rs, custom_service.rs, dbtools.rs, mail.rs, monitor.rs, workers.rs, scheduler.rs
- src/web/mod.rs, web/manager.rs, web/nginx.rs, web/apache.rs, web/caddy.rs
- src/domain.rs, ca.rs, certs.rs, hosts.rs, dns.rs, health.rs, tunnel.rs, inspector.rs
- src/dbbackup.rs, sqlite.rs, migrate.rs
- src/project.rs, project_tools.rs, manifest.rs, profiles.rs, setup.rs, snapshots.rs, envfile.rs, command_catalog.rs, git.rs, shortcuts.rs, editors.rs, terminal.rs, shell_menu.rs, procfile.rs, loadtest.rs, xdebug.rs
- src/quickapp/mod.rs, quickapp/schema.rs, quickapp/plan.rs, quickapp/catalog.rs, quickapp/run.rs, quickapp/commands.rs
- Cargo.toml; catalog/plugins/bun.yaml, dotnet.yaml, go.yaml, java.yaml; catalog/quick-apps/*.yaml (13); catalog/quick-commands.yaml; examples/smoke.rs, smoke_web.rs, smoke_services.rs, timing.rs

### CLI / Helper / Tauri (10 files)
- crates/ols-cli/src/main.rs, crates/ols-cli/Cargo.toml
- crates/ols-helper/src/main.rs, crates/ols-helper/src/service.rs, crates/ols-helper/Cargo.toml
- src-tauri/src/lib.rs, src-tauri/src/main.rs, src-tauri/Cargo.toml, src-tauri/tauri.conf.json, src-tauri/capabilities/default.json

### Frontend (ui/src, ~85 files)
- package.json, vite.config.ts, index.html, tsconfig*.json, src/main.tsx, src/App.tsx, src/core.ts, src/index.css
- src/pages/Dashboard.tsx, Sites.tsx, QuickApps.tsx, Commands.tsx, WebServer.tsx, Config.tsx, Tunnels.tsx, Databases.tsx, Services.tsx, Runtimes.tsx, Profiles.tsx, Plugins.tsx, Logs.tsx, Processes.tsx, Settings.tsx
- src/components/layout/Sidebar.tsx, layout/Titlebar.tsx, CommandPalette.tsx, ConfirmHost.tsx, ai/AiHost.tsx, ai/AiButton.tsx, ai/AiSettings.tsx
- src/components/site/SiteDialog.tsx, site/DomainDialog.tsx, site/ServersPanel.tsx, site/SiteLogs.tsx, site/HtaccessEditor.tsx, site/WebApply.tsx
- src/components/project/EnvironmentPanel.tsx, project/GitPanel.tsx, project/LoadPanel.tsx, project/ProjectCommands.tsx, project/ProjectImports.tsx, project/RepairPanel.tsx, project/SnapshotsPanel.tsx, project/WorkersPanel.tsx
- src/components/ProjectTools.tsx, ReleaseCards.tsx, SettingsExtras.tsx, CustomServices.tsx, DiagnosticsCard.tsx, DoctorDialog.tsx, EnvEditor.tsx, CodeEditor.tsx, Terminal.tsx, SystemMonitor.tsx, MigrateDialog.tsx, OpenWithMenu.tsx, PhpExtensionsDialog.tsx, XdebugDialog.tsx, ErrorCard.tsx, Spinner.tsx, StopIcon.tsx, TechIcon.tsx
- src/components/ui/button.tsx, card.tsx, dialog.tsx, input.tsx, form.tsx, table.tsx, menu.tsx, switch.tsx, badge.tsx, checkbox.tsx
- src/lib/ai.ts, confirm.ts, hooks.ts, nav.ts, theme.tsx, utils.ts, wait.ts, web.ts

### Docs / Config / Build (20 files)
- README.md, CONTRIBUTING.md, SECURITY.md, CHANGELOG.md, CODE_OF_CONDUCT.md, OpenLocalServer_Master_SRS_v4.md
- docs/API.md, IMPLEMENTATION_PLAN.md, STATUS.md, USER_GUIDE.md, PLUGINS.md, SECURITY_REVIEW.md, LOAD_TESTING.md, AI_ASSISTANT.md, UX polish batch doc
- Cargo.toml, Cargo.lock, scripts/dev.bat, scripts/build-installer.bat, scripts/build-inno-installer.bat, scripts/upload-release.bat, installer/open-local-server.iss, scripts/render-icon.mjs

## Files Processed
All 202 files listed above processed via 4 parallel analysis agents (rust-core, UI frontend, tauri-cli-helper, docs-catalogs) on 2026-09-26.

## Log
- [2026-09-26] Initialization completed
- [2026-09-26] File discovery completed (25881 raw files, 202 meaningful after excluding target/node_modules/.git/dist/data-postgres/icons)
- [2026-09-26] Parallel deep analysis completed (4 agents)
- [2026-09-26] Architecture overview written
- [2026-09-26] Full documentation written
- [2026-09-26] Catalog / relationships / data models written
- [2026-09-26] Diagrams written
- [2026-09-26] Developer guide + API reference written
- [2026-09-26] Coverage + consistency + English checks passed
- [2026-09-26] Specs-sync mandate added: AGENTS.md created, CONTRIBUTING.md patched — code change requires specs update in same PR
- [2026-09-27] feat: Settings About section (AboutCard: name, backend ping version, GPL-3.0-only, stack) + version 1.0.0 (workspace Cargo, tauri.conf, README badge); no IPC change; catalog SettingsPage line updated; Files Processed still 202
- [2026-09-27] fix: port holder lookup no longer surfaces tasklist INFO text as process name (stale-PID race), holder cache TTL 15s→5s; Services shows live holder + retry hint; Dashboard services rows regain per-service Start/Stop; Mailpit Open uses open_url; diagnostics auto-rescan
- [2026-09-27] fix: check_port names Windows excluded port ranges (Hyper-V/Docker netsh) so busy-port-with-no-listener reports cause, not ghost holder
- [2026-09-27] fix: netstat owner parse accepts localized/non-LISTENING 5-col TCP rows (LISTENING preferred, fallback otherwise) so holders show on non-English Windows
- [2026-09-27] feat: Redis one-click — Tiny RDM auto-detect (Program Files/LOCALAPPDATA/PATH) + Open in button on Databases Redis tab with URI copy; RedisInsight excluded per request
- [2026-09-27] fix: open_database routes tinyrdm (explicit + redis fallback) to detected Tiny RDM exe — was falling through to "No tool is registered for redis"
- [2026-09-27] style: Runtimes list + Version Manager visual refine only (hierarchy, spacing, status dots, compact Manage, balanced Add cards, default highlight, ghost secondary actions); no behavior / IPC change; Files Processed still 202
- [2026-09-27] fix: Node install froze page — download loop emitted Progress per HTTP chunk (thousands/sec over Tauri into React setState). Added ProgressThrottle (first + 200ms/256KiB + final chunk) + 250ms frontend coalesce; unit test progress_throttle_coalesces_chunk_flood
- [2026-09-27] fix: Node install rejected as "not in the current online or built-in version list" — backend online_versions is memory-only (daemon restart wipes it) while UI localStorage cache persists, so cached choices failed validation and the error flashed away on refresh. Backend install() now does one on-demand online refresh before rejecting (reason appended); frontend tracks backend-verified keys, offers only verified versions, always re-verifies on dialog open, and keeps failure in sticky installError state with retry affordance
- [2026-09-27] feat: version select lists full verified catalog with installed rows disabled + "· Installed" suffix (newest free version preselected); renamed ghost "Use" to prominent secondary "Set default" for global default control
- [2026-09-27] style: prevent chrome text selection app-wide (body select-none); opt-in allowlist: inputs, pre/code, xterm, CodeMirror, data-selectable
- [2026-09-27] fix: Dashboard overlap in narrow windows — header toolbar, donut row, traffic stats, finding rows now wrap (grid-cols-2 base); removed unused var blocking tsc build
- [2026-09-27] fix: Traffic widget stats crushed in 4 columns (narrow card) — 2×2 grid, nowrap values, 4 unique y-ticks
- [2026-09-27] docs: README tour uses 7 webp shots (dashboard, sites, runtimes, version-manager, databases, webserver, tunnels)
- [2026-09-27] fix: removed projects returned to Sites list — folder rescans re-registered them with no opt-out. Added projects.removed skip-list (remove_project records canonical path; sync/scan honor it; explicit Add-folder clears it) + regression test removed_projects_stay_removed_across_rescans
- [2026-09-27] style: Add Site / Quick Apps / Create Quick App one design system — shared form modal width (max-w-2xl), shared FormSection, gap-5 sections + gap-3 grids, compact Quick App cards, equal Serves/Options cards, h-9 Browse shrink-0, footer via Dialog prop, icon errors; no behavior / IPC / order change; Files Processed still 202
- [2026-09-27] feat: remove Tailscale Funnel tunnel provider — struct + registration + URL matcher + UI program map cut; providers now cloudflare/ngrok/localtunnel (+mock in tests); saved tailscale tunnels report unknown provider on start; specs (catalog/architecture/data_models) + docs STATUS/IMPLEMENTATION_PLAN updated; Files Processed still 202
- [2026-09-27] feat: Logs Ask AI picks error/warn lines — picker dialog (Errors/Warns/Both tabs, w-60 filter), timestamp-blind dedupe with ×count badge, char counter, over ~12k chars spills to .log text file via new read_excerpt tool; docs AI_ASSISTANT + catalog updated; Files Processed still 202
- [2026-09-27] fix: build-installer.bat rewrite (5 stages, tool + running-app preflight, always-install deps, cargo-bin/staged-exe split, copy retry with ping wait, ISCC v6/v7 detect, certutil SHA-256 verify + summary); fixed script bugs found live: missing backslash in package.json check, parens in block echoes killing cmd parsing, timeout.exe headless fail, Get-FileHash absent on old PowerShell; full run verified EXIT=0; Files Processed still 202
- [2026-09-27] fix: upload-release.bat retargeted from Open-Local-Assistant to OpenLocalServer (setup pattern, portable exe+helper+dlls, space-tolerant version parse, SHA256SUMS upload); installer/open-local-server.iss enables dir choice (DisableDirPage=no, lowest privileges + override dialog, program-group page); ISCC compile verified; Files Processed still 202
- [2026-09-27] fix: Logs.tsx missing dedupeKey impl broke tsc build — added exported dedupeKey (strip ISO/HH:MM:SS timestamps, collapse ws, lowercase); restores documented timestamp-blind dedupe, no behavior / IPC change; Files Processed still 202
- [2026-09-27] fix: "can't reach this page" root cause found and fixed -- src-tauri/Cargo.toml was missing `[features] default=["custom-protocol"] custom-protocol=["tauri/custom-protocol"]`; tauri-macros uses `cfg!(not(feature="custom-protocol"))` to set `dev=true` in generate_context!(), skipping asset embedding when the feature is absent; added the feature block, plain `cargo build --release` now embeds ui/dist/ correctly; Files Processed still 202
- [2026-09-27] fix: release-only UI, first attempt (superseded) — pinned CodeEditor/DiffView content color to theme foreground, terminal lineHeight 1.3 + windowsMode, Config load fallback message; no IPC change; ui lint 0 errors + vite build green
- [2026-09-27] fix: release-only UI, true root cause — Tauri CSP had no style-src, so prod (custom protocol enforces CSP; dev Vite server does not) blocked every runtime-injected <style> tag and React inline style= attributes. CodeMirror/MergeView ship 100% of their CSS via JS-injected styles → unstyled scaffold (bare gutter numbers, spilling plain text). Added style-src 'self' 'unsafe-inline' to src-tauri/tauri.conf.json; scripts stay 'self'. Rebuild release to pick up; Files Processed still 202
- [2026-09-27] fix: terminal looked empty/dead — explicit xterm theme (bright foreground, green bar cursor, selection tint) instead of WebView2-dependent defaults, plus "[starting shell…]" hint during slow runtime-resolving startup, cleared on first real shell output/exit; prompt path (pty → event → xterm) verified alive in backend/frontend wiring; Files Processed still 202
- [2026-09-27] fix: build-exe-installer.bat was broken — STAGED_* paths missed the backslash after %DIST% (staged into repo root as "releaseOpen Local Server.exe", ISCC SourceExe pointed nowhere), LibDir pointed at the (mis-staged) release dir instead of target\release like build-installer.bat, no copy retry on AV locks, no running-app guard. Rewrote to match proven build-installer.bat patterns (retry copy, tasklist preflight, LibDir=target\release, sizes + certutil SHA-256 summary); smoke run correctly refused while app running; Files Processed still 202
- [2026-09-27] chore: moved dev.bat, build-installer.bat, build-inno-installer.bat (renamed from build-exe-installer.bat), upload-release.bat into scripts/; fixed %~dp0 roots to repo root, updated cross-refs + README/CONTRIBUTING/specs file list; Files Processed still 202
- [2026-09-27] fix: updater pointed at wrong repo (openlocalserver/openlocalserver → kz370/OpenLocalServer in updater.rs default, upload manifest URL, Cargo.toml, README badge, AI referer); upload-release.bat now generates/signs/uploads latest.json + .minisig per release (minisign + key required, else skipped with warning); fixed batch paren-in-echo parse crash and missing manifest in upload line; manifest notes read via .NET (PS Get-Content wrapped it as object, breaking serde); commit-message.txt no longer required; Files Processed still 202

Status: Completed 100%

Documentation ready in /specs

All text written in English.
