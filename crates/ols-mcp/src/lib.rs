use ols_core::{control, CoreCommand, CoreResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use tokio::task::spawn_blocking;
use tracing::{info, warn};

mod auth;

/// MCP JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

/// MCP JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, error: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// MCP tool definition.
#[derive(Debug, Clone, Serialize)]
struct McpTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
}

/// Build the list of MCP tools exposed by OLS.
fn mcp_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "ols_get_sites_dir".into(),
            description: Some(
                "Get the configured base directory for websites and projects (configured via OLS_SITES_DIR or settings). Use this to know where website files and folders should be created."
                    .to_string(),
            ),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_list_domains".into(),
            description: Some("List all registered sites/domains.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_get_domain".into(),
            description: Some("Get complete configuration of one domain by hostname.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "hostname": { "type": "string" } },
                "required": ["hostname"]
            }),
        },
        McpTool {
            name: "ols_add_domain".into(),
            description: Some(
                "Create a new site/domain. 'root' is optional: if omitted, it defaults to <sites_dir>/<hostname> (using OLS_SITES_DIR). Relative paths like 'mysite' or 'mysite/public' are also automatically resolved inside OLS_SITES_DIR."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hostname": {
                        "type": "string",
                        "description": "The local domain hostname (e.g. 'myproject.test' or 'shop.local')."
                    },
                    "root": {
                        "type": "string",
                        "description": "Document root folder path. Optional: if omitted, defaults to <sites_dir>/<hostname>. Relative paths (e.g. 'myproject' or 'myproject/public') are automatically resolved inside OLS_SITES_DIR."
                    },
                    "php": {
                        "type": "string",
                        "description": "PHP version (e.g. '8.3'). Only used if kind is 'php'."
                    },
                    "kind": {
                        "type": "string",
                        "description": "Site kind: 'php', 'static', 'html', or 'proxy'. Defaults to 'php'."
                    },
                    "server": {
                        "type": "string",
                        "description": "Web server ('nginx', 'apache', or 'caddy'). Defaults to system default."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the site is enabled. Defaults to true."
                    }
                },
                "required": ["hostname"]
            }),
        },
        McpTool {
            name: "ols_update_domain".into(),
            description: Some(
                "Modify an existing site's config. 'root' is optional: if provided as a relative path, it is resolved against OLS_SITES_DIR."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hostname": {
                        "type": "string",
                        "description": "The hostname of the existing domain to update."
                    },
                    "root": {
                        "type": "string",
                        "description": "New document root folder path. If relative, resolved against OLS_SITES_DIR."
                    },
                    "php": {
                        "type": "string",
                        "description": "PHP version (e.g. '8.3'). Only used if kind is 'php'."
                    },
                    "kind": {
                        "type": "string",
                        "description": "Site kind: 'php', 'static', 'html', or 'proxy'."
                    },
                    "server": {
                        "type": "string",
                        "description": "Web server ('nginx', 'apache', or 'caddy')."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the site is enabled."
                    }
                },
                "required": ["hostname"]
            }),
        },
        McpTool {
            name: "ols_remove_domain".into(),
            description: Some("Delete a site/domain (destructive).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "hostname": { "type": "string" } },
                "required": ["hostname"]
            }),
        },
        McpTool {
            name: "ols_set_domain_enabled".into(),
            description: Some("Enable or disable a site.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hostname": { "type": "string" },
                    "enabled": { "type": "boolean" }
                },
                "required": ["hostname", "enabled"]
            }),
        },
        McpTool {
            name: "ols_list_services".into(),
            description: Some("List all services with running state.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_start_service".into(),
            description: Some("Start a service by id.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_stop_service".into(),
            description: Some("Stop a service by id.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_restart_service".into(),
            description: Some("Restart a service by id.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_list_tunnels".into(),
            description: Some("List all public tunnels.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_start_tunnel".into(),
            description: Some("Start a tunnel by id.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_stop_tunnel".into(),
            description: Some("Stop a tunnel by id.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_list_databases".into(),
            description: Some("List databases across engines.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "engine": { "type": "string" } }
            }),
        },
        McpTool {
            name: "ols_create_database".into(),
            description: Some("Create a new database.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "engine": { "type": "string" },
                    "name": { "type": "string" },
                    "user": { "type": "string" },
                    "password": { "type": "string" }
                },
                "required": ["engine", "name"]
            }),
        },
        McpTool {
            name: "ols_apply_web".into(),
            description: Some("Regenerate and reload the web server config.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_validate_web".into(),
            description: Some("Validate the web server config without applying.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_list_tasks".into(),
            description: Some("List background tasks and their statuses.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_run_command".into(),
            description: Some("Run any OLS core command by name with JSON args (advanced). `args` must include a `type` field matching the CoreCommand variant (e.g. `run_command` needs `executable`, `args`, `cwd`, `timeout_ms`).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "CoreCommand variant name (e.g. run_command, list_databases)" },
                    "args": { "type": "object", "description": "Command arguments as JSON. Must include a `type` field matching the CoreCommand variant." }
                },
                "required": ["command", "args"]
            }),
        },
    ]
}

/// Resolve the base directory for sites:
/// 1. `OLS_SITES_DIR` environment variable
/// 2. `domains.sites_dir` setting in settings.json
/// 3. OLS default sites directory (`AppPaths::sites_dir()`)
pub fn resolve_sites_dir() -> std::path::PathBuf {
    std::env::var("OLS_SITES_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            ols_core::settings::SettingsService::load(&ols_core::paths::AppPaths::resolve())
                .ok()
                .and_then(|s| s.get("domains.sites_dir").cloned())
                .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| ols_core::paths::AppPaths::resolve().sites_dir())
}

/// Resolve the document root for a domain:
/// - If `root` is omitted, empty, or whitespace, default to `<sites_dir>/<hostname>`.
/// - If `root` is a relative path (e.g. "mysite", "mysite/public", "./mysite"),
///   resolve it against `sites_dir`.
/// - If `root` is an absolute path, use it directly.
pub fn resolve_domain_root(root_opt: Option<&str>, hostname: &str) -> std::path::PathBuf {
    let sites_dir = resolve_sites_dir();
    match root_opt.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => sites_dir.join(hostname),
        Some(r) => {
            let p = std::path::Path::new(r);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                sites_dir.join(p)
            }
        }
    }
}

/// Convert named tool + simple args into a `CoreCommand`.
fn build_command(name: &str, args: &Value) -> Result<CoreCommand, String> {
    use CoreCommand as C;

    macro_rules! req_str {
        ($k:expr) => {
            args.get($k)
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("missing required argument: {}", $k))?
                .to_string()
        };
    }
    macro_rules! opt_str {
        ($k:expr) => {
            args.get($k).and_then(|v| v.as_str()).map(|s| s.to_string())
        };
    }
    macro_rules! opt_bool {
        ($k:expr) => {
            args.get($k).and_then(|v| v.as_bool())
        };
    }

    /// Convert a user-supplied `kind` string (e.g. `"php"`, `"static"`, `"proxy"`,
    /// case-insensitive) into the internally-tagged `SiteKind` JSON that serde expects,
    /// e.g. `{"type": "static"}`. If `kind` is already an object, pass it through.
    fn normalize_kind(value: &Value) -> Value {
        match value {
            Value::String(s) => match s.to_lowercase().as_str() {
                "static" | "html" => json!({"type": "static"}),
                "proxy" => {
                    json!({"type": "proxy", "upstream_port": 8080, "upstream_host": null, "upstream_https": false})
                }
                _ => json!({"type": "php", "version": null}),
            },
            other => other.clone(),
        }
    }

    Ok(match name {
        "ols_list_domains" => C::ListDomains,
        "ols_get_domain" => C::GetDomain {
            hostname: req_str!("hostname"),
        },
        "ols_add_domain" => {
            let mut args = args.clone();
            // Normalize `kind` from a plain string ("static", "html", "php", "proxy")
            // into the internally-tagged SiteKind enum serde expects. Case-insensitive.
            if let Some(k) = args.get("kind") {
                args["kind"] = normalize_kind(k);
            } else {
                args["kind"] = json!({"type": "php", "version": null});
            }
            let hostname = req_str!("hostname");
            // Default root to `<sites_dir>/<hostname>` or resolve relative path against `sites_dir`.
            let root_str = args.get("root").and_then(|v| v.as_str());
            let resolved_root = resolve_domain_root(root_str, &hostname);
            args["root"] = json!(resolved_root.to_string_lossy());

            let mut domain: ols_core::domain::Domain = serde_json::from_value(args)
                .map_err(|e| format!("failed to parse domain args: {e}"))?;
            domain.hostname = hostname;
            domain.root = resolved_root.to_string_lossy().to_string();
            if let Some(server) = opt_str!("server") {
                domain.server = Some(server);
            }
            if let Some(https) = opt_bool!("https") {
                domain.https = https;
            }
            if let Some(redirect_https) = opt_bool!("redirect_https") {
                domain.redirect_https = redirect_https;
            }
            if let Some(enabled) = opt_bool!("enabled") {
                domain.enabled = enabled;
            }
            C::AddDomain { domain }
        }
        "ols_update_domain" => {
            let mut args = args.clone();
            // Normalize `kind` from a plain string ("static", "php", "proxy") into the
            // internally-tagged SiteKind enum serde expects. Case-insensitive.
            if let Some(k) = args.get("kind") {
                args["kind"] = normalize_kind(k);
            } else {
                args["kind"] = json!({"type": "php", "version": null});
            }
            let hostname = req_str!("hostname");
            let root_str = args.get("root").and_then(|v| v.as_str());
            let resolved_root = resolve_domain_root(root_str, &hostname);
            args["root"] = json!(resolved_root.to_string_lossy());

            let mut domain: ols_core::domain::Domain = serde_json::from_value(args)
                .map_err(|e| format!("failed to parse domain args: {e}"))?;
            domain.hostname = hostname;
            domain.root = resolved_root.to_string_lossy().to_string();
            if let Some(server) = opt_str!("server") {
                domain.server = Some(server);
            }
            if let Some(enabled) = opt_bool!("enabled") {
                domain.enabled = enabled;
            }
            if let Some(https) = opt_bool!("https") {
                domain.https = https;
            }
            if let Some(redirect_https) = opt_bool!("redirect_https") {
                domain.redirect_https = redirect_https;
            }
            C::UpdateDomain { domain }
        }
        "ols_remove_domain" => C::RemoveDomain {
            hostname: req_str!("hostname"),
        },
        "ols_set_domain_enabled" => C::SetDomainEnabled {
            hostname: req_str!("hostname"),
            enabled: opt_bool!("enabled").ok_or("missing required argument: enabled")?,
        },
        "ols_list_services" => C::ListServices,
        "ols_start_service" => C::StartService { id: req_str!("id") },
        "ols_stop_service" => C::StopService { id: req_str!("id") },
        "ols_restart_service" => C::RestartService { id: req_str!("id") },
        "ols_list_tunnels" => C::ListTunnels,
        "ols_start_tunnel" => C::StartTunnel {
            id: req_str!("id"),
            confirm_exposure: false,
        },
        "ols_stop_tunnel" => C::StopTunnel { id: req_str!("id") },
        "ols_list_databases" => C::ListDatabases {
            engine: req_str!("engine"),
        },
        "ols_create_database" => C::CreateDatabase {
            engine: req_str!("engine"),
            name: req_str!("name"),
            user: opt_str!("user"),
            password: opt_str!("password"),
        },
        "ols_apply_web" => C::ApplyWeb { overwrite: vec![] },
        "ols_validate_web" => C::ValidateWeb,
        "ols_list_tasks" => C::ListTasks,
        "ols_run_command" => {
            let cmd_name = req_str!("command");
            let cmd_args = args.get("args").cloned().unwrap_or(json!({}));
            if cmd_args.get("type").is_none() {
                return Err(format!(
                    "failed to deserialize command '{cmd_name}': missing required field `type` \
                     in args (e.g. {{\"type\": \"run_command\", \"executable\": \"cmd\", \
                     \"args\": [], \"cwd\": null, \"timeout_ms\": 1000}})"
                ));
            }
            serde_json::from_value::<CoreCommand>(cmd_args)
                .map_err(|e| format!("failed to deserialize command '{cmd_name}': {e}"))?
        }
        _ => return Err(format!("unknown tool: {name}")),
    })
}

