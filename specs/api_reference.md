# API Reference — OpenLocalServer

Three front doors share one command model: Tauri IPC, CLI, HTTP API. All dispatch `CoreCommand` -> `CoreResponse`; errors shaped `{problem, cause, fix}`.

## Tauri IPC
Single command: `invoke('run_command', {command})` from `ui/src/core.ts` `runCommand(cmd)`.

`runCommand` rethrows: Tauri hands back the `Diagnostic` object as the rejection value. A rejected call therefore needs an owner for the error, and a caller that waits on one to render must not wait forever. `core.ts` exports `runCommandWithin(cmd, ms)` for that second case — the same call raced against a deadline that rejects with a Diagnostic naming the command and the seconds waited. It is the client-side half of a rule the core enforces on its side: no command is meant to block indefinitely, and a read that gates rendered content states its failure rather than holding a loading state. SiteDialog's `get_domain` and `get_project_detail` use a 20s deadline and show the Diagnostic with a Retry.

Core side of the same rule: `get_project_detail` clones the settings and custom-install stores and releases their locks before reading the project's files and probing runtimes, so a project on an unreachable mount blocks only itself. An unregistered project id is a stated failure (`"This site has no project any more."` + re-register fix), not a bare invalid-path error. A poisoned core lock is recovered rather than unwrapped, so one panic cannot make every later read panic too.

Events emitted: `process-event`, `terminal-event`, `runtime-event`, `projects-changed`, `ols:navigate`, `ols:status-icon` (payload `boolean` — true when nothing is running, so the UI shows the red mark). `ols:status-icon` is a push-only channel with no fetch-current counterpart, so it is also emitted on every `show_main_window` reveal; without that, a webview mounting after the last state change (window opened from the tray, reload) would keep the initial `useServerStopped` value of `false` and show the green mark while the tray is red.

Event sent by the UI: `ols:ui-ready` (no payload). The main window is created hidden (`visible: false` in `tauri.conf.json`) because the app window is transparent — showing it before the webview paints puts an empty see-through frame on screen for the length of the bundle load. `main.tsx` emits `ols:ui-ready` after two animation frames, once React has actually presented the first frame, and `reveal_window_on_ui_ready` shows the window on it. A 5s fallback timer reveals the window anyway so a UI error can never leave a running app with no window; `MAIN_WINDOW_REVEALED` makes the two paths idempotent. That timer is the one path that can beat the UI, so the page is not left blank while it waits: the pre-paint script in `index.html` marks `<html class="booting">` and `index.css` paints that element with `--background` (which propagates to the canvas) until `main.tsx` removes the class on the first presented frame, so a reveal that happens before React has painted still shows the app's background rather than a see-through frame. No command, payload or capability changes.

Whether that happens is decided once, at startup, by `ols_core::app::should_start_hidden(start_minimized, args)`. The stored `startup.minimized` preference is the whole answer and applies to every launch — the user opening the app themselves, the Windows startup entry, or a shortcut — so the toggle means what it says. `--minimized` is a force flag on top: it hides the window when the preference is off and changes nothing when it is on. When the launch starts hidden, neither reveal path is registered, no `ols:ui-ready` listener exists, and the window stays in the tray until the user opens it; a desktop notification after 2s says the app is running, so a hidden launch cannot be mistaken for a failed one. A hidden window also has no taskbar entry, so the window icon is only observable once the user opens it — which is why `show_main_window` doubles as the mark resync point.
Example:
```ts
import { runCommand } from './core';
const res = await runCommand({ type: 'list_services' });
```

