# Comprehensive Safe Refactoring Plan: Modularizing Files > 500 Lines in OpenLocalServer

## 1. Executive Summary & Objective

In accordance with user requirements, the refactoring scope has been expanded from only files exceeding 4,000 lines to **all codebase files exceeding 500 lines**. A 500-line ceiling ensures tight cohesion, single-responsibility modules, readable navigation, and simple maintenance across both Rust crates and TypeScript UI layers.

### Non-Negotiable Requirements
1. **Safe & Non-Destructive**: Zero behavioral, IPC wire format, or public API contract changes.
2. **Comment Hygiene**: Strip all bloated essay-like architectural commentaries. Replace with concise, 1-2 sentence developer docstrings explaining intent.
3. **Spec Synchronization (`AGENTS.md`)**: Keep `specs/full_documentation.md`, `specs/catalog.txt`, `specs/relationships.txt`, and `specs/runtime.md` updated as files are modularized.
4. **Safety Rule**: Never run `cargo test` while `cargo tauri dev` is actively running.

---

## 2. Complete Codebase Audit: Files Exceeding 500 Lines

A full audit of the project source tree (`crates/`, `src-tauri/`, `ui/src/`) identified **69 source files** exceeding 500 lines, categorized into 4 priority tiers:

### Tier 1: Original Monoliths (>4,000 lines) — *Completed / In Final Polish*
| Original File | Line Count | Target Modular Architecture | Status |
|---|---|---|---|
| [`crates/ols-core/src/command.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/command.rs) | **5,719** | Split into `crates/ols-core/src/command/` (`mod.rs`, `types.rs`, `dispatch.rs`, `tests.rs`) | **Extracted & Test-Verified** (26/26 passed) |
| [`crates/ols-core/src/app.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/app.rs) | **5,402** | Split into `crates/ols-core/src/app/` (`mod.rs`, `domains.rs`, `projects.rs`, `web.rs`, `quickapp.rs`, `system.rs`, `tests.rs`) | **Extracted & Test-Verified** (34/34 passed) |

---

### Tier 2: Heavyweight Source Files (2,000 – 3,000 lines)
These files represent critical business logic that has accumulated excessive responsibilities over time:

