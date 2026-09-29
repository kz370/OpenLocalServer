# OLS — Brand Identity Guide

Status: proposed. Supersedes nothing yet; `specs/` untouched because this is a document, not a behavior change.

---

## 1. The decision

**OLS** is the product name. `Open Local Server` becomes the expansion, used only where a reader will not know the acronym.

The reason this is cheap: the acronym already exists in the codebase. `ols-core`, `ols-helper`, `ols-cli`, the `ols` binary, `OLS_HOME`, `C:\Tools\OLS` in the test fixtures. Nothing internal has to move. What changes is which name is *presented* to a user.

The reverse would be expensive. Renaming `ols-*` crates to `openlocalserver-*` would touch every `Cargo.toml`, every `use` statement, and break the crate names people already depend on from Cargo.lock. Do not do that.

### Name form table

| Context | Form | Notes |
|---|---|---|
| App window title, taskbar, Start menu | `OLS` | No expansion. |
| CLI binary | `ols` | Unchanged. |
| README H1, docs site header | `OLS` | |
| First mention in prose | `OLS (Open Local Server)` | Once per document, then `OLS`. |
| Repo name | `OLS` | GitHub redirects the old name; do the rename in one commit. |
| Crate names | `ols-*` | Unchanged. |
| Verb | "OLS runs", "OLS wrote", "OLS supervises" | Subject is the product, not "the app". |
| Adjective | "OLS-managed" | e.g. OLS-managed installation. |

### Expansion rule

Write the expansion when the audience has not seen the product before: landing pages, release announcements for a new audience, installer welcome text, error messages aimed at a first-run user. Skip it everywhere else. "OLS is the successor to XAMPP" — fine. "Restart OLS to load them" — fine.

---

## 2. Visual direction

### 2.1 The mark

Current mark is a green server-and-globe glyph, embedded as base64 PNG inside three SVG files (`data/home/logo.svg`, `data/home/logo-dark.svg`, `ui/public/favicon*.svg`) and rasterized across ~30 PNG/ICO/ICNS targets in `src-tauri/icons/`.

Rebrand replaces the glyph, keeps the geometry approach. Design brief for the new mark:

- **Form.** A monogram, not an illustration. Three letters in a rounded-square container, or a single abstracted `O` that reads as both a loopback ring and the letter. An illustrated globe forces a geographic metaphor the product does not have — the product is loopback, not internet.
- **Loopback is the idea.** The `O` in OLS is the strongest asset in the name. A ring that terminates in an arrowhead, or a ring that visibly returns to its own start, encodes "everything points at this machine." Use that.
- **Legibility floor.** Must survive 16×16 in the taskbar and in a Start-menu list. Test at 16px, not just at 1024.
- **Redundant states.** The icon set already carries running/stopped and light/dark variants (`favicon-stopped.svg`, `favicon-dark.svg`, `icons/dark/`, `icons/red/`). The new mark needs the same matrix. Keep a per-state delta, not a per-state redesign.

**Deliverables.** One master SVG; a build step that derives every PNG/ICO/ICNS target from it. `scripts/build-app-icons.ps1` already exists for this and is the right home. Embedding a base64 PNG inside a hand-edited SVG is what made the current set drift; replace that with generation.

### 2.2 Color

The palette is already deliberate and documented in `ui/src/index.css:5-7`: teal/emerald around 165–168° hue, paired with a lime success tone, chosen to avoid the indigo every AI-tool dashboard defaults to. **Keep it.** The colors are not the problem; the name is out of step with them.

| Token | Light | Dark | Role |
|---|---|---|---|
| `--primary` | `oklch(0.6 0.135 168)` | `oklch(0.75 0.14 168)` | Primary action, active nav, focus ring |
| `--accent` | `oklch(0.93 0.04 168)` | — | Hover, selection |
| `--success` | `oklch(0.72 0.19 145)` | — | Service running, healthy |
| `--warning` | `oklch(0.78 0.16 75)` | — | Degraded, needs attention |
| `--destructive` | `oklch(0.58 0.22 27)` | — | Stop, delete, failed |

Hue 168 is the signature. Constraint: no new hue is introduced for brand reasons. If a surface needs a color that is not in this set, it is a functional color, and it gets a token name, not a hex literal.

The one adjustment the rebrand permits: raise `--primary` contrast one step in the dark theme so the monogram and the wordmark hold against `oklch(0.15 0.008 200)`. Measure it; do not eyeball it.

### 2.3 Typography

The UI is Tailwind with Radix primitives. There is no brand typeface and there should not be one — a display face in a developer tool reads as marketing, and this product's users open it to fix a port conflict. Wordmark uses the same family as the UI at a heavier weight and tighter tracking. `.tracking-tight` is already the pattern in `ui/src/components/layout/Sidebar.tsx:146`.

