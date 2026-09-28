# Multi-Server Web (Apache + Nginx + Caddy in parallel) — Design

Date: 2026-09-28
Status: approved design, pending implementation plan
Approach: A — extend WebManager (multi-running)

## 1. Goal

Run Apache, Nginx, Caddy together like Laragon. Each service starts/stops
independently, owns its ports. One server is the default. Each site uses the
default unless it overrides to a specific server. Exclusive binding: a site is
rendered only on its assigned server.

Decisions locked:
- Ports: selected default always binds 80/443 effective; non-default servers keep stored custom ports. Stored custom ports preserved when server leaves default role.
- Routing: exclusive, not mirrored.
- Control: web servers appear in Services list, backed by WebManager.

## 2. Current state

- `WebConfig { server, http_port, https_port, php_workers, dns_port }` (`crates/ols-core/src/web/mod.rs`).
  Comment: "Only one runs at a time (they share 80/443)."
- `WebManager.state.server: Option<Running>` + `stop_other_server()` (`manager.rs:353`, `602-610`).
  `apply()` renders all enabled domains for the single active server.
- `Domain` has no server field; `generated_hashes: BTreeMap<server_id, hash>` already
  keyed per server (`domain.rs:174-177`).
- `ServiceManager` handles mailpit/mariadb/postgres/mongodb/redis independently;
  web servers are not in that list.

## 3. Changes

### 3.1 Data model (`web/mod.rs`, `domain.rs`, settings)

```rust
struct ServerPorts { http: u16, https: u16 }

struct WebConfig {
  default_server: String,              // "nginx" | "apache" | "caddy"
  servers: BTreeMap<String, ServerPorts>,
  php_workers: u16,
  dns_port: u16,
}

struct Domain {
  // ... existing ...
  server: Option<String>, // None = use default_server
}
```

- Migration on load: `web.server` → `default_server`; `web.http_port/https_port` →
  `servers[default]`. Other servers get defaults: apache 8080/8443, caddy 8081/8444,
  nginx (non-default) 8082/8445 (or next-free check at first start). `Domain.server` defaults None.
- Effective-port rule (binding): `effective_ports(id) = 80/443 if id == default_server else servers[id]`.
  Stored custom ports never overwritten by default role; switching default only changes
  which server resolves to 80/443. Switching default requires restart of affected servers
  (old default frees 80/443, new default binds them).
- Validation: unknown server id → `Diagnostic{problem,cause,fix}`; uninstalled
  assigned server → fallback to default + warning, never silent drop.
- Keep `server` field accepted as deprecated alias for one release (read + warn).

### 3.2 WebManager (`web/manager.rs`)

- `State { servers: HashMap<String, Running>, ... }` replaces single `server`.
- Remove `stop_other_server()`. Add `start(id)`, `stop(id)`, `is_running(id)`.
- `apply()`: group enabled domains by resolved server
  (`d.server || cfg.default_server`), then per-server pipeline:
  render → write (archive) → validate → start_or_reload — same §28 semantics,
  rollback scoped per server so one bad server never touches others.
- `start_or_reload()`: per-server port check (`ServerPorts`), `wait_ready` on that
  server's HTTP port. Conflict error names owner process + fix (stop it / change ports).
- `status()` → `servers: Vec<ServerAvailability + running + ports>` plus global
  `running` = any alive. `WebStatus.server` becomes `default_server` (compat).
- `list_configs/read/write/set_ownership/history` take resolved server id, not
  global cfg only. History dirs already per-server — no change.
- PHP pools (`PhpPools`) and certs (`CertificateManager`) stay shared across servers.
- DNS/hosts (`sync_dns`) runs once globally, not per server.

### 3.3 Services surface (`service.rs`, `app.rs`, `command.rs`)

- `ServiceManager::list/status/start/stop` gains `nginx/apache/caddy` as proxied
  entries delegating to `WebManager` (no duplicated supervisor state).
- `ServiceStatus.kind = "web"`, `port` = that server's HTTP port, `healthy` = TCP probe.
- IPC: extend `GetWebConfig`/`SetWebConfig` (or new `SetDefaultServer`,
  `SetServerPorts`, `SetDomainServer`) + `StartService/StopService` routing for web ids.
- `site_url()` uses assigned server's ports; non-default ports always shown in URL.

### 3.4 UI (`ui/src/`)

- Services page: three web rows with independent Start/Stop + running dot + port.
- Web settings: default-server picker + per-server HTTP/HTTPS fields + conflict hints.
- Site/domain form: server dropdown (Default / Nginx / Apache / Caddy).
- Logs page: per-server error-log tail selector.

## 4. Ports

Stored custom ports:

| Server | HTTP | HTTPS |
|--------|------|-------|
| apache | 8080 | 8443 |
| caddy | 8081 | 8444 |
| nginx | 8082 | 8445 |

Effective rule: whichever server is selected default binds 80/443 regardless of stored
values; all non-default servers bind stored custom ports. Stored values editable only
for non-default servers (default row shows 80/443 locked). First start probes `check_port`;
conflict → Diagnostic, no auto-steal. `site_url()` omits port for default (80/443),
always shows port for non-default.

## 5. Error handling

All user errors as `Diagnostic{problem,cause,fix}` per AGENTS.md §3:
- Port in use: what/why (owner PID/name) + how (stop owner / change ports).
- Binary missing: which server/version + install path (Runtimes page).
- Validate fail: server output + rollback note ("nothing changed").
- Unknown/retired server id on domain: fallback + warning.

SHA-256 download verify, OS keyring secrets: unchanged paths.

## 6. Testing

- `cargo test -p ols-core`: migration old→new config; per-server grouping
  (exclusive — file exists only on assigned server); port-conflict Diagnostic;
  unknown server fallback; multi-`Running` start/stop isolation.
- `smoke_web` example extended: start nginx+apache together, distinct ports,
  per-site override serves only on assigned server.
- UI: `npm run lint && npm run build`.

## 7. Specs-sync (binding per AGENTS.md §1)

Same PR updates: `specs/full_documentation.md`, `specs/architecture_overview.md`,
`specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt`,
`specs/api_reference.md` (if `CoreCommand` changes), `specs/diagrams/*.mmd`,
plus `specs/runtime.md` log line with Files Processed == Files to Process.

## 8. Out of scope (YAGNI)

- No front-proxy routing all traffic through default.
- No mirrored serving on all servers.
- No dependency-ordered startup (§68 still deferred).
- No per-server PHP/cert stores.

## 9. Task list (for implementation plan)

1. Migrate `WebConfig` + settings keys, keep compat alias.
2. Add `Domain.server`, validation + fallback.
3. `WebManager` multi-running + per-server apply pipeline.
4. Services proxy entries + IPC commands + `site_url`.
5. UI: Services rows, Web settings, domain dropdown, logs selector.
6. Tests + quality gates + specs-sync.
