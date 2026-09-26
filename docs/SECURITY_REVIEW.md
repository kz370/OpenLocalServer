# Security review against SRS §138 (Stage 17)

Reviewed 2026-09-26 by reading the code and running its tests, not by an outside auditor. Each requirement lists
what was checked and what is left. "Not run" means the check needs a real machine or a real service that this
review didn't have.

| # | Requirement | Result | Evidence and remaining risk |
|---|---|---|---|
| 1 | The app doesn't stay elevated | Met | No `requireAdministrator` or elevated manifest in `src-tauri`; the app runs as the user. Elevation happens only in the helper. |
| 2 | Privileged work goes through a helper | Met, one accepted risk | `ols-helper` has a closed command set (`hosts-apply`, `hosts-remove`, `nrpt-add`, `nrpt-remove`) and validates every argument (tests: entries that could hijack real sites are refused; unknown commands and bad NRPT arguments are refused). The optional service's pipe is `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)`: SYSTEM, administrators and **any interactive user** on the machine can send that closed set. On a shared computer, another signed-in user could change the managed hosts block for names the validation allows. Accepted for single-user development machines; tightening the SDDL to the installing user's SID is the fix if that changes. |
| 3 | Packages are verified | Met | Every catalog entry needs a 64-character SHA-256 and an HTTPS URL; a mismatch aborts the install. Plugin and catalog runtimes go through the same check (`OwnedManifest::check`), and catalogs are additionally signature-verified. Several vendors publish no checksum, so those entries are pinned from a direct HTTPS download (noted in `catalog.rs`): trust on first use, not a vendor signature. |
| 4 | Secrets aren't logged | Met for what was checked | `set_setting` logs go through `redact_value`; the support bundle and the AI prompts go through `redact_text` (tests for `.env` values, bearer tokens, URL credentials, known token shapes, private keys and cookies). Secrets live in the keyring, not in `settings.json`. Not re-audited: every `tracing` call in the code base. |
| 5 | No local API on the LAN by default | Met | The control channel is a named pipe that rejects remote clients. The HTTP API is off by default, binds `127.0.0.1` only, needs a bearer token, refuses browser (`Origin`) and rebinding (`Host`) requests, and is read-only unless switched to a short allow-list (tests cover each). Nginx, Apache and Caddy listen on `127.0.0.1`; MariaDB, PostgreSQL, Redis and MongoDB bind `127.0.0.1`; the traffic inspector binds `127.0.0.1`. |
| 6 | Plugins require permissions | Met | Every contribution needs its permission; a plugin stays off until the exact list is approved and a changed manifest voids the approval; zip installs reject paths that leave the folder; code plugins are refused (no sandbox). Tests for each. |
| 7 | Untrusted Quick Apps don't run silently | Met | Imported recipes are untrusted until their source is approved; each run shows a review first. Tests exist in `quickapp::catalog`. Plugin recipes are trusted only through the plugin's approved `quick_apps` permission. |
| 8 | Tunnels need an explicit action | Met | The first start needs `confirm_exposure`; the sidebar shows a public badge; the API can't start tunnels. |
| 9 | Database ports aren't exposed by default | Met | Loopback binds above, and the tunnel target check refuses databases, caches, mail and debugger ports unless the tunnel allows internal targets. |
| 10 | Private keys are protected | Met with a caveat | The CA key gets an owner-only ACL through `icacls`. That call is best-effort: if it fails, certificate generation continues and the key stays protected only by the user profile's own permissions. Keys are never shown, logged or sent. |

## Added in this stage, and how each is contained

- **Local HTTP API**: token stored as a SHA-256 only; constant-time compare; failed sign-ins delayed; 1 MB body limit;
  commands that run programs, change settings, read secrets, change credentials or tunnel tokens, or install plugins
  are on a deny list, and `operate` mode is an allow-list rather than "everything else".
- **Updater**: nothing runs on its own. A manifest without a valid signature from the update key is refused; the
  installer must match the SHA-256 in the signed manifest; only a file the updater downloaded into its own folder
  can be started.
- **Explorer menu**: per user (`HKCU`), runs only the `ols` command line with the folder Explorer passes.
- **Support bundle**: redacted text only; no `.env`, keys, secrets or project files.

## Not done

- The §160 end-to-end run on a clean Windows VM, and an outside security review.
- A signed installer: the build isn't code-signed, so Windows SmartScreen will warn.
- Signing keys for updates and for a default catalog don't exist yet, so neither ships preconfigured.