Selected CoreCommands (type strings mirror Rust variants):
- Dashboard: `get_dashboard`, `get_system_stats`, `run_diagnostics`, `ignore_diagnostic`, `global_search`
- Projects: `register_project{path}`, `remove_project{id}`, `get_project_detail{id}`, `list_projects`, `scan_and_register_projects`, `plan_setup{project_id}`, `apply_setup{project_id,dry_run}`, `get_setup_progress`. `get_project_detail` clones the settings and custom-install stores and drops their locks before `build_detail` reads the project's files and probes runtime versions, so an unreachable project path cannot stall other commands; an id with no registered project returns a Diagnostic saying so and how to fix it. `scan_and_register_projects` reads the skip list before taking the project store and compares each candidate against it by both its stored canonical path and the raw path the scan read, so a folder the user deleted stays deleted however the scan reached it.
- Deleted items: `list_deleted_items`, `forget_deleted_item{value}`, `clear_deleted_items`. `list_deleted_items` returns `DeletedItem{value, deleted_at, kind}` for the two skip lists (`projects.removed` folders and `domains.auto_skip` automatic sites) merged and sorted newest-first; `deleted_at` is 0 for an entry stored before timestamps existed. `forget_deleted_item` removes that value from both lists and answers with the updated list, so the caller can replace its rows without a second read; a blank value is refused with a Diagnostic instead of quietly doing nothing. `clear_deleted_items` empties both lists and answers the same way. None of the three touch files on disk, and none of them re-register anything: the next scan or `sync_auto_domains` does. They are local-only commands and are deliberately absent from the HTTP `operate` allow-list.
- Domains/Web: `add_domain{domain}`, `update_domain`, `remove_domain`, `set_domain_enabled`, `duplicate_domain`, `get_domain`, `list_domains`, `apply_web{overwrite}` (returns one `ApplyReport` per server), `stop_web`, `get_web_status`, `get_web_config`, `list_web_configs`, `read_web_config`, `write_web_config`, `validate_web`, `set_ownership`, `list_config_history`, `restore_config_history`, `health_check{hostname}`, `sync_auto_domains`. `Domain` carries an optional `server` (nginx|apache|caddy); null means the default server. It also carries an optional `path_prefix`, which additionally serves the site at `localhost/<prefix>` on that server's HTTP port; `list_domains` reports it back per site with that server's `http_port` so the UI can build the URL. Config file, history and error-log commands resolve the server from the site, so each site's files are read under the server that renders it. `remove_domain` deletes the site *and* the list entry: when it was the last site of its project the project is removed too (same skip-list path as `remove_project`, files untouched), so the row does not reappear as "project only". `apply_web`/`apply_web_server` snapshot the domain store, run the render/validate/reload outside the store lock, and commit the generated-config hashes afterwards, so `list_domains` (and the UI poll that calls it) never waits on a multi-second apply; a change made during an apply is picked up by the next one.
- Certs: `list_certificates`, `get_ca_info`, `trust_ca`, `untrust_ca`, `regenerate_certificate`, `revoke_certificate`
- Runtimes: `list_runtime_catalog`, `refresh_runtime_catalog`, `install_runtime{id,version}`, `pause_runtime{id,version,paused}`, `cancel_runtime{id,version}`, `remove_runtime`, `list_custom_installs`, `set_custom_install`, `remove_custom_install`, `scan_php_folder`
- Services/DB: `list_services` (includes nginx/apache/caddy with kind `web`, the port each binds, a TCP `healthy` probe, and `log_source` — the log source holding that service's own output), `start_service{id}`, `stop_service{id}`, `restart_service{id}` (all three accept a web server id: start renders that server's sites, restart re-applies it, stop bounces only that server). All three are verified, not fire-and-forget: `start_service` waits for the spawn to be decided and returns a `Diagnostic` naming the OS reason when the process never started; `stop_service` waits for exit and errors when a kill was ignored; `restart_service` fails on either half instead of reporting a bounce that did not happen, `get_connection_info{id}`, `list_databases{engine}`, `create_database{engine,name,user?,password?}`, `list_db_users`, `create_db_user`, `list_db_backups`, `backup_database`, `restore_database`, `open_db_tool`, `list_sqlite`, `detect_sqlite`, `associate_sqlite`, `check_sqlite`
  `create_database` takes either no credentials or both a user and a password. With none, it connects as the engine's superuser (the bundled servers are provisioned passwordless on loopback). With both, it creates the database and the user, makes the user its owner and stores the password in the OS keyring, so the app never needs superuser rights. A half-supplied pair is refused with a message naming what is missing rather than creating a database nobody can log into.
