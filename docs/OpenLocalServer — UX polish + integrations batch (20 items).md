# OpenLocalServer — UX polish + integrations batch (20 items)

## Context
User feedback list after using the app: modals feel abrupt, several pages look dated or misaligned
(settings, runtimes, web config history, databases, sidebar, commands), tray menu is minimal, no
live "sites" folder, missing DB tools (HeidiSQL for Postgres, NoSQLBooster for Mongo), diagnostics
need manual clicks, git clone lacks auth, plus new features (Procfile, .htaccess editing with AI,
real public domain) and a docs change (Linux = Ubuntu + Debian only).

Decisions made with user:
- Real domain → **Cloudflare named tunnel** (IP-independent, no port forwarding).
- Sites folder → **`<install dir>\sites`**, fallback `~/Sites` when install dir not writable; watched live.
- Diagnostics → **auto-run safe (non-destructive) fixes**, destructive still ask, plus "Fix all".
- Icon → **stacked layers / globe mark**, rendered through existing pipeline.

Work in 6 phases, one commit per item (conventional commits, LF line endings — see memory).

---

## Phase 1 — UI primitives & page polish

**1. Animated modals** — [ui/src/components/ui/dialog.tsx](ui/src/components/ui/dialog.tsx)
- Replace `if (!open) return null` with small `usePresence(open, 160)` hook (keeps mounted during exit, sets `data-state=open|closed`).
- Add keyframes in [ui/src/index.css](ui/src/index.css): backdrop fade, panel zoom `scale(.96)→1` + fade, 160ms ease-out; opacity/transform only (GPU, no lag), `will-change: transform`, drop `backdrop-blur` during animation (blur is the lag source), `prefers-reduced-motion` disables.
- Apply same classes to hand-rolled modals: [SiteDialog.tsx:171](ui/src/components/site/SiteDialog.tsx#L171), [CommandPalette.tsx:240](ui/src/components/CommandPalette.tsx#L240).

**2. Switch instead of checkboxes**
- New `ui/src/components/ui/switch.tsx` (`button role="switch" aria-checked`, sm/md sizes, keyboard).
- Rewrite `Toggle` in [form.tsx:23](ui/src/components/ui/form.tsx#L23) to render Switch (label/hint left, switch right) → all 40 usages upgrade at once.
- New styled `ui/checkbox.tsx` for real multi-select lists; replace the 6 raw `<input type=checkbox>` (Plugins:237, Commands:807, AiHost:261, LoadPanel:441/571, RepairPanel:120) with Switch (on/off options) or Checkbox (row selection).

**3. Settings page redesign** — [ui/src/pages/Settings.tsx](ui/src/pages/Settings.tsx), [SettingsExtras.tsx](ui/src/components/SettingsExtras.tsx)
- Two-pane layout: left section nav (General, Sites & domains, Startup & tray, Diagnostics, AI, Resources, Backups), right scroll area.
- New `SettingRow` primitive (title + hint left, control right, divider between rows) in form.tsx; group rows in section cards. Uses Switch from item 2.
- New settings introduced by later items live here (sites folder, diagnostics auto-fix, tray).

**4. Add-site modal** — [DomainDialog.tsx](ui/src/components/site/DomainDialog.tsx)
- Wide dialog, sections with headings: *Source* (project / Quick App), *Address* (domain input with live `https://…` preview + TLD hint), *Serves* as 3 icon cards (PHP / Reverse proxy / Static) instead of select, *Options* (HTTPS / Redirect / Wildcard switches in one row).
- Folder auto-fills from project; drop fixed `w-96`. Same `DomainSettings` still reused in SiteDialog settings tab.

**5. Runtimes page** — [ui/src/pages/Runtimes.tsx](ui/src/pages/Runtimes.tsx), [runtime.rs:39-129](crates/ols-core/src/runtime.rs#L39)
- `loading` state: skeleton rows + spinner until first `list_runtime_catalog` resolves; empty message only after load.
- Speed: `detect_system_install` spawns `<exe> --version` sequentially per entry — run detections in parallel (`std::thread::scope`) inside `catalog()`.
- Table: add real `TableHeader`; each group = full-width row `<TableCell colSpan={5}>` (icon + name + `installed/total`, muted bg); items are normal rows with standard `border-b`. Remove `border-b-2 / border-b-0` hack and the first-cell-only group label.

**6. Web config history VS Code-style** — [ui/src/pages/Config.tsx](ui/src/pages/Config.tsx)
- History list becomes a "Timeline" card in the left column under Files (like VS Code Explorer → Timeline); selected version lifted to page state.
- History tab shows restore bar + `DiffView` at full width of right pane (no nested 15rem column).

**7. Databases alignment** — [ui/src/pages/Databases.tsx:140-259](ui/src/pages/Databases.tsx#L140)
- Databases card: replace flex rows (bare text node) with `Table` (Database | actions right-aligned) matching Users table.
- Users card: inputs row aligned with table; grid gets `items-start` so cards don't stretch. Verify with screenshot from user after.

## Phase 2 — Navigation & layout

**8. Grouped sidebar** — [Sidebar.tsx:50](ui/src/components/layout/Sidebar.tsx#L50)
- `NAV_GROUPS`: Dashboard (ungrouped) · **Sites**: Sites, Quick Apps, Commands, Tunnels · **Web**: Web server, Web config · **Data & services**: Databases, Services, Runtimes · **Environment**: Profiles, Plugins · **Monitor**: Logs, Processes · Settings pinned bottom.
- Small uppercase group labels, collapsible, collapse state in localStorage (try/catch).

**9. Commands page in Sites style** — [ui/src/pages/Commands.tsx](ui/src/pages/Commands.tsx), pattern from [Sites.tsx](ui/src/pages/Sites.tsx)
- Header: project select + actions (New custom command). Command-line input card stays.
- One card: search + source tabs with count badges + full-width table (Command, Source, Description, Last run, Run/actions).
- Row click opens a wide Dialog with side nav (Run form / Details / History) reusing existing `CommandForm` / `QuickDetail`.

## Phase 3 — Desktop integration

**10. New app icon** — [assets/icon.svg](assets/icon.svg), [ui/public/favicon.svg](ui/public/favicon.svg)
- Design new SVG: gradient tile, three isometric stacked layers with small "localhost" globe/dot accent; readable at 16px (simplified strokes).
- `node scripts/render-icon.mjs` → `assets/icon.png`, then `npx tauri icon assets/icon.png` regenerates [src-tauri/icons/](src-tauri/icons/). Sidebar logo updated if it inlines the old mark. Tray uses `default_window_icon()` → updates automatically.

**11. Laragon-style tray menu** — [src-tauri/src/lib.rs:59-107](src-tauri/src/lib.rs#L59)
- Build with `Submenu`/`CheckMenuItem`, rebuilt via `tray.set_menu` after each tray action and on core change events (sites/services/runtimes changed).
- Layout: Open · Start all / Stop all · ── · **Sites ▸** (each site → open URL; Open sites folder; Add site…) · **Web server ▸** (Start/Reload/Stop; ✓ Nginx/Apache/Caddy switch; Open config folder) · **PHP ▸** (✓ installed versions = default; Extensions…) · **Databases ▸** (start/stop each; Open HeidiSQL / NoSQLBooster) · **Services ▸** (toggle each; Open Mailpit) · **Quick App ▸** (recipes) · **Tools ▸** (Terminal, Run doctor, Data folder) · Preferences… · ── · Quit.
- Items that need UI emit `ols:navigate` event to window (Sidebar/App listens) and show it.

**12. Default `sites` folder + live detection**
- [paths.rs](crates/ols-core/src/paths.rs): `sites_dir()` = `<exe dir>\sites` when writable (debug: `<repo>/data/sites`), else `~/Sites`. Created on startup.
- [app.rs:228](crates/ols-core/src/app.rs#L228) `default_projects_dir()` uses it (setting `quickapps.projects_dir` still overrides); always included in `projects.roots`.
- Add `notify` + `notify-debouncer-mini` to [crates/ols-core/Cargo.toml](crates/ols-core/Cargo.toml); watch each root non-recursively, debounce ~1.5s → `sync_auto_domains` + emit event so Sites page refreshes. Removed folders: mark site "folder missing", don't delete.
- Settings: show sites folder path + "Open" button; toggle "Watch sites folder".

## Phase 4 — Tools & automation

**13–14. DB tools: HeidiSQL for Postgres, NoSQLBooster for MongoDB** — [dbtools.rs](crates/ols-core/src/dbtools.rs), [app.rs:580](crates/ols-core/src/app.rs#L580), Databases.tsx
- Backend already launches HeidiSQL for Postgres (`--nettype=8`); gap is UI choice. Expose built-in tools with ids (`heidisql`, `pgadmin`, `nosqlbooster`) in tool list; `open_database` accepts them as `tool_id`.
- Detect NoSQLBooster (`%LOCALAPPDATA%\Programs\nosqlbooster4mongo\NoSQLBooster for MongoDB.exe`, Program Files, PATH), engine `mongodb`, default for Mongo. No documented URI CLI arg → launch it and copy `{uri}` to clipboard + notification "Connect → From URI, paste" (test passing URI first; if it works use it).
- UI: "Open in ▸" split button per engine (default tool + list of detected/registered tools); per-engine default tool setting.

**15. Git clone with username/password or SSH** — [git.rs:636](crates/ols-core/src/git.rs#L636), [ProjectImports.tsx:32](ui/src/components/project/ProjectImports.tsx#L32)
- `GitClone` command gains `auth: Option<GitAuth>`: `Https{username, password, remember}` | `Ssh{key_path}`.
- HTTPS: reuse existing askpass env (`git_env`, git.rs:264) with one-off creds; `remember` → `git_set_credentials(host, …)` (keyring).
- SSH: `GIT_SSH_COMMAND="ssh -i <key> -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"`; passphrase via `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force` script (same pattern as git-askpass.cmd). Remember key per host (setting `git.ssh_key.<host>`) so `git_env` also applies it for pull/push.
- Clone dialog: Auth segmented (None / Username + password / SSH key with file picker defaulting to `~/.ssh/id_ed25519`); auto-select SSH when URL is `git@…`/`ssh://`. Secrets redacted from output (existing).

**16. Auto-fix diagnostics** — [diagnostics.rs](crates/ols-core/src/diagnostics.rs), [repair.rs](crates/ols-core/src/repair.rs), [DiagnosticsCard.tsx](ui/src/components/DiagnosticsCard.tsx), [DoctorDialog.tsx](ui/src/components/DoctorDialog.tsx)
- `Finding.auto_fixable = fix_command.is_some() && !is_destructive(cmd)`.
- Core `auto_fix_diagnostics()`: runs at startup (`run_autostart`) and on a periodic tick; each finding id attempted once per session (no loops); result → notification "Fixed: …" or leaves finding with "auto-fix failed" detail. Setting `diagnostics.auto_fix` (default on).
- "Fix all" button in DiagnosticsCard + DoctorDialog; destructive ones go through existing confirm.

## Phase 5 — New features

**17. Procfile support** — new `crates/ols-core/src/procfile.rs`
- Parse `Procfile` / `Procfile.dev` (`name: command`, comments, `$PORT`/`${PORT}` substitution), unit-tested.
- Mapping: non-`web` entries → `Worker` via `WorkerStore` ([workers.rs:21](crates/ols-core/src/workers.rs#L21)); `web` → `AppSpec` on a Proxy site ([domain.rs:75](crates/ols-core/src/domain.rs#L75)) with a free port, skipped for PHP sites.
- Command `ImportProcfile{project_id, dry_run}` returns preview; manifest setup ([setup.rs:1006](crates/ols-core/src/setup.rs#L1006)) uses Procfile when manifest has no workers.
- UI: WorkersPanel banner "Procfile found — Import" with preview.

**18. .htaccess editor with AI**
- Commands `read_site_file` / `write_site_file {hostname, name}` restricted to allowlist (`.htaccess`) inside docroot, previous content archived via config history.
- SiteDialog new tab ".htaccess" (PHP sites): [CodeEditor](ui/src/components/CodeEditor.tsx) language `apache`; "Create from template" (Laravel / WordPress / SPA). Banner when active server is nginx/Caddy (file ignored there).
- AI: add `AiAnswer.file` (full content in fenced block) in [ai.rs:272](crates/ols-core/src/ai.rs#L272) + `onFile` callback in `AskAi` ([ui/src/lib/ai.ts](ui/src/lib/ai.ts), [AiHost.tsx](ui/src/components/ai/AiHost.tsx)) → `DiffView` preview → "Apply to editor" (user saves). Same callback wired into Config page AI so its suggestions become applicable too.

**19. Real public domain via Cloudflare tunnel** — [domain.rs:128](crates/ols-core/src/domain.rs#L128), [tunnel.rs](crates/ols-core/src/tunnel.rs), web generators in [crates/ols-core/src/web/](crates/ols-core/src/web/)
- `Domain.public_domain: Option<String>` (+ `tunnel_id`).
- Generators emit extra **HTTP-only, no-redirect** vhost for the public domain bound to 127.0.0.1 (nginx `server_name`, Apache `ServerName`, Caddy `http://host` block so no ACME) — avoids redirect loop since TLS ends at Cloudflare. Pass `X-Forwarded-Proto` through.
- Site settings "Public domain" section: enter domain, pick/create named Cloudflare tunnel (token stored in keyring as today), shows dashboard instructions (Public hostname → `http://localhost:<http port>`), existing first-exposure confirmation and public badge.
- Tunnel `autostart` flag + supervisor `RestartPolicy` for named tunnels → survives IP changes/reboots (cloudflared dials out). Hint for Laravel `APP_URL` / TrustProxies.

## Phase 6 — Docs

**20. Linux = Ubuntu + Debian only**
- [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) :436, :443, :502 (drop Fedora/Arch; apt only; systemd-resolved on Ubuntu, hosts file on Debian; `.deb` package), [docs/STATUS.md](docs/STATUS.md) :104, SRS platform lines (:8, :134, :330, :4304).
- Also update STATUS.md / USER_GUIDE.md / CHANGELOG.md for items 11–19.

---

## Verification
- Rust: `cargo test -p ols-core` (new tests: procfile parser, sites_dir resolution, public-domain vhost output for nginx/apache/caddy, git auth env building, NoSQLBooster detection args, auto-fix skips destructive), `cargo clippy --workspace`.
- UI: `npm run build` and `npm run lint` in `ui/`.
- Manual via `scripts\dev.bat`: open/close several dialogs (smooth zoom, Esc/backdrop exit animates); Settings switches; Runtimes shows skeleton then grouped table; Config History tab diff full width with timeline on left; Databases columns aligned; sidebar groups collapse; Commands table + dialog; tray submenus act and refresh; drop a folder into `sites` → `<name>.test` appears without restart; open Postgres in HeidiSQL, Mongo in NoSQLBooster; clone private repo via HTTPS creds and via SSH key; break a fixable item (e.g. stop web server config) → auto-fixed + notification; import sample Procfile; edit .htaccess with AI suggestion; public domain through named Cloudflare tunnel reachable after changing network.