### 2.4 Wordmark

`OLS` set as three caps, tight tracking, with the `O` carrying the loopback detail. Lockup with a device glyph: mark, then wordmark, 8px gap, baseline-aligned. Minimum lockup width 96px; below that, mark alone.

---

## 3. Tone of voice

### 3.1 Position

OLS is a tool that does not make you ask for permission. It installs runtimes, edits the hosts file, trusts a certificate authority, and runs a LocalSystem service. The product's job is to make that safe enough that the developer stops noticing.

The tone follows from that: **plain, specific, and finished.** Not clever, not apologetic, not corporate.

### 3.2 Rules

**Name the actual thing.** "Port 3306 is already used by `mysqld.exe` (PID 4120)." Not "Database port unavailable."

**State cause and fix together.** This is already a product rule, not just a writing rule — see AGENTS.md §3, `Diagnostic{problem,cause,fix}`. Every user-visible error names what broke, why, and what to do. Copy inherits this.

**Never blame the user.** "OLS could not read `C:\Program Files\OLS\config.yaml` — the file is owned by another process. Close OLS Helper in Services and retry." Not "You don't have permission."

**Short sentences. Active voice. Present tense.** "OLS starts Nginx." Not "Nginx will be started by OLS."

**Say what happened, then what happens next.** Not both in one clause.

**No exclamation marks in the product UI.** The app reports a lot of state; excitement reads as noise. One place where enthusiasm is allowed: the changelog, first person, sparingly.

### 3.3 Vocabulary

Prefer the domain words the user already uses. "Service" over "managed process." "Runtime" over "dependency." "Site" over "virtual host." "Project" over "workspace." This vocabulary is already established in the SRS and should survive the rebrand unchanged — a rename that also renames the domain is two changes at once.

### 3.4 Banned

- "Simply", "just", "effortlessly", "powerful", "seamless", "robust", "blazing fast"
- "Sorry" / "Oops" in the UI
- Exclamation marks in UI strings
- Second person in a `Diagnostic` cause clause — describe the system, not the reader
- Marketing adjectives in a settings page, a log line, or an error

### 3.5 Before / after

| Before | After |
|---|---|
| "Everything OpenLocalServer depends on, checked in one go" | "Everything OLS depends on, checked in one go" |
| "Start with Windows — Adds OpenLocalServer to your account's startup list" | "Start with Windows — Adds OLS to your account's startup list" |
| "OpenLocalServer will not silently destroy manual configuration." | "OLS never overwrites configuration you edited by hand." |
| "Restart OpenLocalServer afterwards to load them." | "Restart OLS to load them." |
| "The user owns the whole file." | "You own the whole file." |

That last row is the general rule: the docs were written in spec voice ("the user"), the UI is written in operator voice. A rebrand is the right moment to make the UI consistently operator voice, because you are touching those strings anyway.

---

## 4. Touchpoint migration

### 4.1 Inventory

462 matches across 121 tracked files.

| Area | Hits | Files | Risk |
|---|---|---|---|
| `crates/` | 192 | 58 | Mixed — see 4.3 |
| `OpenLocalServer_Master_SRS_v4.md` | 72 | 1 | Low |
| `ui/src/` | 49 | 30 | Low |
| `specs/` | 39 | 8 | Low |
| `docs/` | 26 | 6 | Low |
| `installer/open-local-server.iss` | 22 | 1 | **High** — 4.3 |
| `scripts/` | 22 | 3 | Medium |
| `src-tauri/` | 13 | 3 | **High** — 4.3 |
| `README.md` | 8 | 1 | Low |
| `release-notes/` | 4 | 1 | Low |

### 4.2 Tier 1 — safe, do first

Pure presentation. No installed state references these.

- `ui/src/` — 30 files, 49 strings. Titlebar, sidebar, settings, tooltips, dialog copy, `DOCS_URL`.
- `README.md`, `docs/*`, `CHANGELOG.md`, `SECURITY.md`, `release-notes/*`
- `OpenLocalServer_Master_SRS_v4.md` — the SRS is a requirements document; its subject becomes OLS.
- `specs/*` — 8 files, 39 hits. Note that `specs/` is the binding source of truth per AGENTS.md §1; a rebrand that changes displayed names without changing behavior still needs a line in `specs/runtime.md` under `## Log`.
- `crates/` doc comments and `//!` module headers — 58 files, but only the prose half.
- `crates/ols-core/src/ai.rs:437` — the assistant's system prompt names the product. It must track whatever the UI says.
- The welcome page in `crates/ols-core/src/domain.rs:344-500` — served at `openlocalserver.test`, HTML `<title>` and body copy. Visible to every new user.
- User-visible Rust strings: `MANIFEST_HEADER` at `crates/ols-core/src/manifest.rs:276` is written into a file the user commits to git. Renaming it produces a diff in every project on the next run.

