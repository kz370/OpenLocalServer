# Docker Manager + "Create Site from Docker" — Implementation Plan

> **For agentic workers:** Work task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> Each task ends with the quality gates and a Conventional Commit.

**Status:** in progress on `release/2.10.0` — Phase 1 core done (client, status, containers list/actions, Docker page, MCP tools); events stream (Task 1.3) next. Date: 2026-11-05.

**Target release:** **2.10.0**. The version was bumped to 2.10.0 early, at the maintainer's request, when `release/2.10.0` was created; Task 6.3 now only finalises release notes and the website.

**Goal:** Give OLS a first-class Docker area: see and control containers, images, volumes,
networks and Compose stacks, and turn any container (or a Docker template) into a normal OLS
site with its own `*.test` domain, trusted HTTPS, logs and health check.

**Architecture:** New `DockerManager` in `ols-core` that talks to the Docker Engine API
(named pipe on Windows, Unix socket elsewhere). Compose actions shell out to `docker compose`.
Docker sites reuse the existing **proxy site** (`SiteKind::Proxy`, §30) — no new web-server
code path. Everything is exposed as `CoreCommand`s, so GUI, CLI, HTTP API and MCP get it for free.

**Tech stack:** Rust (`hyper` client + `tokio` named pipe), existing `TaskManager`,
`ProcessSupervisor`, `TerminalManager`, `port` checker, `WebManager`; React + shadcn UI.

---

## 0. What already exists (we build on it)

| Piece | Where | How Docker uses it |
|---|---|---|
| Proxy site kind | `crates/ols-core/src/domain/types.rs` (`SiteKind::Proxy{upstream_port, upstream_host, upstream_https}`) | A Docker site = proxy site pointed at the container's host port |
| Self-signed upstream TLS skip | `crates/ols-core/src/web/caddy.rs:257` | Containers serving HTTPS work out of the box |
| Reverse-proxy quick app | `crates/ols-core/catalog/quick-apps/reverse-proxy.yaml` | Template for the "site from container" flow |
| Proxy mode in site dialog | `ui/src/components/site/DomainDialog.tsx:1064` | Gains a "Pick a container" picker |
| Port checker + Hyper-V/Docker excluded ranges | `crates/ols-core/src/port.rs:189` | Free host-port picking for new containers |
| Background tasks | `crates/ols-core/src/tasks.rs` (§164) | Image pulls, compose up/down, prune |
| Terminal (portable-pty) | `crates/ols-core/src/terminal.rs` | `docker exec -it` shell in a tab |
| Monitor "work happens elsewhere" | `crates/ols-core/src/monitor.rs:42` | Docker sites skip local process stats |

---

## 1. Key decisions (locked unless reviewer objects)

1. **Transport = Engine API, not CLI parsing.**
   - Windows: `\\.\pipe\docker_engine`; Linux/macOS (dev only): `/var/run/docker.sock`; or `tcp://host:port`.
   - **As built (Phase 1):** a small *blocking* HTTP/1.1 client (`docker/client.rs`), not `hyper`.
     `Core::dispatch` is synchronous, so a blocking client fits it directly. It needs **no new
     dependencies**, and each request runs on a short-lived thread with a hard timeout.
   - Why not `bollard`: large dependency, and we only need ~25 endpoints. Revisit if the list grows.
2. **Compose = CLI.** There is no Engine API for Compose. Run `docker compose -p <name> -f <file> ...`
   through `ProcessSupervisor` so output lands in the 500-line ring + logs.
