# Plan: event-driven diagnostics auto-fix + accurate Settings text

## Context

Settings > General > Diagnostics says "Runs at startup and every five minutes." The code matches
(`src-tauri/src/lib.rs:2138-2155` loops and sleeps 300s), but:

- A full `diagnose()` plus every project's `diagnose_project()` runs every 5 minutes even when nothing changed. A fix can also lag up to 5 minutes behind the problem.
- The text omits two rules: each problem is auto-fixed only once per session (`auto_fix_attempted`, `crates/ols-core/src/repair.rs:518`), and service start/restart fixes are never run in the background (`is_background_auto_fixable`, `repair.rs:489`).

Goal: run auto-fix when state actually changes, keep a slow safety-net timer, and correct the text.

## Design

A trigger-driven worker replaces the fixed 300s loop. No new core API: `Core::auto_fix_diagnostics()` stays unchanged.

Triggers (the broadcast channels already exist and are subscribed in `src-tauri/src/lib.rs`):

1. App startup, after `run_autostart()` finishes (existing spot, `lib.rs:2134`).
2. Project change: the `project_events` listener (`lib.rs:2037-2043`).
3. Runtime installed: the `RuntimeEvent::Installed` arm (`lib.rs:2010`).
4. Setting `diagnostics.auto_fix` saved as on, so enabling it fixes immediately.
5. Fallback timer every 30 minutes, for out-of-band changes (for example the user deletes a folder on disk).

Not a trigger: `ServiceStateEvent`. It fires often, and service lifecycle fixes are excluded from auto-fix anyway.

## Changes

1. `src-tauri/src/lib.rs`
   - Create an `Arc<tokio::sync::Notify>` (`auto_fix_trigger`) before the listeners.
   - Call `auto_fix_trigger.notify_one()` in the project listener, the `Installed` arm, and after startup autostart.
   - Replace the `loop { ... sleep(300) }` with a worker: `select!` on `notified()` or a 30-minute sleep, then debounce about 2s (drain extra notifications), then run `auto_fix_diagnostics()` inside `tokio::task::spawn_blocking` (it blocks on `wait_process` for up to 900s). Keep the existing `notify()` success/failure messages.
2. Setting-save hook
   - Find where `set_setting` for `diagnostics.auto_fix` is handled (grep `set_setting` in `crates/ols-core` and `src-tauri`) and notify the trigger when the value becomes true. If there is no clean hook, notify from the Tauri command wrapper.
3. `ui/src/pages/Settings.tsx:811`: change the hint to
   "Fixes safe problems as soon as they are found: at startup and when projects or runtimes change. Fixes that may replace or remove data always ask first. Each problem is tried once per session. Saved with the Save button below."
4. Specs (AGENTS.md section 1)
   - Update `specs/full_documentation.md` (Business Rules: diagnostics auto-fix triggers), `specs/catalog.txt`, `specs/relationships.txt` (trigger wiring).
   - Append a log line to `specs/runtime.md`; keep Files Processed equal to Files to Process.
   - Search `crates/ols-core/catalog/user_manual.md`, `docs/OLS_USER_MANUAL.md` and `release-notes/` for "five minutes" wording. If any changes, keep the two manuals identical and sync release notes and `CHANGELOG.md` (AGENTS.md section 5).

## Reuse

- `Core::auto_fix_diagnostics` (`crates/ols-core/src/repair.rs:501`): unchanged, already dedups per finding.
- Existing subscriptions in `src-tauri/src/lib.rs:1984-2043`: only add a notify call.

## Verification

- `cargo fmt --all`
- `cargo clippy -p ols-core -p ols-helper --all-targets`
- `cargo test -p ols-core -p ols-helper` (skip if `cargo tauri dev` is running, per AGENTS.md)
- `cd ui && npm run lint && npm run build`
- Manual in `cargo tauri dev`:
  - Break something safe-fixable (for example a missing `.env` in a project), then add or change the project. Confirm the "Fixed: ..." notification appears within about 2s, without waiting 5 minutes.
  - Confirm no repeated scans while idle (temporary tracing log, or check the logs).
  - Toggle the setting off then on. Confirm a run happens immediately when turned on.
  - Confirm service start/restart findings are still never auto-run.

## Decision

Keep a 30-minute fallback timer (recommended) rather than dropping the timer entirely.