/// Send a `CoreCommand` to the running OLS app via the local control channel.
async fn send_command(cmd: CoreCommand) -> Result<CoreResponse, String> {
    let cmd_clone = cmd.clone();
    spawn_blocking(move || {
        let paths = ols_core::paths::AppPaths::resolve();
        control::send(&paths, cmd_clone)
            .map_err(|e| format!("{e}"))
            .and_then(|r| r.map_err(|d| format!("{}: {}", d.problem, d.cause)))
    })
    .await
    .map_err(|e| format!("command task failed: {e}"))?
}

/// Format a `CoreResponse` into a human-readable string for the MCP client.
fn format_response(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Domains { domains } => {
            if domains.is_empty() {
                "No domains registered.".into()
            } else {
                let mut out = String::from("Registered domains:\n");
                for d in domains {
                    out.push_str(&format!(
                        "- {} ({}, {}, server: {}, ssl: {}, url: {})\n",
                        d.hostname,
                        d.group,
                        if d.enabled { "enabled" } else { "disabled" },
                        d.server,
                        if d.https { "yes" } else { "no" },
                        d.url
                    ));
                }
                out
            }
        }
        CoreResponse::Services { services } => {
            if services.is_empty() {
                "No services found.".into()
            } else {
                let mut out = String::from("Services:\n");
                for s in services {
                    out.push_str(&format!(
                        "- {}: {}\n",
                        s.id,
                        if s.running { "running" } else { "stopped" }
                    ));
                }
                out
            }
        }
        CoreResponse::Tunnels { tunnels } => {
            if tunnels.is_empty() {
                "No tunnels configured.".into()
            } else {
                let mut out = String::from("Tunnels:\n");
                for t in tunnels {
                    out.push_str(&format!(
                        "- {}: {} -> {} ({})\n",
                        t.config.id, t.config.name, t.config.target, t.state
                    ));
                }
                out
            }
        }
        CoreResponse::Tasks { tasks } => {
            if tasks.is_empty() {
                "No tasks found.".into()
            } else {
                let mut out = String::from("Tasks:\n");
                for t in tasks {
                    out.push_str(&format!("- {}: {} ({:?})\n", t.id, t.kind, t.state));
                }
                out
            }
        }
        CoreResponse::Ok => "Done.".into(),
        CoreResponse::Text { text } => text.clone(),
        CoreResponse::Domain { domain } => {
            let d = domain.as_ref();
            format!(
                "Domain {}:\n- Document root: {}\n- Server: {}\n- Enabled: {}\n- HTTPS: {}\n- URL: {}",
                d.hostname,
                d.root,
                d.server.as_deref().unwrap_or("-"),
                d.enabled,
                d.https,
                d.url()
            )
        }
        CoreResponse::Names { names } => {
            if names.is_empty() {
                "None.".into()
            } else {
                names.join("\n")
            }
        }
        CoreResponse::CommandResult { entry } => {
            format!(
                "Command: {}\nArgs: {}\nExit code: {}\nDuration: {}ms\nTimed out: {}",
                entry.command,
                entry.args.join(" "),
                entry
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".into()),
                entry.duration_ms,
                entry.timed_out
            )
        }
        CoreResponse::CommandHistory { entries } => {
            if entries.is_empty() {
                "No command history.".into()
            } else {
                let mut out = String::from("Command history:\n");
                for e in entries {
                    out.push_str(&format!(
                        "- {} {} (exit: {}, {}ms, timed_out: {})\n",
                        e.command,
                        e.args.join(" "),
                        e.exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "-".into()),
                        e.duration_ms,
                        e.timed_out
                    ));
                }
                out
            }
        }
        other => format!("{:?}", other),
    }
}