3. **OLS never installs Docker.** It detects Docker Desktop / Engine, can *start* Docker Desktop,
   and links to the installer. (Docker Desktop has its own licence terms; we don't redistribute it.)
4. **Ownership labels.** Everything OLS creates gets labels:
   `dev.ols.managed=true`, `dev.ols.site=<hostname>`, `dev.ols.template=<id>`.
   Only labelled objects can be deleted from a site's "Delete" flow; unlabelled ones need the Docker page.
5. **Docker sites are proxy sites + a link.** New optional field on `Domain`:
   `docker: Option<DockerLink>` (see §3). Site start/stop/delete then drives the container too.
6. **Safety mandates (AGENTS.md §3).**
   - SHA-256: template images are pinned by digest (`image@sha256:...`); after pull we compare
     `RepoDigests` to the pinned digest and refuse on mismatch.
   - Secrets: template passwords go to the OS keyring and are passed to `docker compose` as
     **process env vars** (`${OLS_SECRET_X}` interpolation) — never written into `docker-compose.yml` or `.env`.
   - Errors: every Docker failure maps to `Diagnostic{problem,cause,fix}` (e.g. "Docker is not running" →
     fix = `docker_start_desktop`).

---

## 2. Feature list

### 2.0 The Docker page (where it lives, what it shows)

**Sidebar:** new item **Docker** in the **Sites** group, directly under *Sites*
(`ui/src/components/layout/Sidebar.tsx` `NAV_GROUPS` → `sites`):

```
Sites
  Projects
  Sites
  Docker      ← new
  Quick Apps
  Commands
  Tunnels
```

**Focus:** containers *as websites*. It is not a full Docker admin console (no Portainer clone).

**Layout, top to bottom**

1. **Status bar** (always visible)
   - 🟢 Docker running / 🔴 stopped + **Start Docker** button.
   - Counts: running containers · Docker-linked sites · disk used (disk only after Storage tab loaded it once).
   - Docker not installed → whole page becomes an empty state with a **Get Docker** link. No polling.
2. **Tab: Containers** (default)
   - Grouped by Compose stack. Filter: *All · Running · Linked to sites · Created by OLS*.
   - Row: status dot · name · image · port chips (`:8080 ↗`) · site badge (`app.test`) **or** **Create site** button ·
     quick actions (start/stop, restart, logs, shell, ⋯).
   - Click row → side panel with tabs *Overview · Logs · Shell · Stats · Env (secrets hidden)*.
3. **Tab: Templates** — "Docker apps" gallery (same look as Quick Apps). One click → name → `name.test` with HTTPS.
4. **Tab: Storage & Cleanup** — images, volumes, disk usage, **Reclaim space** (preview first), volume backup/restore.

**Links from other pages**
- *Sites* page: Docker badge + container state on Docker sites; filter "Docker sites".
- *Add site* dialog (proxy mode): **Pick a container**.
- *Projects* page: **Run with Docker** when a `Dockerfile` / compose file is detected.

### 2.1 Docker manager (the "nice features")

**Status & setup**
- Detect: installed? running? version, OS type (Linux/Windows containers), WSL2 backend, context.
- One-click **Start Docker Desktop** and wait until the engine answers (task with progress).
- Status shown on the Docker page + tray entry (cached value, no extra probing); Dashboard card only
  when Docker is installed (running/total containers; disk usage only if already cached).

**Containers**
- List with state, image, ports, uptime, CPU/RAM (from `/containers/{id}/stats?stream=false`).
- Start / stop / restart / pause / unpause / remove (remove asks to confirm; optional "also remove volumes").
- Live logs (follow, search, timestamps) — reuses the Logs page as source `docker:<container>`.
- **Shell into container**: opens an OLS terminal tab running `docker exec -it <id> sh|bash`.
- Inspect (env vars redacted via `redact.rs`, mounts, networks, labels) in a read-only panel.
- "Open in browser" for each published port; "Copy connection string" for DB containers.
- Group by Compose project; filter by running / OLS-managed / all.

**Images & volumes** (Storage & Cleanup tab)
- Images: list (size, tags, created, in use?), pull with layer progress (TaskManager), remove, prune dangling.
- Volumes: list, size (from `/system/df`), remove unused, prune with **dry-run preview** of what will go.
- Volume backup/restore to a `.tar.gz` (via throwaway `alpine` container, pinned digest).
- Networks: read-only list inside container details (no networks editor in v1).

**Compose**
- Detect `compose.yaml` / `docker-compose.yml` in registered projects (new detection marker).
- Up / down / restart / pull / logs per stack and per service; show services + their ports.

**Cleanup & health**
- Disk usage panel (`/system/df`) + "Reclaim space" wizard (prune images/containers/volumes/build cache,
  each with a size preview).
- Diagnostics rules: Docker not running; container port collides with an OLS service port;
  host port inside a Windows excluded range; Docker site's container gone/stopped.
- Engine **events stream** (`/events`) → new Tauri event `docker-event` so the UI updates live.

### 2.2 Create a site from Docker (three entry points)

**A. From a running container** (fastest win)
1. Sites → Add site → *Reverse proxy* → **"Pick a container"**.
2. User picks container + one of its published ports (unpublished ports are listed with a hint:
   "Publish this port or let OLS recreate the container with it").
3. OLS creates a proxy site `upstream=127.0.0.1:<hostPort>` (https auto-detected by a probe),
   stores `DockerLink`, runs the normal web pipeline (config → validate → reload → cert → DNS → health).

**B. From a Docker template** ("Docker apps" catalog)
- New catalog `crates/ols-core/catalog/docker-templates/*.yaml`, same strict-schema + Minijinja style
  as quick-apps. Starter set: WordPress + MariaDB, Ghost, n8n, Uptime Kuma, Gitea, Directus,
  Metabase, Adminer, Nextcloud.
- Flow: pick template → answers form (name, domain, version) → **plan preview** (images + digests,
  host ports, volumes, files to be written) → run as a background task:
  pull + verify digest → write `<sites_dir>/<name>/compose.yaml` → `docker compose up -d`
  → wait for container health → create proxy site with `DockerLink` → open the site.
- Ports: host ports chosen with `port.rs` (skips used + excluded ranges); bound to `127.0.0.1` only.

**C. From a project folder** (Dockerfile / compose already in the repo)
- Detection marks the project `docker-compose` or `dockerfile`.
- Project page gets **"Run with Docker"**: choose service + port → same as A, plus up/down buttons.
- Dockerfile-only: `docker build -t ols/<project>:dev .` (task with output) then run with a published port.

**Shared site behaviour**
- Site toggle on/off = `docker start/stop` (or `compose up/down`) + web config enable/disable.
- Site delete offers: keep container / remove container / remove container + volumes (labelled only).
- Health check adds a step: "container running & healthy" before DNS→TCP→TLS→HTTP.
- If the container restarts with a different host port, the event stream triggers a re-resolve and web re-apply.
- Containers can reach OLS: template compose files add
  `extra_hosts: ["host.docker.internal:host-gateway"]` and docs show how to reach OLS MariaDB/Redis/Mailpit.
- Site bundles (§165): export includes `compose.yaml` + optional volume tarballs; import recreates the stack.

### 2.3 Out of scope for v1

Rarely needed for local sites, and each one makes the page heavier:
- Networks editor (create/connect/disconnect).
- Image build UI on the Docker page (Dockerfile projects build from the *Projects* page instead).
- Registry login / private registries.
- Editing a container's config (Docker must recreate the container for that).
- Swarm and Kubernetes.

Power users can still do all of these with the `docker` CLI.

---

## 2.5 Performance rules (binding — Docker must not slow down OLS)

**Goal:** a user who never opens the Docker page pays **zero** cost. A user who does pays only while looking.

### Startup
- **No Docker work at app start.** `DockerManager` is created lazily on first Docker command.
  No `/_ping`, no pipe connect, no catalog load during `startup.rs`.
- **Exception (Phase 3):** if at least one site has a `DockerLink`, a single status check runs **after** the UI is ready
  (`ols:ui-ready`), in the background, with a 2 s timeout.
- Docker templates catalog is parsed on first open of the *Templates* tab, then cached.

### Network calls to the engine
- Every call runs on its own short-lived thread with a hard timeout — never on the Tauri main thread, never holding a store lock.
- Hard timeouts (as built): 2 s status (`/version`), 3 s lists, 20 s actions (stop waits up to 10 s for the container); long work (pull, compose, prune, `/system/df`) runs as a background task.
- **One** list call per refresh: `/containers/json` already has ports + labels → no per-container `inspect` in the list.
- `/system/df` is slow (can take seconds): only on *Storage* tab open or the refresh button; cached 60 s.
- Results cached in memory (containers 2 s, images/volumes 30 s); repeated UI calls inside that window hit the cache.

### Live updates (no polling loops)
- **Events stream** (`/events`) is the only live feed. It is open **only while** the Docker page is mounted
  **or** a Docker-linked site exists. Closed when neither is true.
- Events are debounced: at most one `docker-event` to the UI every 250 ms (batched).
- **Stats** (CPU/RAM): never for all containers. Only for the container whose *Stats* tab is open,
  one `stream=false` call every 3 s, stopped when the panel closes.
- **Logs:** start with the last 500 lines, follow only while the *Logs* tab is visible, capped ring buffer, virtualized list.
- Everything pauses when the window is hidden / minimized to tray (`document.visibilityState`).

### Docker not running / not installed
- Not installed → status cached for the session; no retries.
- Not running → no retry loop. Re-check only on user action (page open, **Start Docker**, refresh)
  or, for Docker-linked sites, backoff 5 s → 10 s → 30 s → 60 s max.
- A missing engine never delays site health checks for non-Docker sites.

### UI bundle
- `Docker.tsx` and `components/docker/*` load with `React.lazy` → zero bytes added to the initial bundle.
- Container table virtualized above 50 rows. Template icons small `.webp`.

### Dependencies
- No `bollard`, no new heavy crates — `hyper` / `hyper-util` are already in the tree.

### Budgets (checked in review)

| Metric | Budget |
|---|---|
| App startup time | +0 ms (no Docker work on the startup path) |
| Idle CPU with Docker page closed, no Docker sites | 0 (no timers, no open connections) |
| Extra memory with Docker page closed | < 1 MB |
| Docker page first paint, 100 containers | < 300 ms |
| Initial UI bundle size | ≤ +5 KB (sidebar icon + lazy loader only). Measured in Phase 1: +3.5 KB raw / +1.2 KB gzip; page itself is an 8.5 KB lazy chunk |

### Performance tests
- Fake-engine test: start `Core`, run startup, assert the fake engine received **0** requests.
- Test: events stream closes when the last subscriber (page) unsubscribes and no Docker sites exist.
- Test: stats polling stops when the panel closes.
- Test: cached list served without a second engine request inside the cache window.
- `npm run build` check: Docker code is in its own chunk, not in the entry chunk.

---

## 3. Data model changes

```rust
// crates/ols-core/src/docker/types.rs
pub struct DockerStatus { installed: bool, running: bool, version: Option<String>,
                          os_type: Option<String>, context: Option<String>, desktop_path: Option<PathBuf> }
pub struct ContainerSummary { id, name, image, state, status, ports: Vec<PortBinding>,
                              labels: BTreeMap<String,String>, compose_project: Option<String>, ols_site: Option<String> }
pub struct PortBinding { container_port: u16, protocol: String, host_ip: Option<String>, host_port: Option<u16> }
pub struct ImageSummary { id, tags: Vec<String>, size: u64, created: i64, in_use: bool }
pub struct VolumeSummary { name, driver, size: Option<u64>, in_use: bool, labels }
pub struct NetworkSummary { id, name, driver, containers: usize }
pub struct ComposeStack { name, file: PathBuf, services: Vec<ComposeService>, project_id: Option<String> }
pub struct DiskUsage { images: u64, containers: u64, volumes: u64, build_cache: u64, reclaimable: u64 }

// crates/ols-core/src/domain/types.rs (new optional field on Domain, serde default = None)
pub enum DockerLink {
    Container { name: String, container_port: u16 },
    Compose   { project: String, file: PathBuf, service: String, container_port: u16 },
}

// crates/ols-core/src/docker/template.rs (catalog schema)
pub struct DockerTemplate { id, name, description, icon, category,
    images: Vec<PinnedImage /* name + sha256 digest */>, questions: Vec<Question>,
    compose: String /* Minijinja */, web_service: String, web_port: u16,
    secrets: Vec<String>, health_path: Option<String> }
```

Settings keys: `docker.enabled` (bool, default auto), `docker.endpoint` (override pipe/socket),
`docker.desktop_path`, `docker.events` (bool).

---

## 4. CoreCommand additions

| Group | Commands |
|---|---|
| Status | `get_docker_status` ✅, `start_docker_desktop` ✅, `docker_disk_usage`, `docker_watch{on}` (page subscribes/unsubscribes to live events) |
| Containers | `list_docker_containers{all}` ✅, `docker_container_action{id,action}` ✅ (start/stop/restart/pause/unpause; no kill), `docker_remove_container{id,volumes,force}`, `docker_inspect_container{id}`, `docker_container_stats{id}`, `docker_container_logs{id,tail,since}`, `docker_open_shell{id,shell}` |
| Images | `docker_list_images`, `docker_pull_image{reference}` → `TaskStarted`, `docker_remove_image{id,force}` |
| Volumes / networks | `docker_list_volumes`, `docker_remove_volume{name}`, `docker_backup_volume{name,dest}`, `docker_restore_volume{name,source}`, `docker_list_networks` |
| Prune | `docker_prune_plan{kinds}` (dry run), `docker_prune{kinds}` → `TaskStarted` |
| Compose | `docker_list_compose`, `docker_compose_action{file,project,action,service?}` → `TaskStarted` |
| Templates | `docker_list_templates`, `docker_plan_template{id,answers}`, `docker_run_template{id,answers}` → `TaskStarted` |
| Sites | `docker_create_site{container,container_port,hostname,https?}`, `docker_link_site{hostname,link}`, `docker_unlink_site{hostname}` |

> **Naming (as built):** read commands use the existing `get_` / `list_` prefixes (`get_docker_status`,
> `list_docker_containers`) so the HTTP API treats them as read-only automatically. Later read commands follow the same rule.

**HTTP API / MCP gating**
- Read-only mode: `docker_status`, `docker_list_*`, `docker_inspect_*`, `docker_disk_usage`.
- Operate mode: start/stop/pull/compose/template/site commands.
- **Never** over HTTP/MCP: `docker_open_shell`, `docker_remove_volume`, `docker_prune` (destructive / interactive).
- MCP tools: `ols_docker_status` ✅, `ols_docker_start` ✅, `ols_docker_list_containers` ✅, `ols_docker_container_action` ✅,
  later `ols_docker_create_site`, `ols_docker_list_templates`, `ols_docker_run_template`.

**CLI**
```bash
olsc docker status
olsc docker ps [--all]
olsc docker start|stop|restart|logs <container>
olsc docker images | pull <ref>
olsc docker compose up|down|logs [--project <name>]
olsc docker template list | run <id>
olsc docker site <container> --port 8080 --domain app.test
olsc docker prune --dry-run
```

---

## 5. Tasks (phased — each phase is shippable on its own)

### Phase 1 — Engine client + status (foundation)

- [x] **Task 1.1: Docker client transport** (done: blocking client, see decision 1)
  - Create `crates/ols-core/src/docker/{mod.rs,client.rs,types.rs,error.rs}`.
  - `DockerClient::connect(endpoint)`; `get_json<T>`, `post`, `delete`, streaming `get_lines` (logs/events/pull).
  - Cargo: `hyper` add `client`; `hyper-util` add `client-legacy`, `http1`; tokio `net` already on.
  - Tests: unit-test request building + error mapping with a fake in-process HTTP server (no Docker needed).
- [x] **Task 1.2: `DockerManager` + status**
  - Add to `Inner` as a plain field whose creation does **no I/O** (as built; no `OnceCell` needed) — first engine request on first Docker command (§2.5).
  - Response cache (containers 2 s, images/volumes 30 s, `/system/df` 60 s) + hard timeouts per call.
  - `docker_status`, `docker_start_desktop` (find `Docker Desktop.exe` via registry/Program Files, launch, poll `/_ping` ≤ 120 s as a task).
  - Diagnostics: map "pipe not found" → `Diagnostic{problem:"Docker is not running", cause, fix:"docker_start_desktop"}`.
- [ ] **Task 1.3: Events stream** → `docker-event` broadcast; Tauri forwards it (`src-tauri`).
  - Ref-counted: `docker_watch{on}` from the page + "Docker-linked sites exist" → open; both gone → close.
  - Debounce/batch to one UI event per 250 ms.
- [x] **Task 1.4: Performance guard tests** (done: `app_startup_makes_no_docker_requests`, cache test, hung-engine timeout test; events-stream test lands with Task 1.3) (§2.5): zero engine requests during startup; stream closes on last
  unsubscribe; cache hit inside window. These stay in CI so later phases can't regress them.

### Phase 2 — Manager UI (containers, images, volumes, networks)

- [ ] **Task 2.1:** (containers ✅ done in Phase 1) image/volume/network list + action commands (`command/types/commands.rs`, `command/dispatch/docker.rs`).
- [ ] **Task 2.2:** logs as a log source `docker:<name>` in `app/system/logs.rs`.
- [ ] **Task 2.3:** `docker_open_shell` via `TerminalManager` (`docker exec -it`), shell fallback bash → sh.
- [ ] **Task 2.4:** (partly done: status bar, empty states, Containers tab with filters, Compose groups, port chips and start/stop/restart/pause; still to do: side panel, Templates and Storage tabs, virtualization) UI page per §2.0: `ui/src/pages/Docker.tsx` (loaded with `React.lazy`) with status bar +
  tabs *Containers · Templates · Storage & Cleanup*; components under `ui/src/components/docker/`
  (`DockerStatusBar`, `ContainerTable` (virtualized > 50 rows, grouped by Compose stack), `ContainerPanel`
  (Overview/Logs/Shell/Stats/Env), `PullImageDialog`, `PruneWizard`).
  Sidebar item **Docker** in the *Sites* group under *Sites* (`Sidebar.tsx`, new `DockerIcon` in `SidebarIcons.tsx`);
  command-palette actions; empty state with "Start Docker" / "Get Docker".
  Stats/logs polling only while their tab is visible; pause when the window is hidden.
- [ ] **Task 2.5:** Dashboard card (only when Docker is installed, cached values) + tray submenu (Start Docker, running count).
- [ ] **Task 2.6:** Prune plan/run + disk usage + volume backup/restore (pinned `alpine@sha256:` helper image).

### Phase 3 — Create site from a running container (entry point A)

- [ ] **Task 3.1:** `DockerLink` on `Domain` (serde default, migration-free) + validation.
- [ ] **Task 3.2:** `docker_create_site`: resolve host port → build `SiteKind::Proxy` → existing domain add + web apply; set label only if OLS created the container.
- [ ] **Task 3.3:** site lifecycle hooks: enable/disable/delete drive the container; health step "container running".
- [ ] **Task 3.4:** host-port re-resolve on `docker-event` (`start` / `die`) → re-apply web for that site only.
- [ ] **Task 3.5:** UI: "Pick a container" in `DomainDialog.tsx` proxy mode; "Create site" button on each container row; Docker badge + container status on site cards.

### Phase 4 — Docker templates (entry point B)

- [ ] **Task 4.1:** template schema + loader `crates/ols-core/src/docker/template.rs`, catalog dir `crates/ols-core/catalog/docker-templates/`; strict schema tests (unknown keys rejected, every image has a digest).
- [ ] **Task 4.2:** plan (review screen data) + run task: pull → verify digest → render compose → keyring secrets → `compose up -d` → wait healthy → create site. Journal each step for rollback (`journal.rs`).
- [ ] **Task 4.3:** first 6 templates: WordPress+MariaDB, Ghost, n8n, Uptime Kuma, Gitea, Adminer.
- [ ] **Task 4.4:** UI "Docker apps" gallery (reuse Quick Apps wizard layout), template icons as `.webp`.

### Phase 5 — Project Compose / Dockerfile (entry point C)

- [ ] **Task 5.1:** detection markers `compose.yaml`, `docker-compose.yml`, `Dockerfile` in `detection.rs` (read-only).
- [ ] **Task 5.2:** compose list/actions + "Run with Docker" on the project page; Dockerfile build task.
- [ ] **Task 5.3:** site bundle export/import of compose + optional volume tarballs.

### Phase 6 — Front doors, docs, release

- [ ] **Task 6.1:** CLI `olsc docker ...` (`crates/ols-cli`), HTTP API gating list in `api.rs`, MCP tools in `crates/ols-mcp`.
- [ ] **Task 6.2:** User manual section "Docker" (`crates/ols-core/catalog/user_manual.md` + `docs/OLS_USER_MANUAL.md`), troubleshooting entries (Docker not running, WSL2 off, port excluded).
- [ ] **Task 6.3: Release 2.10.0** — version bump + release-notes sync per AGENTS.md §5, all in one commit:
  - Version `2.9.0` → `2.10.0` in root `Cargo.toml` (`[workspace.package]`), `src-tauri/tauri.conf.json`,
    `ui/package.json`, `ui/package-lock.json` (preferably via `scripts/update-version.bat`).
  - New `release-notes/v2.10.0.md` (headline: Docker page + create sites from Docker), and a matching
    `## [2.10.0] — YYYY-MM-DD` section in `CHANGELOG.md`.
  - `crates/ols-core/catalog/user_manual.md` and `docs/OLS_USER_MANUAL.md`: header `> **Version:** 2.10.0`,
    Table of Contents, Section 17 "Version 2.10.0 Specific Notes & What's New" (identical in both files).
  - `ui/src/pages/Settings.tsx`: `releaseNotes` import → `../../../release-notes/v2.10.0.md?raw`,
    version badge fallback and `get_app_guide` version → `2.10.0`.
  - `index.html`, `website/`: version badges, `OLS-2.10.0-setup.exe` / portable zip links, SHA-256 checksums, footer.
  - `README.md`: badge `version-2.10.0-blue`.

---

## 6. Specs-sync checklist (AGENTS.md §1 — every phase PR)

- `specs/full_documentation.md` — new Docker section (business rules, safety, labels, gating).
- `specs/architecture_overview.md` — add `DockerManager` to managers + module table row (`ols-core: docker/*`).
- `specs/catalog.txt`, `specs/relationships.txt` — new files and their edges (DockerManager → WebManager, TaskManager, TerminalManager, port, Domain store).
- `specs/data_models.txt` — `DockerLink`, `DockerTemplate`, summaries, settings keys.
- `specs/api_reference.md` — new commands, `docker-event`, CLI, HTTP/MCP gating.
- `specs/diagrams/*.mmd` — add Docker manager + "site from container" sequence diagram.
- `specs/runtime.md` — log line; Files Processed == Files to Process.

## 7. Quality gates (every task)

```bash
cargo fmt --all
cargo clippy -p ols-core -p ols-helper --all-targets
cargo test -p ols-core -p ols-helper
cd ui && npm run lint && npm run build
```
(Do not run `cargo test` while `cargo tauri dev` is running.)

## 8. Testing strategy

- **Unit (no Docker):** request building, JSON parsing from recorded fixtures, error → Diagnostic mapping, template schema, compose rendering, port picking, `DockerLink` serde round-trip.
- **Fake engine:** tiny hyper server in `test_support.rs` answering `/_ping`, `/containers/json`, `/events` — drives manager + site-creation tests in CI.
- **Manual (real Docker Desktop on Windows):** checklist in `testing/docker.md` — start Desktop, run each template, site from container, port change re-resolve, delete flows, prune dry-run.

## 9. Risks & open questions

| Risk | Mitigation |
|---|---|
| Docker Desktop not installed / licence | Detect + link only; feature hides cleanly when absent |
| Windows-containers mode | Detect `OSType`; templates require Linux mode, show a fix diagnostic |
| Host port drift after restart | Event-driven re-resolve (Task 3.4) |
| Secrets in compose files | Env-var interpolation from keyring only; test asserts rendered file has no secret values |
| Destructive actions via API/MCP | Excluded from HTTP/MCP gating list |
| Docker slows down the app | §2.5 rules: lazy manager, no startup calls, no polling loops, lazy UI chunk, guard tests in CI (Task 1.4) |
| Template digests go stale | `refresh_catalogs` can ship updated signed template catalogs (minisign, like other catalogs) |

**Open questions for the maintainer**
1. Should Phase 1–2 ship before site creation, or ship Phase 1+3 first (smaller, highest value)?
2. Podman support later (same Engine API) — worth keeping the endpoint configurable from day one? (Plan says yes.)
3. Which 6 templates first?