### 4.3 Tier 2 — state-bearing, rename breaks installs

**These must not change in the rebrand commit.** Each is a live identifier on someone’s machine. Renaming any of them orphans that user’s configuration, and there is no recovery path.

| Identifier | Location | Consequence of renaming |
|---|---|---|
| `dev.openlocalserver.app` | `tauri.conf.json` `identifier`; Inno `AppId` | Inno Setup treats a new `AppId` as a *different product*. Two installs, two uninstallers, orphaned service. |
| `C:\OpenLocalServer` | `.iss` `DefaultDirName` | Installs land in a new folder; `ols-helper` copy and runtimes not found. |
| `C:\Program Files\OpenLocalServer\` | `crates/ols-helper/src/service.rs:56` | Elevated helper not found; every hosts-file and cert operation fails. |
| ~~`Open Local Server.exe`~~ → `OLS.exe` | installer `SourceExe`, `AppExe`, shortcuts | **Renamed — see 4.6.** A pre-rename install still runs under the old name, so every "is it running?" check in the `.iss` `[Code]` section asks about both, and `PruneUnshippedFiles` deletes the old exe at `ssPostInstall`. |
| `openlocalserver.exe` | installer `DaemonExe` | Daemon lifecycle detection fails. |
| `OpenLocalServerHelper` (service) | `service.rs:29` | Existing helper orphaned as a running LocalSystem service with no managing process. |
| `\\.\pipe\OpenLocalServerHelper` | `service.rs:30` | Client/service mismatch; helper unreachable. |
| `# BEGIN OpenLocalServer` / `# END OpenLocalServer` | `hosts.rs:8-9` | New code appends a second block. The old entries stay, and stale hostnames keep resolving. Silent, and confusing. |
| `'OpenLocalServer'` NRPT comment | `ols-helper/src/main.rs:112-119` | Old DNS rules never removed; `.test` lookups fail on split-tunnel networks. |
| `OpenLocalServer` (keyring service) | `crates/ols-core/src/secrets.rs:10` | Every stored DB password and API token becomes unreadable. **No recovery.** |
| `.openlocalserver/` | `manifest.rs:22`, git ignore rules, AI prompt | Project manifests stop being found. Committed to users’ git repos. |
| `openlocalserver.test` | `domain.rs:252` | New wildcard cert and hosts entries; the old domain keeps resolving to the old content. |
| `dev/OpenLocalServer/OpenLocalServer` | `paths.rs:54` `ProjectDirs` | App-data fallback is not found; settings, sites, and DB appear empty. |
| `OpenLocalServer_Master_SRS_v4.md` | repo root | The README links to it by name. Rename, then fix the link in the same commit. |
| GitHub `kz370/OpenLocalServer` | URLs in `Cargo.toml`, `ReleaseCards.tsx:20,154` | 404s. GitHub redirects; still fix them. |

**Rule:** a string that a user’s machine persists, and that no migration routine reads, is frozen forever. `paths.rs:62` already shows the pattern for doing this properly — `migrate_legacy` moves old data once, then the new name owns it. If a name must change later, it needs a migration of that shape, written and tested before the rename, not after.

**Exception taken, once.** `Open Local Server.exe` → `OLS.exe` did ship, against the rule above. §4.5 records why it was safe and exactly what carries it. The other rows in this table are still frozen.

### 4.4 Sequencing

Five releases. Each one ships.

**R1 — Display only.** Tier 1 in full. Titlebar, sidebar, settings, dialogs, docs, README, welcome page, AI system prompt, SRS. Nothing that touches disk. Old installs keep working exactly as before, because nothing they read changed. This is the release that establishes the name.

**R2 — Assets.** New mark across the full icon matrix, generated by `scripts/build-app-icons.ps1` from one master SVG. Include running/stopped and light/dark variants, and the Wordmark store logo. Ship as a normal release; the icon is what users see in the taskbar, and it should not arrive bundled with copy changes that need review.

**R3 — Installer and release surface.** Rename the setup file to `OLS-{version}-setup.exe`, the portable zip and the SHA256SUMS file to match, and the app exe to `OLS.exe` (see §4.5 for why the exe was safe to move and what carries the old name). Keep `AppId`, `DefaultDirName` and `DaemonExe` exactly as they are, and leave a comment at each saying so and pointing at this table. Rename the repo to `OLS`; GitHub redirects the old path. Update the URLs in `Cargo.toml` and `ReleaseCards.tsx`.

**R4 — Deprecation notice.** No renames. In-app banner and a release-notes entry: "OLS is the new name for Open Local Server. Nothing is changing about how it works or where your data lives." The `legacy_root` migration already in `paths.rs` is the precedent for how the project tells users that a location moved.

