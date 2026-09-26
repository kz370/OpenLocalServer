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

## Specs-sync mandate (binding)

`specs/` is the single source of truth. Any code change MUST update specs in the same PR:

- Rust (`crates/`, `src-tauri/`): `specs/full_documentation.md`, `specs/architecture_overview.md` (if layers change), `specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt` (if types change), `specs/api_reference.md` (if `CoreCommand` changes), `specs/diagrams/*.mmd` as needed.
- UI (`ui/src/`): `specs/full_documentation.md`, `specs/catalog.txt`, `specs/relationships.txt`, `specs/api_reference.md` (if IPC changes).
- Config / catalogs / plugins / quick-apps: `specs/data_models.txt` + `specs/full_documentation.md`.
- Then append log line to `specs/runtime.md` under `## Log`.
- If no spec update needed, state why in PR description. Behavior change without spec update gets rejected. See `AGENTS.md`.

## Guidelines

- Keep a change focused. One feature or fix per pull request.
- Add a test for new logic in `ols-core`.
- Errors shown to the user should say what went wrong, why, and how to fix it.
- Anything that downloads a file must verify its SHA-256.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`).
