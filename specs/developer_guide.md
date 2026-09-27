# Developer Guide — OpenLocalServer

## Setup
Requirements: Windows 10/11 x64, Rust stable, Node 22, Tauri prerequisites (WebView2, MSVC build tools).
Clone repo, then run `dev.bat` from repo root. Script installs UI deps if `ui/node_modules` missing, builds `ols-helper`, starts Vite dev server, launches `cargo tauri dev`.
Portable layout: `data/` beside exe holds settings, certs, web configs, DB data, logs. Override with `OLS_HOME` env var. Debug run uses `repo/data`.

## Dependencies
Rust workspace members: `crates/ols-core`, `crates/ols-helper`, `crates/ols-cli`, `src-tauri`. Key crates: tokio, serde, reqwest+rustls, rcgen, portable-pty, sysinfo, keyring, minijinja, clap, hyper, notify, minisign-verify.
Frontend: React 19, Vite 8, Tailwind 4, shadcn/ui, CodeMirror, xterm.js, Tauri API 2.11. Install via `npm install` in `ui/`.
Managed binaries downloaded on demand with SHA-256 verify; never run unverified binary.

## Run Instructions
- Dev app: `dev.bat`.
- Dev CLI: `cargo run -p ols-cli -- <cmd> --help`.
- Smoke checks: `cargo run --release --example smoke -p ols-core`; web smoke needs `OLS_HOME=<dir> OLS_HOSTS_FILE=<dir>/hosts`; services smoke similar.
- Production: `build-installer.bat` (4-stage) builds UI, builds release exes (`cargo build -p openlocalserver -p ols-helper --release`), copies to `release/`, compiles Inno Setup if found, offers GitHub upload via `upload-release.bat`.
- First run flow: Runtimes page install PHP/Node -> Sites add folder + domain `*.test` -> WebServer apply + trust CA once -> Services start DB/mail.

## Debugging
- Logs: `data/logs/ols-core.log` (JSON daily rolling, secrets redacted). UI Logs page polls `list_log_sources` / `read_log` every 1.5s; filter by severity regex.
- Diagnostics: Dashboard DiagnosticsCard (`run_diagnostics`), DoctorDialog (`doctor`), CLI `ols doctor`. Every error shaped `{problem,cause,fix}`.
- Processes page shows raw supervisor state + output ring (200 lines); system stats poll 2s.
- Health: Sites page per-site `health_check` runs DNS->TCP->TLS->Cert->HTTP chain with per-step detail.
- Common traps: port busy -> PortChecker reports owner via netstat/tasklist (never kills); web invalid -> old config kept + error; drift -> hash detect, preserve hand edits, flag badge.
- Isolated tests: `test_support::isolated_home()` sets temp OLS_HOME with global env lock.

## Extension Guide
- New CoreCommand: add variant in `command.rs`, implement in `app.rs` Inner method, return CoreResponse, expose in `core.ts` types, call via `runCommand`. No direct manager access from UI.
- New runtime: add pin in `catalog.rs` (HTTPS URL + 64-hex SHA256), probe logic in `runtime.rs`, UI row in Runtimes page. Remote catalogs need minisign key.
- New plugin: write `plugin.yaml` (id/name/version/permissions/contributes), `ols plugin install <folder|zip>`; permissions gate exact list; manifest change voids approval.
- New quick-app: YAML in `catalog/quick-apps/` following schema (variables/steps/files/env/post_create/domain); validate via strict schema + Minijinja strict undefined.
- New web server: implement `WebServer` trait (render_main/render_site/validate/start/reload/stop), register in `server_by_id`, add UI tab.
- New tunnel provider: implement `TunnelProvider` trait (authenticate/create/start/stop/status/url/logs), register in TunnelManager.

## Style Rules
- `cargo fmt --all` before PR.
- `cargo clippy -p ols-core -p ols-helper --all-targets` clean.
- `cargo test -p ols-core -p ols-helper` pass.
- `cd ui && npm run lint && npm run build` pass.
- Errors always Diagnostic shape, never raw strings to UI. Secrets never in logs/files; use keyring + redactor. HTTPS + SHA256 for all downloads. No telemetry by default.

## Contribution Guide
Fork, branch, small focused PRs. Update docs/STATUS.md when adding features. Add smoke example coverage for new managers. Security issues: see SECURITY.md, do not file public issue. License MIT. See CONTRIBUTING.md and CODE_OF_CONDUCT.md.
