# Contributing

Thanks for helping. This is a short guide to getting a change in.

## Setup

You need Windows, Rust (stable), Node 22 and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```
dev.bat
```

That installs the UI dependencies if needed, builds the helper, starts the Vite dev server and launches the app.

## Layout

- `crates/ols-core` — all app logic (runtimes, sites, servers, databases, diagnostics).
- `crates/ols-helper` — the small elevated helper (hosts file, trust store).
- `src-tauri` — the desktop shell.
- `ui` — the React interface.
- `docs` — the [implementation plan](docs/IMPLEMENTATION_PLAN.md) and [status](docs/STATUS.md).

## Before you open a pull request

```
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper
cd ui && npm run lint && npm run build
```

CI runs the same checks.

## Guidelines

- Keep a change focused. One feature or fix per pull request.
- Add a test for new logic in `ols-core`.
- Errors shown to the user should say what went wrong, why, and how to fix it.
- Anything that downloads a file must verify its SHA-256.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`).
