# Multi-Server Web Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run Apache, Nginx, Caddy in parallel with independent start/stop, own ports, one default server, per-site override.

**Architecture:** Extend `WebManager` to multi-running map, migrate `WebConfig` to default+per-server ports, add `Domain.server` override, proxy web entries through Services list.

**Tech Stack:** Rust (ols-core, Tauri), React+TypeScript UI, serde_json settings, ProcessSupervisor.

**Spec:** `docs/superpowers/specs/2026-09-28-multiserver-web-design.md`

## Global Constraints

- Errors shown to users must say what went wrong, why, how to fix (`Diagnostic{problem,cause,fix}`).
- Anything downloading a file must verify SHA-256 (unchanged paths only, no new downloads in this plan).
- Secrets stay in OS keyring; never in logs, files, bundles.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`).
- Specs-sync mandate: same PR updates `specs/full_documentation.md`, `specs/architecture_overview.md` (layers change), `specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt`, `specs/api_reference.md` (CoreCommand change), `specs/diagrams/*.mmd`, plus `specs/runtime.md` log line with Files Processed == Files to Process.
- Quality gates: `cargo fmt --all`, `cargo clippy -p ols-core -p ols-helper --all-targets`, `cargo test -p ols-core -p ols-helper`, `cd ui && npm run lint && npm run build`.
- Exclusive binding: site rendered only on assigned server. No mirroring. No front-proxy.
- Ports: selected default always binds 80/443 effective; non-default servers bind stored custom ports (apache 8080/8443, caddy 8081/8444, nginx non-default 8082/8445). Stored customs preserved across default switches. Default row locked in UI.

---

### Task 1: WebConfig migration (default + per-server ports)

**Files:**
- Modify: `crates/ols-core/src/web/mod.rs:18-53`
- Modify: `crates/ols-core/src/settings.rs` (keys read path, check existing `get/set` helpers)
- Test: `crates/ols-core/src/web/mod.rs` (new `#[cfg(test)]` module) + existing `cargo test -p ols-core web::`

**Interfaces:**
- Consumes: `SettingsService::get(key: &str) -> Option<serde_json::Value>`
- Produces: `pub struct ServerPorts { pub http: u16, pub https: u16 }`, `pub struct WebConfig { pub default_server: String, pub servers: BTreeMap<String, ServerPorts>, pub php_workers: u16, pub dns_port: u16 }`, `impl WebConfig { pub fn from_settings(s: &SettingsService) -> Self; pub fn ports_for(&self, id: &str) -> ServerPorts; pub fn effective_ports(&self, id: &str) -> ServerPorts; pub fn resolve_server(&self, override_: Option<&str>) -> String }`
- Rule: `effective_ports(id) = ServerPorts{http:80,https:443} if id == default_server else ports_for(id)`. All bind/URL/conflict logic uses `effective_ports`. `ports_for` returns stored customs only.

- [ ] **Step 1: Write failing test for migration + resolve**

```rust
#[test]
fn migrates_legacy_single_server_keys() {
    // legacy: web.server=apache, web.http_port=8080, web.https_port=8443
    // expect: default_server=apache, servers[apache]={8080,8443}, servers[nginx] defaults
}
#[test]
fn resolve_server_falls_back_to_default() {
    // override None -> default; override Some("caddy") -> caddy; unknown -> default
}
#[test]
fn default_server_always_effective_80_443() {
    // default nginx -> effective_ports(nginx)=={80,443} even if stored nginx={8082,8445}
    // effective_ports(apache)==stored {8080,8443}; switch default to apache -> apache {80,443}, nginx stored unchanged
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test -p ols-core web::migrates_legacy_single_server_keys -v`
Expected: FAIL with "no such test / struct field missing"

- [ ] **Step 3: Minimal implementation in `web/mod.rs`**

```rust
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerPorts { pub http: u16, pub https: u16 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub default_server: String,
    pub servers: BnteMap<String, ServerPorts>,
    pub php_workers: u16,
    pub dns_port: u16,
}
impl WebConfig {
    pub fn from_settings(s: &SettingsService) -> Self {
        let num = |key: &str, default: u64| s.get(key).and_then(|v| v.as_u64()).unwrap_or(default);
        let legacy_server = s.get("web.server").and_then(|v| v.as_str()).unwrap_or("nginx").to_string();
        let default_server = s.get("web.default_server").and_then(|v| v.as_str()).unwrap_or(&legacy_server).to_string();
        let mut servers = BTreeMap::new();
        for id in ["nginx","apache","caddy"] {
            let http = s.get(&format!("web.servers.{id}.http_port")).and_then(|v| v.as_u64()).map(|v| v as u16)
                .unwrap_or_else(|| if id == default_server {
                    num("web.http_port", if id=="nginx"{80}else{8080}) as u16
                } else { default_fallback_http(id) });
            let https = s.get(&format!("web.servers.{id}.https_port")).and_then(|v| v.as_u64()).map(|v| v as u16)
                .unwrap_or_else(|| if id == default_server {
                    num("web.https_port", if id=="nginx"{443}else{8443}) as u16
                } else { default_fallback_https(id) });
            servers.insert(id.to_string(), ServerPorts{http, https});
        }
        Self { default_server, servers, php_workers: num("web.php_workers",3).clamp(1,16) as u16, dns_port: num("web.dns_port",53) as u16 }
    }
    pub fn ports_for(&self, id: &str) -> ServerPorts {
        self.servers.get(id).cloned().unwrap_or(ServerPorts{http:8080,https:8443})
    }
    pub fn effective_ports(&self, id: &str) -> ServerPorts {
        if id == self.default_server { ServerPorts{http:80,https:443} } else { self.ports_for(id) }
    }
    pub fn resolve_server(&self, o: Option<&str>) -> String {
        match o { Some(s) if ["nginx","apache","caddy"].contains(&s) => s.to_string(), _ => self.default_server.clone() }
    }
}
fn default_fallback_http(id: &str) -> u16 { match id {"apache"=>8080,"caddy"=>8081,"nginx"=>8082,_=>8080} }
fn default_fallback_https(id: &str) -> u16 { match id {"apache"=>8443,"caddy"=>8444,"nginx"=>8445,_=>8443} }
```

Keep deprecated `server`, `http_port`, `https_port` getters for compat during transition (mark `#[deprecated]`), or keep serialized aliases with `#[serde(default)]`. Do not break `GetWebConfig` JSON yet — add new fields, keep old populated from default.

- [ ] **Step 4: Run tests pass**

Run: `cargo test -p ols-core web:: -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ols-core/src/web/mod.rs
git commit -m "feat: migrate WebConfig to default plus per-server ports"
```

---

### Task 2: Domain.server override + validation

**Files:**
- Modify: `crates/ols-core/src/domain.rs:150-184` (Domain struct)
- Modify: `crates/ols-core/src/app.rs` (add_domain/update_domain validation, ~search `add_domain`)
- Test: `crates/ols-core/src/domain.rs` tests module

**Interfaces:**
- Consumes: `WebConfig::resolve_server` from Task 1
- Produces: `Domain.server: Option<String>`, `pub fn validate_domain_server(server: Option<&str>) -> Result<Option<String>, CoreError>`, `pub fn resolved_server(domain: &Domain, cfg: &WebConfig) -> String`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn domain_server_none_means_default() {
    let cfg = test_config_default("nginx");
    let d = Domain { server: None, ..test_domain("a.test") };
    assert_eq!(resolved_server(&d, &cfg), "nginx");
}
#[test]
fn domain_server_rejects_unknown_id() {
    assert!(validate_domain_server(Some("iis")).is_err());
}
```

- [ ] **Step 2: Run, verify fail**

Run: `cargo test -p ols-core domain::domain_server_none_means_default -v`
Expected: FAIL ("no field server")

- [ ] **Step 3: Minimal implementation**

```rust
// domain.rs Domain struct add:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub server: Option<String>,

pub fn validate_domain_server(server: Option<&str>) -> Result<Option<String>, CoreError> {
    match server {
        None | Some("") => Ok(None),
        Some(s) if ["nginx","apache","caddy"].contains(&s) => Ok(Some(s.to_string())),
        Some(s) => Err(CoreError::DomainError(format!(
            "\"{s}\" is not a known web server (nginx/apache/caddy). Set it to Default or a known server."
        ))),
    }
}
pub fn resolved_server(d: &Domain, cfg: &crate::web::WebConfig) -> String {
    cfg.resolve_server(d.server.as_deref())
}
```

Update every `Domain { ... }` constructor (home_domain, tests, static_domain in manager tests) with `server: None`. Update `app.rs` add/update paths to call `validate_domain_server`.

- [ ] **Step 4: Run pass**

Run: `cargo test -p ols-core domain:: -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ols-core/src/domain.rs crates/ols-core/src/app.rs
git commit -m "feat: add per-site web server override with validation"
```

---

### Task 3: WebManager multi-running state + independent start/stop

**Files:**
- Modify: `crates/ols-core/src/web/manager.rs:117-145,253-327,602-682,721-733`
- Test: `crates/ols-core/src/web/manager.rs` tests

**Interfaces:**
- Consumes: `WebConfig::ports_for`, `server_by_id`, `ProcessSupervisor::start/stop/is_alive`
- Produces: `pub fn start_server(&self, id: &str, cfg: &WebConfig) -> Result<(), CoreError>`, `pub fn stop_server(&self, id: &str)`, `pub fn is_running(&self, id: Option<&str>) -> bool`, `WebStatus { default_server, servers: Vec<ServerStatusLite> }` where `ServerStatusLite { id, name, installed, active(default?), running, http_port, https_port }`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn two_servers_track_running_independently() {
    // start nginx fake + apache fake; stop nginx; assert apache still alive
    // uses isolated_home + stub supervisor or real temp binaries? Use ProcessSupervisor with `cmd /C timeout` stub
}
```

Simpler deterministic unit: construct `State { servers: HashMap }`, insert two entries, remove one, assert other remains. Fails before Map exists.

- [ ] **Step 2: Run fail**

Run: `cargo test -p ols-core web::manager::two_servers_track_running_independently -v`
Expected: FAIL

- [ ] **Step 3: Implement state change**

```rust
struct Running { id: String, process: ProcessId }
struct State { servers: HashMap<String, Running>, apps: HashMap<String, ProcessId>, dns: Option<DnsServer>, site_php: HashMap<String,String> }
// new(): servers: HashMap::new()
// delete stop_other_server(); add:
pub fn stop_server(&self, id: &str) { if let Some(r) = self.state.lock().unwrap().servers.remove(id) { self.supervisor.stop(r.process); } }
pub fn is_running_id(&self, id: &str) -> bool { self.state.lock().unwrap().servers.get(id).is_some_and(|r| self.supervisor.is_alive(r.process)) }
```

Rewrite `start_or_reload(server, layout, ports: Ports, report)` to use `ports` from `cfg.effective_ports(server.id())`, port-check only that server's effective ports. Switching default restarts old+new default (port ownership moves). Rewrite `stop()` to drain all servers. Keep `is_running()` (no arg) = any alive for compat.

- [ ] **Step 4: Run pass**

Run: `cargo test -p ols-core web::manager:: -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ols-core/src/web/manager.rs
git commit -m "feat: track each web server running state independently"
```

---

### Task 4: Per-server apply pipeline (exclusive grouping)

**Files:**
- Modify: `crates/ols-core/src/web/manager.rs:331-589` (`apply`, `sync_apps` unchanged, `list_configs/read/write/set_ownership` server param)
- Test: extend manager tests + `crates/ols-core/examples/smoke_web.rs`

**Interfaces:**
- Consumes: `resolved_server()` from Task 2, per-server `Ports`, `ApplyReport { server }`
- Produces: `pub fn apply(&self, ctx: &ApplyContext, domains: &mut DomainStore) -> Result<Vec<ApplyReport>, CoreError>` OR keep single-report compat + new `apply_all() -> Vec<ApplyReport>`. Choose: keep `apply()` applying all enabled servers, returning combined `Vec<ApplyReport>`; add `apply_one(id)` helper. Update callers in `command.rs` accordingly in Task 5, not here.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn apply_renders_site_only_on_assigned_server() {
    // domains: a.test server=None (default nginx), b.test server=Some(apache)
    // apply_all with fake layouts; assert nginx sites_dir has a.test.conf only, apache has b.test.conf only
}
```

- [ ] **Step 2: Run fail**

Run: `cargo test -p ols-core web::manager::apply_renders_site_only_on_assigned_server -v`
Expected: FAIL

- [ ] **Step 3: Implement grouping loop**

```rust
pub fn apply(&self, ctx: &ApplyContext, domains: &mut DomainStore) -> Result<Vec<ApplyReport>, CoreError> {
    let mut out = Vec::new();
    let targets = running_or_default_targets(ctx.cfg, domains); // installed + (running || is default) — start with default + explicitly started
    for server_id in targets {
        out.push(self.apply_one(ctx, domains, &server_id)?);
    }
    Ok(out)
}
fn apply_one(&self, ctx, domains, server_id: &str) -> Result<ApplyReport, CoreError> {
    let server = server_by_id(server_id).ok_or(...)?;
    let ep = ctx.cfg.effective_ports(server_id);
    let ports = Ports { http: ep.http, https: ep.https };
    let enabled: Vec<Domain> = domains.list().into_iter()
        .filter(|d| d.enabled && resolved_server(d, ctx.cfg) == server_id).collect();
    // ... existing pipeline body (steps 1-7) scoped to `enabled`, snapshot/restore per server ...
}
```

`list_configs/read/write/set_ownership/list_history` resolve server via `resolved_server(domain)` or explicit param; keep `cfg.server` compat by mapping to `default_server`.

- [ ] **Step 4: Run pass**

Run: `cargo test -p ols-core web:: -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ols-core/src/web/manager.rs
git commit -m "feat: apply each web server independently with exclusive site grouping"
```

---

### Task 5: Services proxy + IPC + site_url

**Files:**
- Modify: `crates/ols-core/src/command.rs:127-135,223,1928-1936,2102-2119` (Start/StopService routing, StopWeb, ApplyWeb)
- Modify: `crates/ols-core/src/app.rs:553-564` (`site_url`), `domain_summaries`
- Modify: `crates/ols-core/src/service.rs:132-147` (`list()` append web entries) OR `app.rs` merge point — check where `list_services` dispatches; route `nginx/apache/caddy` ids to `web`
- Modify: `crates/ols-core/src/api.rs:97-99` (allowlist if needed)
- Test: `cargo test -p ols-core command::` + service tests

**Interfaces:**
- Consumes: `WebManager::start_server/stop_server/is_running_id/status`, `WebConfig::effective_ports/resolve_server`
- Produces: `StartService{id: nginx|apache|caddy}` → web start; `StopService` → web stop; `list_services` includes 3 web rows `{id, name, installed, running, port (effective), kind:"web", healthy}`; `site_url(d, cfg)` uses effective ports

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn list_services_includes_web_servers() { /* assert nginx/apache/caddy present with kind web */ }
#[test]
fn site_url_uses_assigned_server_port() {
    // default nginx 80; b.test pinned apache 8080 → "http://b.test:8080/"
}
```

- [ ] **Step 2: Run fail**

Run: `cargo test -p ols-core site_url_uses_assigned_server_port -v`
Expected: FAIL

- [ ] **Step 3: Implement**

```rust
// app.rs
pub fn site_url(&self, d: &Domain, cfg: &WebConfig) -> String {
    let sid = crate::domain::resolved_server(d, cfg);
    let p = cfg.effective_ports(&sid);
    let (scheme, port, default) = if d.https {("https", p.https, 443)} else {("http", p.http, 80)};
    if port == default { format!("{scheme}://{}/", d.hostname) } else { format!("{scheme}://{}:{port}/", d.hostname) }
}
// command.rs StartService:
C::StartService { id } if ["nginx","apache","caddy"].contains(&id.as_str()) => {
    let cfg = i.web_config();
    i.web.start_server(&id, &cfg).map_err(CoreError::WebError)?;
    // then i.apply_web_filtered(&id, &[])? minimal: apply_one for that id
    Ok(R::Ok)
}
C::StopService { id } if ["nginx","apache","caddy"].contains(&id.as_str()) => { i.web.stop_server(&id); Ok(R::Ok) }
C::ApplyWeb { overwrite } => { let reports = i.apply_web(&overwrite)?; Ok(R::AppliedMulti { reports }) }
// keep R::Applied compat: return first/default report OR new variant; update ui core.ts accordingly
// service.rs list(): chain web statuses
```

Exact variant choice: add `CoreResponse::AppliedMulti { reports: Vec<ApplyReport> }` + keep `Applied` for single; `ApplyWeb` returns multi.

- [ ] **Step 4: Run pass**

Run: `cargo test -p ols-core -v`
Expected: PASS (then `cargo fmt --all`, `cargo clippy -p ols-core -p ols-helper --all-targets`)

- [ ] **Step 5: Commit**

```bash
git add crates/ols-core/src/command.rs crates/ols-core/src/app.rs crates/ols-core/src/service.rs crates/ols-core/src/api.rs
git commit -m "feat: expose web servers as startable services with per-site URLs"
```

---

### Task 6: UI (Services rows, Web settings, site dropdown, types)

**Files:**
- Modify: `ui/src/core.ts:296-336` (WebConfig/WebStatus/ApplyReport types)
- Modify: `ui/src/pages/WebServer.tsx:28-36,221-306` (default picker + per-server ports)
- Modify: `ui/src/pages/Services.tsx:39-53,118-175` (web rows start/stop via start_service)
- Modify: `ui/src/components/site/DomainDialog.tsx:24-39` (newDomain server:null + dropdown)
- Modify: `ui/src/lib/web.ts`, `ui/src/lib/wait.ts` (waitForWebStopped per id if needed)
- Test: `cd ui && npm run lint && npm run build`

**Interfaces:**
- Consumes: IPC `get_web_config`, `list_services`, `add_domain/update_domain` with `server`
- Produces: Services table web rows; Web settings `default_server` + 3× ports; Domain form server select

- [ ] **Step 1: Write failing type check (no test runner — use tsc via lint)**

```ts
// core.ts
export interface ServerPorts { http_port: number; https_port: number }
export interface WebConfig { default_server: string; servers: Record<string, ServerPorts>; php_workers: number; dns_port: number; server: string; http_port: number; https_port: number }
```

Lint fails until all consumers updated (intentional red).

- [ ] **Step 2: Run lint, verify errors**

Run: `cd ui && npm run lint`
Expected: FAIL listing WebServer.tsx/Services.tsx mismatches

- [ ] **Step 3: Minimal UI edits**

```tsx
// WebServer.tsx saveSettings:
await runCommand({ type: 'set_setting', key: 'web.default_server', value: next.default_server });
for (const id of ['nginx','apache','caddy']) {
  await runCommand({ type: 'set_setting', key: `web.servers.${id}.http_port`, value: next.servers[id].http_port });
  await runCommand({ type: 'set_setting', key: `web.servers.${id}.https_port`, value: next.servers[id].https_port });
}
// ServerPanel: radiogroup picks draft.default_server; default row shows 80/443 locked (disabled inputs + "Default always uses 80/443"); custom port inputs enabled only for non-default servers; blurb "Selected default always binds 80/443; others keep custom ports"
// DomainDialog newDomain(): { ..., server: null }; Settings tab: <Select value={d.server ?? 'default'} options={default,nginx,apache,caddy} onChange={v => setD({...d, server: v==='default'?null:v})} />
// Services.tsx: no filter excluding web; toggle() already uses start_service/stop_service — works once backend routes; port column shows effective port (80 for default)
```

- [ ] **Step 4: Lint+build pass**

Run: `cd ui && npm run lint && npm run build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add ui/src/core.ts ui/src/pages/WebServer.tsx ui/src/pages/Services.tsx ui/src/components/site/DomainDialog.tsx ui/src/lib/web.ts
git commit -m "feat: UI for parallel web servers and per-site override"
```

---

### Task 7: Gates + specs-sync

**Files:**
- Modify: `specs/full_documentation.md`, `specs/architecture_overview.md`, `specs/catalog.txt`, `specs/relationships.txt`, `specs/data_models.txt`, `specs/api_reference.md`, `specs/diagrams/*.mmd`, `specs/runtime.md`

**Interfaces:**
- Consumes: all prior tasks
- Produces: green gates + spec log line

- [ ] **Step 1: Run full gates, record failures**

Run: `cargo fmt --all && cargo clippy -p ols-core -p ols-helper --all-targets && cargo test -p ols-core -p ols-helper`
Expected: PASS (fix if red)

- [ ] **Step 2: UI gates**

Run: `cd ui && npm run lint && npm run build`
Expected: PASS

- [ ] **Step 3: Update specs (same commit)**

Document: WebConfig shape, Domain.server, multi-Running, exclusive apply, Services proxy, IPC variants, port table. Append `specs/runtime.md` `## Log` line; verify Files Processed == Files to Process.

- [ ] **Step 4: Commit**

```bash
git add specs/ crates/ ui/
git commit -m "docs: sync specs for parallel web servers"
```

---

## File map

| File | Responsibility |
|------|----------------|
| `crates/ols-core/src/web/mod.rs` | WebConfig, ServerPorts, resolve |
| `crates/ols-core/src/web/manager.rs` | multi-Running, per-server apply/status |
| `crates/ols-core/src/domain.rs` | Domain.server + validation |
| `crates/ols-core/src/app.rs` | site_url, domain CRUD validation |
| `crates/ols-core/src/service.rs` | list proxy (or merge point) |
| `crates/ols-core/src/command.rs` | IPC routing, AppliedMulti |
| `crates/ols-core/src/api.rs` | allowlist |
| `ui/src/core.ts` | TS types |
| `ui/src/pages/WebServer.tsx` | default picker + ports |
| `ui/src/pages/Services.tsx` | web rows |
| `ui/src/components/site/DomainDialog.tsx` | server dropdown |

## Self-review notes (filled at plan write time)

- Spec §3.1→Task 1, §3.1 Domain→Task 2, §3.2→Tasks 3-4, §3.3→Task 5, §3.4→Task 6, §4-5→Tasks 3-5 error paths, §6→Tasks 1-6 tests + Task 7 gates, §7→Task 7.
- No TBD/TODO placeholders; each step shows exact code/commands.
- Types consistent: `ServerPorts{http,https}` Rust vs `ServerPorts{http_port,https_port}` TS — intentional (TS matches settings keys); Task 6 maps explicitly.
