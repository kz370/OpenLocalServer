# Safe Refactoring Plan: Modularizing Monolithic Files in OpenLocalServer

## 1. Executive Summary & Objective

In accordance with project maintainability guidelines and architecture best practices, this plan defines the **safe, non-destructive refactoring** of oversized files in OpenLocalServer—specifically targeting files exceeding 4,000 lines.

### Primary Goals
1. **Break Monoliths into Focused Modules**: Decompose massive files into smaller, single-responsibility files (targeted under 600–1,200 lines each) using Rust's directory module idiom (`mod.rs`, submodules).
2. **Preserve 100% Behavioral & API Compatibility**: Zero breaking changes to public APIs, IPC contracts, data types, serializations, or CLI command signatures.
3. **Comment Hygiene**: Replace essay-like multi-paragraph doc comments with concise, direct, human developer comments explaining intent without bloat.
4. **Enforce Safety & Spec Synchronization**: Maintain quality gates (`cargo clippy`, `cargo test`, `npm run lint`, `npm run build`) and update all `specs/` documentation per `AGENTS.md`.

---

## 2. Codebase Audit: Monolithic Files (>4,000 Lines)

An audit of the codebase identified two files that severely exceed the 4,000-line threshold:

| File | Original Line Count | File Size | Primary Responsibilities |
|---|---|---|---|
| [`crates/ols-core/src/command.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/command.rs) | **5,719 lines** | 203 KB | Central IPC/CLI command router (`CoreCommand`), response enums (`CoreResponse`), `Core` struct, 76-arm dispatcher match block, and comprehensive dispatch unit tests. |
| [`crates/ols-core/src/app.rs`](file:///I:/Development/repos/OpenLocalServer/crates/ols-core/src/app.rs) | **5,402 lines** | 218 KB | Central application state (`Inner`), domain CRUD & sync, project scanning, web server config application, Quick App execution engine (`QuickHost`), Windows startup/registry, log streaming, and unit test suites. |

*(Note: The next largest files are ~2,000–2,600 lines, such as `runtime.rs`, `ai.rs`, `domain.rs`, and `ui/src/core.ts`, which remain well below the 4,000-line threshold and are kept stable).*

---

## 3. Modular Architecture & Decomposition Plan

### 3.1. Target 1: `crates/ols-core/src/command/` (Replaces `command.rs`)

Rather than housing types, routing, task management, and tests in a single 5,719-line file, `command` is modularized into:

```
crates/ols-core/src/command/
├── mod.rs          # Module declarations, re-exports, Core struct definition, and public dispatch() entry point (~110 lines)
├── types.rs        # CoreCommand, CoreResponse, DashboardData, TaskProgress enum/struct definitions (~1,800 lines)
├── dispatch.rs     # Core::dispatch_inner method containing the central match routing logic (~2,660 lines)
└── tests.rs        # Command dispatch integration and unit tests (~1,145 lines)
```

#### Responsibilities & Boundaries:
- **`mod.rs`**: Re-exports all public types so external callers (`ols-cli`, `src-tauri`, `ols-mcp`) experience seamless backward compatibility with `crate::command::*` and `crate::CoreCommand`.
- **`types.rs`**: Isolates all serialization traits, serde tagging (`rename_all = "camelCase"`), and parameter payloads. All excessive 20-line comments are condensed to crisp 1-line docstrings.
- **`dispatch.rs`**: Implements `Core::dispatch_inner`, handling command dispatching across all application domains (system, services, web, domains, projects, runtimes, tools).
- **`tests.rs`**: Isolates all unit test cases validating dispatch behavior, state responses, and error handling.

---

### 3.2. Target 2: `crates/ols-core/src/app/` (Replaces `app.rs`)

`Inner` is the central state struct of `ols-core`. In Rust, `impl Inner` blocks can be partitioned cleanly across submodules in the same crate without runtime overhead or API changes:

```
crates/ols-core/src/app/
├── mod.rs          # Inner struct definition, constructors (new, with_parts), lock() helper, shared constants (~450 lines)
├── domains.rs      # impl Inner: domain CRUD, bulk domain operations, site summaries, site icons (~850 lines)
├── projects.rs     # impl Inner: project root discovery, scanning, skip/deletion tracking, filesystem watcher (~600 lines)
├── web.rs          # impl Inner: apply_web, apply_web_server, web server config writes, port validation (~235 lines)
├── quickapp.rs     # impl Inner: plan_quick_app, run_quick_command, QuickHost implementation for Host (~1,000 lines)
├── system.rs       # impl Inner: health checks, Windows autostart registry, log inspection/clearing, DB explorer (~700 lines)
└── tests.rs        # Consolidated unit tests for all Inner methods (~1,300 lines)
```

#### Responsibilities & Boundaries:
- **`mod.rs`**: Holds the `Inner` struct, field definitions, constructors (`new`, `with_parts`, `apply_api`), `lock()` helper, and shared constants (`PROJECT_ROOTS`, `PROJECT_SKIP`, `AUTO_SKIP`, `OLS_EXCLUDED_FOLDER`, etc.).
- **`domains.rs`**: Domain lifecycle management, host record updates, domain suggestions, TLS certificate binding, site groups, and favicon downloads.
- **`projects.rs`**: Project directory discovery, `SkipEntry` / `DeletedItem` tracking, excluded folder lists, and directory watcher lifecycle.
- **`web.rs`**: Web server configuration generation, server port conflict checking, tunnel broadcasts, and hash tracking.
- **`quickapp.rs`**: Application blueprint execution, requirement analysis, runtime resolution, and `QuickHost` orchestration.
- **`system.rs`**: Diagnostic environment health checks, Windows Run registry startup entries, log streaming, and database GUI integration.
- **`tests.rs`**: All unit tests verifying `Inner` behavior across domains, projects, startup settings, and quick apps.

---

## 4. Comment Hygiene Guidelines

Both files previously contained verbose architectural commentary. In the new structure, comments adhere to these rules:
- **Concise & Direct**: Explain *why* something is done if non-obvious, or *what* it accomplishes in one or two clear sentences.
- **Remove Historical Narrative**: Omit multi-paragraph essays debating alternatives or documenting historical bug workarounds unless critical for safety.
- **Human-Friendly**: Maintain a professional, clean developer voice.

---

## 5. Quality Gates & Safety Constraints

### Safety Constraints (Binding)
1. **Never run `cargo test` if the user is running `cargo tauri dev`**: Check process lists before initiating test runs to avoid locks or interfering with the dev environment.
2. **Deterministic Quality Verification**:
   ```bash
   cargo fmt --all
   cargo clippy -p ols-core -p ols-helper --all-targets
   cargo test -p ols-core -p ols-helper
   cd ui && npm run lint && npm run build
   ```
3. **No Breaking Changes**:
   - Zero modifications to JSON IPC wire serialization tags.
   - Zero breaking changes to `CoreResponse` and `CoreCommand`.
   - All crate-internal helpers maintain proper `pub(crate)` visibility.

---

## 6. Specs Synchronization Protocol (Mandatory per `AGENTS.md`)

Per the repository's binding mandate, any structural or code change must update `specs/`:
1. **`specs/full_documentation.md`**: Update File Analysis section to document the new `command/` and `app/` submodules and their individual responsibilities.
2. **`specs/catalog.txt`**: Add the new file entries for `command/` and `app/` submodules; remove legacy single-file references.
3. **`specs/relationships.txt`**: Update module dependency relationships for `command` and `app`.
4. **`specs/runtime.md`**:
   - Recompute `Files to Process` to reflect the new modular files.
   - Set `Files Processed` to equal `Files to Process`.
   - Append a conventional commit log entry under `## Log` (e.g. `refactor(core): modularize command.rs and app.rs into focused submodules`).