| File | Lines | Current Responsibilities | Proposed Modular Split Plan |
|---|---|---|---|
| [`crates/ols-core/src/runtime.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/runtime.rs) | **2,837** | Runtime manager, downloader, unpacker, verification, system detection, path resolution, events | `runtime/` (`mod.rs`, `manager.rs`, `detect.rs`, `download.rs`, `unpack.rs`, `events.rs`, `tests.rs`) |
| [`crates/ols-core/src/ai.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/ai.rs) | **2,794** | Ollama client, model pull/catalog, prompts, chat completions, code generation, embeddings | `ai/` (`mod.rs`, `ollama.rs`, `models.rs`, `prompts.rs`, `chat.rs`, `tests.rs`) |
| [`crates/ols-core/src/command/dispatch.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/command/dispatch.rs) | **2,643** | Giant dispatch match block handling all command categories | `command/dispatch/` (`mod.rs`, `system.rs`, `services.rs`, `web.rs`, `domains.rs`, `projects.rs`, `runtimes.rs`, `tools.rs`) |
| [`crates/ols-cli/src/main.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-cli/src/main.rs) | **2,368** | CLI command parsing, terminal formatting, interactive prompts, IPC bridge | `ols-cli/src/` (`main.rs`, `cli.rs`, `format.rs`, `commands/` [subcommands]) |
| [`crates/ols-core/src/domain.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/domain.rs) | **2,362** | Domain store, hosts file manipulation, DNS, SSL bindings, site types, validation | `domain/` (`mod.rs`, `store.rs`, `hosts.rs`, `ssl.rs`, `types.rs`, `validation.rs`, `tests.rs`) |
| [`ui/src/core.ts`](file:///I:/Development/repos/OpenLocalServer/ui/src/core.ts) | **2,257** | Tauri IPC wrappers, state management, event listeners, command definitions | `ui/src/core/` (`index.ts`, `ipc.ts`, `events.ts`, `commands.ts`, `types.ts`) |
| [`crates/ols-core/src/web/manager.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/web/manager.rs) | **2,221** | Web server manager, Apache/Nginx/Caddy orchestration, config template generation | `web/manager/` (`mod.rs`, `apply.rs`, `validation.rs`, `templates.rs`, `ports.rs`) |
| [`crates/ols-core/src/setup.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/setup.rs) | **2,141** | First-run setup, default runtimes, hosts configuration, CA certificate bootstrap | `setup/` (`mod.rs`, `bootstrap.rs`, `ca.rs`, `defaults.rs`, `installer.rs`) |

---

### Tier 3: Moderate Monoliths (1,000 – 2,000 lines)

#### Rust Modules:
| File | Lines | Proposed Modular Split Plan |
|---|---|---|
| [`crates/ols-core/src/snapshots.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/snapshots.rs) | 1,881 | Split into `snapshots/` (`mod.rs`, `create.rs`, `restore.rs`, `storage.rs`, `manifest.rs`) |
| [`crates/ols-core/src/command/types.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/command/types.rs) | 1,828 | Split commands into `command/types/` (`commands.rs`, `responses.rs`, `models.rs`) |
| [`crates/ols-core/src/auto_backup.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/auto_backup.rs) | 1,813 | Split into `auto_backup/` (`mod.rs`, `engine.rs`, `retention.rs`, `scheduler.rs`) |
| [`crates/ols-mcp/src/lib.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-mcp/src/lib.rs) | 1,580 | Split into `ols-mcp/src/` (`lib.rs`, `protocol.rs`, `tools.rs`, `prompts.rs`, `resources.rs`) |
| [`crates/ols-core/src/service.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/service.rs) | 1,517 | Split into `service/` (`mod.rs`, `supervisor.rs`, `windows.rs`, `discovery.rs`) |
| [`crates/ols-core/src/sitebundle.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/sitebundle.rs) | 1,505 | Split into `sitebundle/` (`mod.rs`, `archive.rs`, `export.rs`, `import.rs`) |
| [`crates/ols-core/src/loadtest.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/loadtest.rs) | 1,501 | Split into `loadtest/` (`mod.rs`, `engine.rs`, `metrics.rs`, `reporter.rs`) |
| [`crates/ols-core/src/ddns.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/ddns.rs) | 1,436 | Split into `ddns/` (`mod.rs`, `providers/` [cloudflare, duckdns, route53, noip]) |
| [`crates/ols-core/src/app/system.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/app/system.rs) | 1,293 | Split into `app/system/` (`health.rs`, `registry.rs`, `logs.rs`, `dbtools.rs`) |
| [`crates/ols-core/src/git.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/git.rs) | 1,287 | Split into `git/` (`mod.rs`, `repo.rs`, `commit.rs`, `branch.rs`, `remote.rs`) |
| [`crates/ols-core/src/tunnel.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/tunnel.rs) | 1,280 | Split into `tunnel/` (`mod.rs`, `ngrok.rs`, `localtunnel.rs`, `cloudflare.rs`) |
| [`crates/ols-core/src/quickapp/plan.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/quickapp/plan.rs) | 1,278 | Split into `quickapp/plan/` (`mod.rs`, `analyzer.rs`, `resolver.rs`, `builder.rs`) |
| [`crates/ols-core/src/command/tests.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/command/tests.rs) | 1,146 | Partition test cases across relevant domain submodules |
| [`src-tauri/src/lib.rs`](file:///I:/Development/repos/OpenLocalServer/src-tauri/src/lib.rs) | 1,093 | Split into `src-tauri/src/` (`lib.rs`, `tray.rs`, `menu.rs`, `commands.rs`, `window.rs`) |
| [`crates/ols-core/src/app/quickapp.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/app/quickapp.rs) | 1,007 | Split into `app/quickapp/` (`host.rs`, `requirements.rs`, `runner.rs`) |

#### UI Components & Pages:
| File | Lines | Proposed Modular Split Plan |
|---|---|---|
| [`ui/src/components/site/SiteDomainsPanel.tsx`](file:///I:/Development/repos/OpenLocalServer/ui/src/components/site/SiteDomainsPanel.tsx) | 1,860 | Decompose into subcomponents: `DomainRow.tsx`, `DomainActions.tsx`, `DomainSslBadge.tsx`, `BulkDomainBar.tsx` |
| [`ui/src/components/site/DomainDialog.tsx`](file:///I:/Development/repos/OpenLocalServer/ui/src/components/site/DomainDialog.tsx) | 1,384 | Decompose into tab panels: `GeneralTab.tsx`, `SslTab.tsx`, `RoutingTab.tsx`, `AdvancedTab.tsx` |
| [`ui/src/pages/Commands.tsx`](file:///I:/Development/repos/OpenLocalServer/ui/src/pages/Commands.tsx) | 1,142 | Extract `CommandPalette.tsx`, `CommandHistoryList.tsx`, `CommandOutputView.tsx` |
| [`ui/src/pages/Sites.tsx`](file:///I:/Development/repos/OpenLocalServer/ui/src/pages/Sites.tsx) | 1,120 | Extract `SiteList.tsx`, `SiteCard.tsx`, `SiteFilterBar.tsx`, `CreateSiteModal.tsx` |

---

### Tier 4: Files Between 500 – 1,000 Lines

#### Rust Modules (to refactor into focused submodules):
- `crates/ols-core/src/plugin.rs` (969 lines) → `plugin/` (`mod.rs`, `loader.rs`, `manifest.rs`, `sandbox.rs`)
- `crates/ols-core/src/app/tests.rs` (944 lines) → split across matching sub-tests
- `crates/ols-core/src/quickapp/run.rs` (943 lines) → `quickapp/run/` (`mod.rs`, `exec.rs`, `status.rs`)
- `crates/ols-core/src/app/domains.rs` (910 lines) → `app/domains/` (`crud.rs`, `ssl.rs`, `bulk.rs`)
- `crates/ols-core/src/php.rs` (908 lines) → `php/` (`mod.rs`, `pools.rs`, `extensions.rs`, `ini.rs`)
- `crates/ols-core/src/process.rs` (897 lines) → `process/` (`mod.rs`, `supervisor.rs`, `windows.rs`)
- `crates/ols-core/src/tasks.rs` (833 lines) → `tasks/` (`mod.rs`, `queue.rs`, `worker.rs`)
- `crates/ols-core/src/repair.rs` (808 lines) → `repair/` (`mod.rs`, `rules.rs`, `actions.rs`)
- `crates/ols-core/src/migrate.rs` (784 lines) → `migrate/` (`mod.rs`, `v1.rs`, `v2.rs`, `importer.rs`)
- `crates/ols-core/src/quickapp/catalog.rs` (745 lines) → `quickapp/catalog/` (`mod.rs`, `builtin.rs`, `parser.rs`)
- `crates/ols-core/src/command_catalog.rs` (732 lines) → `command_catalog/` (`mod.rs`, `metadata.rs`, `docs.rs`)
- `crates/ols-core/src/wordpress.rs` (679 lines) → `wordpress/` (`mod.rs`, `cli.rs`, `config.rs`, `detector.rs`)
- `crates/ols-core/src/profiles.rs` (642 lines) → `profiles/` (`mod.rs`, `store.rs`, `apply.rs`)
- `crates/ols-core/src/detection.rs` (637 lines) → `detection/` (`mod.rs`, `stack.rs`, `rules.rs`)
- `crates/ols-core/src/app/projects.rs` (633 lines) → `app/projects/` (`scan.rs`, `skip.rs`, `watcher.rs`)
- `crates/ols-core/src/web/nginx.rs` (624 lines) → `web/nginx/` (`mod.rs`, `parser.rs`, `generator.rs`)
- `crates/ols-core/src/inspector.rs` (617 lines) → `inspector/` (`mod.rs`, `system.rs`, `network.rs`)
- `crates/ols-core/src/api.rs` (611 lines) → `api/` (`mod.rs`, `routes.rs`, `middleware.rs`)
- `crates/ols-core/src/resources.rs` (609 lines) → `resources/` (`mod.rs`, `monitor.rs`, `limits.rs`)
- `crates/ols-core/src/dbtools.rs` (607 lines) → `dbtools/` (`mod.rs`, `discovery.rs`, `drivers.rs`)
- `crates/ols-core/src/web/mod.rs` (568 lines) → trim and extract helpers
- `crates/ols-core/src/envfile.rs` (542 lines) → `envfile/` (`mod.rs`, `parser.rs`, `writer.rs`)
- `crates/ols-core/src/catalog.rs` (537 lines) → `catalog/` (`mod.rs`, `store.rs`, `version.rs`)
- `crates/ols-core/src/quickapp/schema.rs` (532 lines) → `quickapp/schema/` (`types.rs`, `validation.rs`)
- `crates/ols-core/src/acme.rs` (523 lines) → `acme/` (`client.rs`, `challenge.rs`, `cert.rs`)
- `crates/ols-core/src/project_tools.rs` (516 lines) → `project_tools/` (`mod.rs`, `runners.rs`)
- `crates/ols-core/src/scheduler.rs` (509 lines) → `scheduler/` (`mod.rs`, `cron.rs`, `job.rs`)

#### UI Pages & Components:
- `ui/src/pages/Tunnels.tsx` (902 lines) → extract `TunnelList.tsx`, `TunnelConfigModal.tsx`
- `ui/src/pages/QuickApps.tsx` (901 lines) → extract `AppCatalogGrid.tsx`, `AppInstallModal.tsx`
- `ui/src/pages/Databases.tsx` (848 lines) → extract `DatabaseList.tsx`, `DbConnectionModal.tsx`
- `ui/src/pages/Runtimes.tsx` (756 lines) → extract `RuntimeCard.tsx`, `RuntimeVersionPicker.tsx`
- `ui/src/pages/Config.tsx` (747 lines) → extract `ConfigEditor.tsx`, `ConfigDiffViewer.tsx`
- `ui/src/pages/Dashboard.tsx` (726 lines) → extract `StatWidgets.tsx`, `ServiceStatusBar.tsx`
- `ui/src/components/SettingsExtras.tsx` (686 lines) → extract individual setting section panels
- `ui/src/pages/Settings.tsx` (674 lines) → extract tab panels into `settings/`
- `ui/src/components/project/LoadPanel.tsx` (646 lines) → extract `LoadConfig.tsx`, `LoadCharts.tsx`
- `ui/src/components/project/GitPanel.tsx` (624 lines) → extract `BranchSelector.tsx`, `CommitList.tsx`
- `ui/src/components/project/BackupsPanel.tsx` (556 lines) → extract `BackupCard.tsx`, `RestoreDialog.tsx`
- `ui/src/components/site/SiteLogoPanel.tsx` (538 lines) → extract `IconPicker.tsx`, `FaviconGenerator.tsx`
- `ui/src/components/ReleaseCards.tsx` (533 lines) → extract `ChangelogItem.tsx`, `UpdateAction.tsx`
- `ui/src/pages/WebServer.tsx` (513 lines) → extract `PortStatusGrid.tsx`, `VirtualHostCard.tsx`

---

## 3. Systematic Execution Roadmap

Refactoring is executed in sequential, atomic phases. After each file group is refactored:
1. Verify compiler & build: `cargo check` and `cd ui && npm run build`.
2. Clean doc comments of unnecessary narrative bloat.
3. Update `specs/` (documentation, catalog, relationships, and runtime log).
4. Run `cargo test` only when `cargo tauri dev` is inactive.

### Sequential Phases
1. **Phase 1: Polish Tier 1 Monoliths & Sub-Dispatch**
   - Further partition `crates/ols-core/src/command/dispatch.rs` (2,643 lines) into domain dispatch handlers (`command/dispatch/*.rs`).
   - Finalize `crates/ols-core/src/app/` submodule splits.
2. **Phase 2: Heavyweight Core Infrastructure (Tier 2)**
   - Modularize `runtime.rs` (2,837 lines) → `runtime/`.
   - Modularize `ai.rs` (2,794 lines) → `ai/`.
   - Modularize `domain.rs` (2,362 lines) → `domain/`.
   - Modularize `web/manager.rs` (2,221 lines) → `web/manager/`.
   - Modularize `setup.rs` (2,141 lines) → `setup/`.
   - Modularize `ols-cli/src/main.rs` (2,368 lines) → `ols-cli/src/`.
3. **Phase 3: Frontend Architecture Core & Key Dialogs (Tier 2/3 UI)**
   - Modularize `ui/src/core.ts` (2,257 lines) → `ui/src/core/`.
   - Decompose `ui/src/components/site/SiteDomainsPanel.tsx` (1,860 lines).
   - Decompose `ui/src/components/site/DomainDialog.tsx` (1,384 lines).
   - Modularize `ui/src/pages/Commands.tsx` & `ui/src/pages/Sites.tsx`.
4. **Phase 4: Moderate Monoliths (Tier 3)**
   - Refactor `snapshots.rs`, `auto_backup.rs`, `service.rs`, `sitebundle.rs`, `loadtest.rs`, `ddns.rs`, `git.rs`, `tunnel.rs`, `quickapp/plan.rs`, and `src-tauri/src/lib.rs`.
5. **Phase 5: Files 500 – 1,000 Lines (Tier 4)**
   - Refactor remaining 27 core files and 14 UI files under the 500-line limit.

---

## 4. Verification & Quality Gates

Each refactored module must strictly pass:
```text
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper   # Only when tauri dev is not running
cd ui && npm run lint && npm run build
```
And synchronize with:
- `specs/full_documentation.md`
- `specs/catalog.txt`
- `specs/relationships.txt`
- `specs/runtime.md`