**R5 — Optional, probably never.** Renaming any Tier 2 identifier requires a migration routine per identifier, a test per migration, and a release note that says which data moved. The keyring entry in `secrets.rs` cannot be migrated at all — a service name is the addressing key, and the old one cannot be written to without the old name. Renaming it destroys every stored secret on every machine. **Leave it.**

### 4.5 The exe rename — `Open Local Server.exe` → `OLS.exe`

This is the one §4.3 identifier that moved, so the reasoning is recorded rather than assumed.

**Why it was safe.** The exe name is not a *key*. Nothing reads it back as an identifier: settings, sites, secrets, the helper service, the hosts block, the pipe name and the cert store are all addressed by something else, and a rename of the file changes none of them. `AppId` and `DefaultDirName` are the two that genuinely are keys — one to the uninstall registry entry, one to the install folder — and both are untouched, so the upgrade lands in the same folder, over the same `AppId`, with the same uninstaller. `crates/ols-core/src/app.rs` resolves its own path with `std::env::current_exe()` rather than a literal, which is what makes this a rename rather than a migration.

**What actually breaks, and what carries it.** The file lock. A pre-rename install is still running as `Open Local Server.exe`, holding the exact file `[Files]` is about to write, and that is the bug this script has spent its life preventing: an install that reports success while the old version stays on disk. So:

- `installer/open-local-server.iss` keeps `LegacyAppExe` next to `AppExe` and asks about **both** in every check that can matter — `RegisteredDir` (so a pre-rename install is still *detected* as an existing install rather than mistaken for a second copy), `AnyAppRunningHere`, `InitializeSetup`, and `PrepareToInstall`.
- `PruneUnshippedFiles` does **not** list `LegacyAppExe` as shipped, so the old exe is deleted at `ssPostInstall` — which only runs after `PrepareToInstall` has confirmed the image is closed, so the delete cannot hit a lock. Leaving it would be worse than untidy: two app exes in one folder, the old one still holding the single-instance mutex, and the user's existing shortcut still launching the version that was just replaced.
- `scripts/build-installer.bat` gained `:app_running`, which checks all three names — `OLS.exe`, `Open Local Server.exe`, and the cargo bin `openlocalserver.exe`, since a dev build out of `target\` runs under the third.
- `scripts/upload-release.bat` accepts either exe name in a dist folder, so a folder built before the rename is still publishable.

**Still frozen:** `dev.openlocalserver.app` (`identifier` and `AppId`), `C:\OpenLocalServer`, `C:\Program Files\OpenLocalServer\`, `openlocalserver.exe` (the daemon), `OpenLocalServerHelper`, `\\.\pipe\OpenLocalServerHelper`, the hosts markers, the NRPT comment, the keyring service, `.openlocalserver/`, `openlocalserver.test`, `dev/OpenLocalServer/OpenLocalServer`.

**Why the artifact names moved with it.** The setup file is what a user downloads and clicks, so leaving it as `Open-Local-Server-1.0.0-setup.exe` while the thing it installs is `OLS.exe` is the same inconsistency one level up. The portable zip and the SHA256SUMS file follow the setup file, since the checksums name the setup file.

**The lesson for the next rename.** A file name is safe to change; a *registry value, service name, pipe name, or domain* is not, because those are addressing keys. Anything that is a key needs `migrate_legacy`-shaped code written and tested first, and some of them — the keyring service name — can never be migrated at all.

### 4.6 Acceptance for the rebrand

A release is done when:

- `grep -ri "open local server" ui/ docs/ README.md specs/` returns nothing user-facing. Tier 2 strings are expected to remain and are individually accounted for in 4.3.
- Every string in the 4.3 table is either unchanged or has a shipped migration.
- `specs/runtime.md` has a `## Log` line, and `Files Processed` still equals `Files to Process` (AGENTS.md §1).
- Quality gates pass: `cargo fmt --all`, `cargo clippy -p ols-core -p ols-helper --all-targets`, `cargo test -p ols-core -p ols-helper`, `cd ui && npm run lint && npm run build`.
- Upgrade path tested on a machine that has a real R0 install: sites still listed, services still start, stored secrets still read, the hosts block is not duplicated, `.openlocalserver/` manifests still resolve.

---

## 5. What not to do

- **Do not rename the crates.** `ols-*` is already right.
- **Do not introduce a tagline.** "Local development, simplified" is currently on the welcome page at `domain.rs:351`. It is fine, it is not a new one. Do not add a second.
- **Do not change the domain vocabulary.** Service, runtime, site, project. Those are the product, not the branding.
- **Do not touch `.openlocalserver/`.** It is in users’ git repositories.
- **Do not migrate the keyring.** There is no migration.
- **Do not ship the icon and the copy in the same release.** Two independent changes, two independent reviews, two independent rollback decisions.
