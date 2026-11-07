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
- when using images as refs always use webp extention

## 4. Explanation and Accessibility Rules

When explaining concepts, errors, workflows, or technical decisions, use the i-have-adhd skill (/i-have-adhd) whenever available.

Break explanations into short, clear sections.

Prefer simple language, concise sentences, and step-by-step instructions.

Use bullet points, numbered steps, and examples when they improve clarity.

Avoid unnecessary walls of text or overwhelming amounts of information.

Explain complex concepts one step at a time without sacrificing technical accuracy.

Keep code examples separate from explanations.

Highlight important warnings, decisions, and takeaways.

## 5. Version Bump & Release Notes Synchronization Mandate (binding)

Whenever the application version is bumped or changed (e.g. via `scripts/update-version.bat` or manual edits), **or whenever `release-notes/v<version>.md` is updated**, all contributors and AI agents MUST update and synchronize the following files in the same pull request / commit:

1. **Manifests & Packages (on version bump):**
   - Root `Cargo.toml` (`[workspace.package] version = "<version>"`)
   - `src-tauri/tauri.conf.json` (`version: "<version>"`)
   - `ui/package.json` and `ui/package-lock.json` (`"version": "<version>"`)
2. **Release Notes & Settings User Manual (on every release notes update AND version bump):**
   - `release-notes/v<version>.md`: Create/update the version release notes (rendered in `Settings → User Manual → Release Notes`).
   - `CHANGELOG.md`: Keep the `## [<version>] — YYYY-MM-DD` section synchronized with `release-notes/v<version>.md`.
   - `crates/ols-core/catalog/user_manual.md`: Update header `> **Version:** <version>`, Table of Contents, and Section 17 ("Version <version> Specific Notes & What's New" — rendered in `Settings → User Manual → User Manual`) whenever release notes are added or modified.
   - `docs/OLS_USER_MANUAL.md`: Keep header `> **Version:** <version>` and Section 17 ("Version <version> Specific Notes & What's New") identical to `crates/ols-core/catalog/user_manual.md`.
3. **In-App Manual & Settings Viewer:**
   - `ui/src/pages/Settings.tsx`: Ensure the `releaseNotes` import targets `../../../release-notes/v<version>.md?raw`, the header version badge fallback matches `<version>`, and `get_app_guide` queries the updated version.
4. **Website & Landing Page (on version bump):**
   - `index.html` and `website/`: Update version badges, download links (`OLS-<version>-setup.exe`, portable zip, SHA256 checksums), and footer version marks.
   - `README.md`: Update the version badge link (`version-<version>-blue`).

## 6. Security & Behavior Preservation Rules

These rules apply to every code change, feature, bug fix, and refactoring.

1. **Security First:** Follow secure coding practices. Validate and sanitize untrusted input, enforce proper authorization, and never trust client-side validation alone.

2. **No Secrets:** Never hardcode, expose, log, or commit API keys, credentials, tokens, passwords, or sensitive information.

3. **Safe Operations:** Protect filesystem operations against path traversal and unauthorized access. Safely handle external processes, network requests, file uploads, downloads, and archive extraction.

4. **Input & Output Safety:** Prevent SQL injection, XSS, command injection, and other injection vulnerabilities. Use parameterized queries and context-appropriate output encoding.

5. **Dependency Security:** Avoid unnecessary dependencies. Use trusted sources, review dependency vulnerabilities, and verify downloaded executable files and archives using trusted integrity checks.

6. **Error Handling:** Handle failures safely without exposing sensitive information. Never silently suppress errors or leave operations in an inconsistent state.

7. **Behavior Preservation:** Do not change existing functionality, UI, workflows, APIs, configurations, or defaults unless explicitly required to fix a confirmed bug or implement a requested feature.

8. **Minimal Changes:** Make the smallest safe change possible. Avoid unrelated refactoring and breaking changes.

9. **Testing:** Add or update tests for every meaningful change, especially security-sensitive functionality. Run relevant tests, linting, and static analysis before considering work complete.

10. **Security Review:** Before finalizing any change, review the modified code for newly introduced vulnerabilities, unintended side effects, and regressions.

**Golden Rule:** Every change must be secure, tested, and backward-compatible wherever possible. Never sacrifice security or existing functionality for convenience.