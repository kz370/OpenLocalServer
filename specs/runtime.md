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
- Cargo.toml, Cargo.lock, dev.bat, build-installer.bat, installer/open-local-server.iss, scripts/render-icon.mjs

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
- [2026-09-27] fix: port holder lookup no longer surfaces tasklist INFO text as process name (stale-PID race), holder cache TTL 15s→5s; Services shows live holder + retry hint; Dashboard services rows regain per-service Start/Stop; Mailpit Open uses open_url; diagnostics auto-rescan
- [2026-09-27] fix: check_port names Windows excluded port ranges (Hyper-V/Docker netsh) so busy-port-with-no-listener reports cause, not ghost holder
- [2026-09-27] fix: netstat owner parse accepts localized/non-LISTENING 5-col TCP rows (LISTENING preferred, fallback otherwise) so holders show on non-English Windows
- [2026-09-27] feat: Redis one-click — Tiny RDM auto-detect (Program Files/LOCALAPPDATA/PATH) + Open in button on Databases Redis tab with URI copy; RedisInsight excluded per request
- [2026-09-27] fix: open_database routes tinyrdm (explicit + redis fallback) to detected Tiny RDM exe — was falling through to "No tool is registered for redis"
- [2026-09-27] style: Runtimes list + Version Manager visual refine only (hierarchy, spacing, status dots, compact Manage, balanced Add cards, default highlight, ghost secondary actions); no behavior / IPC change; Files Processed still 202
- [2026-09-27] style: Add Site / Quick Apps / Create Quick App one design system — shared form modal width (max-w-2xl), shared FormSection, gap-5 sections + gap-3 grids, compact Quick App cards, equal Serves/Options cards, h-9 Browse shrink-0, footer via Dialog prop, icon errors; no behavior / IPC / order change; Files Processed still 202

Status: Completed 100%

Documentation ready in /specs

All text written in English.
