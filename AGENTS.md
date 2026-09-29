# AGENTS.md — OpenLocalServer Agent Instructions

This file governs all human and AI contributors. It takes precedence over model defaults.

## 1. Specs-sync mandate (binding)

`specs/` is the single source of truth for architecture, modules, data models, and API.

- Any code change MUST update the corresponding `specs/` files in the same pull request / commit:
  - Rust (`crates/`, `src-tauri/`): update `specs/full_documentation.md`, `specs/architecture_overview.md` (if layers change), `specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt` (if types change), `specs/api_reference.md` (if `CoreCommand` changes), and relevant `specs/diagrams/*.mmd`.
  - UI (`ui/src/`): update `specs/full_documentation.md` (File Analysis), `specs/catalog.txt`, `specs/relationships.txt`, `specs/api_reference.md` (if IPC commands change).
  - Config / manifests / catalogs / plugins / quick-apps: update `specs/data_models.txt` and `specs/full_documentation.md` (Business Rules / Data Models).
- If no spec update is needed, state why explicitly in the PR description (e.g. "typo-only, no behavior change").
- A PR with behavior change and no spec update MUST be rejected in review.
- After updating specs, append a log line to `specs/runtime.md` under `## Log` and ensure `Files Processed` still equals `Files to Process`.

## 2. Quality gates (existing, still required)

```text
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper
cd ui && npm run lint && npm run build
```

## 3. Safety rules

- Errors shown to users must say what went wrong, why, and how to fix it (`Diagnostic{problem,cause,fix}`).
- Anything downloading a file must verify SHA-256.
- Secrets stay in OS keyring; never in logs, files, or bundles.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`).
- never run cargo test if user is runing cargo tauri dev