/// Handle a single JSON-RPC request.
async fn handle_request(req: JsonRpcRequest, override_token: Option<&str>) -> JsonRpcResponse {
    // Auth gate: reject unauthenticated requests before any tool runs.
    if let auth::AuthOutcome::Err(err) = auth::AuthMiddleware::check(&req, override_token) {
        return JsonRpcResponse::err(req.id, err);
    }

    match req.method.as_str() {
        "initialize" => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": {},
                    "prompts": {}
                },
                "serverInfo": {
                    "name": "ols-mcp",
                    "version": "2.3.0"
                }
            });
            JsonRpcResponse::ok(req.id, result)
        }
        "initialized" => JsonRpcResponse::ok(req.id, json!(null)),
        "tools/list" => {
            let tools: Vec<Value> = mcp_tools()
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema
                    })
                })
                .collect();
            JsonRpcResponse::ok(req.id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            if name == "ols_get_sites_dir" {
                let sites_dir = resolve_sites_dir();
                return JsonRpcResponse::ok(
                    req.id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Sites root directory: {}\n\nWhen adding a domain with `ols_add_domain` without specifying `root`, it defaults to `<sites_dir>\\<hostname>`. Any relative path is also resolved inside this directory.",
                                sites_dir.display()
                            )
                        }]
                    }),
                );
            }
            match build_command(name, &args) {
                Ok(cmd) => match send_command(cmd).await {
                    Ok(response) => {
                        let text = format_response(&response);
                        JsonRpcResponse::ok(
                            req.id,
                            json!({ "content": [{ "type": "text", "text": text }] }),
                        )
                    }
                    Err(e) => JsonRpcResponse::err(req.id, json!({ "code": -32603, "message": e })),
                },
                Err(e) => JsonRpcResponse::err(req.id, json!({ "code": -32603, "message": e })),
            }
        }
        "resources/list" => {
            let sites_dir = resolve_sites_dir();
            JsonRpcResponse::ok(
                req.id,
                json!({
                    "resources": [
                        {
                            "uri": "ols://sites-dir",
                            "name": "OLS Sites Directory",
                            "description": format!("Configured base directory for websites: {}", sites_dir.display()),
                            "mimeType": "text/plain"
                        }
                    ]
                }),
            )
        }
        "resources/read" => {
            let uri = req.params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            if uri == "ols://sites-dir" {
                let sites_dir = resolve_sites_dir();
                JsonRpcResponse::ok(
                    req.id,
                    json!({
                        "contents": [
                            {
                                "uri": uri,
                                "mimeType": "text/plain",
                                "text": sites_dir.to_string_lossy()
                            }
                        ]
                    }),
                )
            } else {
                JsonRpcResponse::err(
                    req.id,
                    json!({ "code": -32602, "message": format!("unknown resource URI: {uri}") }),
                )
            }
        }
        "prompts/list" => JsonRpcResponse::ok(req.id, json!({ "prompts": [] })),
        "prompts/get" => JsonRpcResponse::err(
            req.id,
            json!({ "code": -32601, "message": "prompts not supported" }),
        ),
        "ping" => JsonRpcResponse::ok(req.id, json!(null)),
        other => JsonRpcResponse::err(
            req.id,
            json!({ "code": -32601, "message": format!("method not found: {other}") }),
        ),
    }
}