- Logs: `list_log_sources` -> `LogSource{id,name,kind}`. Ids: `app`, `web:<server>:error` / `web:<server>:access` (all three servers, not just the configured default), `service:<id>` (a process-backed service's own stdout/stderr; empty while it is stopped, except after a Start that never spawned, where it returns the recorded reason), `process:<pid>`, `run:<id>`. The bare `web:error` / `web:access` ids still read the default server. `read_log{source,max_lines}`, `clear_log{source}`, `export_log{source,dest}` accept all of them.
- Workers/Scheduler: `list_workers`, `save_worker`, `start_worker`, `stop_worker`, `list_schedules`, `save_schedule`, `run_schedule_now`
- Terminal/Process: `open_terminal{cwd}`, `terminal_input{id,data}`, `terminal_resize`, `close_terminal`, `list_processes`, `start_process{spec}`, `stop_process{id}`, `get_process_output`, `check_port{port}`
- Git/Env: `git_status`, `git_commit`, `git_pull`, `git_push`, `git_diff`, `git_clone`, `list_ssh_keys`, `read_env_file`, `save_env_file`, `compare_env`
- QuickApps: `list_quick_apps`, `get_quick_app{id}`, `plan_quick_app{id,answers}`, `start_quick_app{id,answers}`, `get_quick_run{id}`, `cancel_quick_run`
- Plugins/Catalogs: `list_plugins`, `install_plugin`, `set_plugin_enabled`, `remove_plugin`, `list_catalog_sources`, `add_catalog_source{name,url,pubkey}`, `refresh_catalogs`
- Tunnels: `list_tunnels`, `start_tunnel{name}`, `stop_tunnel`, `check_tunnel`, `tunnel_log`, `list_tunnel_requests`, `replay_tunnel_request`
- Resources: `get_resource_limits` -> `{limits: ResourceLimits, cpu: CpuCapStatus}`, `set_resource_limits{limits}`. `validate` runs on the way in, so a percentage outside 1-100, more threads than the machine has logical cores, both of them set at once, or a `cpu_limiter_path` that is not a file is refused by name rather than stored. The returned `cpu` is re-read after a save, which is what lets the Resources card show a cap as unappliable the moment the utility is not where it was.
- AI/Updater/ misc: `ai_get_state`, `ai_preview`, `ai_start`, `ai_job`, `ai_cancel`, `get_updater_status`, `check_update`, `download_update`, `install_update`, `export_support_bundle`, `doctor`

## CLI (`ols`)
Global `--json` for raw pretty JSON. Auto-starts `ols daemon` when app closed (detached process, 30s pipe wait).
- `ols start|stop|restart|status`
- `ols setup [--path] [--dry-run] [-y]`
- `ols doctor` | `ols repair [project] [-y]`
- `ols project list|add <path>|remove <name>|start|stop|clone <url> <path>`
- `ols runtime list|install <id> [version]` (45min timeout, 2s poll)
- `ols php use <version>`
- `ols service list|start|stop|restart|logs -n 100` (`logs` resolves the service's `log_source` first)
- `ols domain list` | `ols certificate list`
- `ols tunnel list|start <name> [-y]|stop`
- `ols quick-app list` | `ols quick-command list|run <id> [--project]`
- `ols worker list|start|stop` | `ols snapshot list|create --label`
- `ols plugin list|install|enable|disable|remove`
- `ols catalog list|add|remove|refresh|install`
- `ols api status|enable [--port] [--operate]|disable|token`
- `ols update check|download|install`
- `ols test load [--site] [--profile smoke]`
- `ols ai status|on|off|test|ask|explain`
- `ols support-bundle <dest>` | `ols network` | `ols daemon [--stop]`

## HTTP API
Disabled by default. Enable via Settings -> Local API or `ols api enable`. Binds `127.0.0.1:7420` only. Auth `Authorization: Bearer ols_...` (only hash stored). Rejects `Origin` header and non-loopback Host. Body cap 1MB. Read-only mode allows `list_*/get_*/check_*`, diagnostics, git status/log, read_log. Operate mode adds start/stop/apply/install/register/snapshot/backup.
```bash
curl -H "Authorization: Bearer ols_..." http://127.0.0.1:7420/v1/ping
curl -H "Authorization: Bearer ols_..." -H "Content-Type: application/json" \
  -d '{"type":"list_services"}' http://127.0.0.1:7420/v1/command
```
Success `{"ok":true,"result":{...}}`. Fail `{"ok":false,"error":{problem,cause,fix}}` with 4xx.

## Errors
All layers return Diagnostic: `problem` (short), `cause` (root), `fix` (actionable, often runnable CoreCommand). UI renders ErrorCard with Fix/Details. CLI prints aligned problem/cause/fix. Never expose secrets, keys, tokens, or absolute home paths (masked + redacted).

## Examples
```ts
// Start service then wait
await runCommand({ type: 'start_service', id: 'mariadb' });
```
```bash
ols project add C:\\Sites\\shop
ols setup --path C:\\Sites\\shop --dry-run
ols service start mariadb
```