/// Read a line from stdin.
async fn read_line(stdin: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    match stdin.read_line(&mut line)? {
        0 => Ok(None),
        _ => {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
    }
}

/// Run the MCP server over stdio. Connects to the running OLS app via its local
/// control channel (named pipe + control.json token), so OLS must be running.
pub async fn run_stdio(override_token: Option<String>) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    info!("ols-mcp server starting (connects to running OLS app)");

    loop {
        let line = match read_line(&mut stdin).await? {
            Some(line) => line,
            None => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                warn!("failed to parse request: {e}");
                let _ = writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&JsonRpcResponse::err(
                        json!(null),
                        json!({ "code": -32700, "message": format!("parse error: {e}") }),
                    ))
                    .unwrap()
                );
                stdout.flush().ok();
                continue;
            }
        };

        let resp = handle_request(req, override_token.as_deref()).await;
        let out = serde_json::to_string(&resp).unwrap();
        let _ = writeln!(stdout, "{out}");
        stdout.flush().ok();
    }

    info!("ols-mcp server stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_domain_root_defaults_to_hostname_subfolder() {
        let root = resolve_domain_root(None, "myshop.test");
        assert!(root.is_absolute());
        assert!(root.ends_with("myshop.test"));
    }

    #[test]
    fn resolve_domain_root_handles_empty_root() {
        let root = resolve_domain_root(Some("   "), "myshop.test");
        assert!(root.is_absolute());
        assert!(root.ends_with("myshop.test"));
    }

    #[test]
    fn resolve_domain_root_resolves_relative_path() {
        let root = resolve_domain_root(Some("custom-folder/public"), "myshop.test");
        assert!(root.is_absolute());
        let s = root.to_string_lossy();
        assert!(s.contains("custom-folder"));
        assert!(s.ends_with("public"));
    }

    #[test]
    fn build_command_add_domain_without_root() {
        let cmd = build_command(
            "ols_add_domain",
            &json!({
                "hostname": "test.local"
            }),
        )
        .expect("should build AddDomain without root");

        if let CoreCommand::AddDomain { domain } = cmd {
            assert_eq!(domain.hostname, "test.local");
            assert!(domain.root.ends_with("test.local"));
            assert!(std::path::Path::new(&domain.root).is_absolute());
        } else {
            panic!("expected AddDomain variant");
        }
    }

    #[test]
    fn build_command_add_domain_with_relative_root() {
        let cmd = build_command(
            "ols_add_domain",
            &json!({
                "hostname": "test.local",
                "root": "test-folder"
            }),
        )
        .expect("should build AddDomain with relative root");

        if let CoreCommand::AddDomain { domain } = cmd {
            assert_eq!(domain.hostname, "test.local");
            assert!(domain.root.ends_with("test-folder"));
            assert!(std::path::Path::new(&domain.root).is_absolute());
        } else {
            panic!("expected AddDomain variant");
        }
    }
}
