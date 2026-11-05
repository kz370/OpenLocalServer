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
                "Create a new site/domain. Supports PHP, Static, Proxy, Node.js, and Python sites. HTTPS is enabled by default with trusted local CA certificates. 'root' is optional: if omitted, defaults to <sites_dir>/<hostname>. Relative paths are resolved against OLS_SITES_DIR."
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
                        "description": "Document root folder path. Optional: if omitted, defaults to <sites_dir>/<hostname>. Relative paths are automatically resolved inside OLS_SITES_DIR."
                    },
                    "kind": {
                        "type": "string",
                        "description": "Site kind: 'php', 'static', 'html', 'proxy', 'node', or 'python'. Defaults to 'php'."
                    },
                    "php": {
                        "type": "string",
                        "description": "PHP version (e.g. '8.3'). Only used if kind is 'php'."
                    },
                    "command": {
                        "type": "string",
                        "description": "Start command for supervised background Node or Python processes (e.g. 'npm run dev', 'python main.py', 'uvicorn main:app --reload')."
                    },
                    "port": {
                        "type": "integer",
                        "description": "Upstream reverse proxy port (e.g. 3000 for Node, 8000 for Python). Defaults automatically."
                    },
                    "runtime": {
                        "type": "string",
                        "description": "Runtime version pin (e.g. 'node', 'node:20.18.0', 'python', 'python:3.12.9')."
                    },
                    "venv": {
                        "type": "string",
                        "description": "Python virtual environment directory or executable path (e.g. '.venv', 'venv')."
                    },
                    "server": {
                        "type": "string",
                        "description": "Web server ('nginx', 'apache', or 'caddy'). Defaults to system default."
                    },
                    "https": {
                        "type": "boolean",
                        "description": "Whether HTTPS is enabled using OLS local CA certificates. Defaults to true."
                    },
                    "redirect_https": {
                        "type": "boolean",
                        "description": "Whether HTTP requests automatically redirect to HTTPS. Defaults to true when https is true."
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
                "Modify an existing site's config. Supports switching kinds ('php', 'node', 'python', 'static', 'proxy'), toggling HTTPS/SSL with local CA certificates, and adjusting start commands, ports, and runtimes."
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
                    "kind": {
                        "type": "string",
                        "description": "Site kind: 'php', 'static', 'html', 'proxy', 'node', or 'python'."
                    },
                    "php": {
                        "type": "string",
                        "description": "PHP version (e.g. '8.3'). Only used if kind is 'php'."
                    },
                    "command": {
                        "type": "string",
                        "description": "Start command for supervised background Node or Python processes."
                    },
                    "port": {
                        "type": "integer",
                        "description": "Upstream reverse proxy port."
                    },
                    "runtime": {
                        "type": "string",
                        "description": "Runtime version pin (e.g. 'node', 'node:20.18.0', 'python', 'python:3.12.9')."
                    },
                    "venv": {
                        "type": "string",
                        "description": "Python virtual environment directory or executable path."
                    },
                    "server": {
                        "type": "string",
                        "description": "Web server ('nginx', 'apache', or 'caddy')."
                    },
                    "https": {
                        "type": "boolean",
                        "description": "Enable or disable HTTPS with OLS local CA certificate."
                    },
                    "redirect_https": {
                        "type": "boolean",
                        "description": "Whether HTTP redirects to HTTPS."
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
            description: Some(
                "List databases across engines (MariaDB/MySQL, PostgreSQL, SQLite). 'engine' is optional: if omitted, databases across all engines are listed."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "engine": {
                        "type": "string",
                        "description": "Optional database engine ('mariadb', 'mysql', 'postgres', or 'sqlite'). If omitted, databases from all engines are listed."
                    }
                }
            }),
        },
        McpTool {
            name: "ols_create_database".into(),
            description: Some(
                "Create a new database. Supports MariaDB, MySQL, PostgreSQL, and SQLite. For SQLite, 'name' can be a database file name or path."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "engine": {
                        "type": "string",
                        "description": "Database engine: 'mariadb', 'mysql', 'postgres', or 'sqlite'."
                    },
                    "name": {
                        "type": "string",
                        "description": "Database name (or SQLite file path/name)."
                    },
                    "user": {
                        "type": "string",
                        "description": "Optional username for MariaDB/PostgreSQL."
                    },
                    "password": {
                        "type": "string",
                        "description": "Optional user password for MariaDB/PostgreSQL."
                    }
                },
                "required": ["engine", "name"]
            }),
        },
        McpTool {
            name: "ols_list_tables".into(),
            description: Some(
                "List all tables in a database. Supports MariaDB/MySQL, PostgreSQL, and SQLite."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "engine": {
                        "type": "string",
                        "description": "Database engine ('mariadb', 'postgres', or 'sqlite')."
                    },
                    "database": {
                        "type": "string",
                        "description": "Database name (for MariaDB/PostgreSQL) or database file path (for SQLite)."
                    }
                },
                "required": ["engine", "database"]
            }),
        },
        McpTool {
            name: "ols_execute_sql".into(),
            description: Some(
                "Execute a SQL query or command on a database to view table rows, inspect schemas, or modify data and tables (SELECT, INSERT, UPDATE, DELETE, CREATE TABLE, ALTER TABLE, etc.). Supports MariaDB/MySQL, PostgreSQL, and SQLite."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "engine": {
                        "type": "string",
                        "description": "Database engine ('mariadb', 'postgres', or 'sqlite')."
                    },
                    "database": {
                        "type": "string",
                        "description": "Database name (for MariaDB/PostgreSQL) or database file path (for SQLite). Optional for server-level queries."
                    },
                    "sql": {
                        "type": "string",
                        "description": "SQL statement or query to execute."
                    }
                },
                "required": ["engine", "sql"]
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
            name: "ols_unignore_site".into(),
            description: Some("Bring back a project's hidden site: removes the project's `.ols_ignore_site` marker so its site shows again. The project id comes from ols_list_projects.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project id from ols_list_projects." }
                },
                "required": ["project_id"]
            }),
        },
        McpTool {
            name: "ols_docker_status".into(),
            description: Some("Docker status: whether Docker is installed and running, engine version, container OS type (linux/windows), and the endpoint OLS uses. Never fails; a stopped engine is reported as running=false with a plain-language message.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_docker_start".into(),
            description: Some("Start Docker Desktop and wait (up to 2 minutes) until its engine answers. Runs as a background task; returns the task id (follow it with ols_list_tasks).".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_docker_list_containers".into(),
            description: Some("List Docker containers with state, image, published ports (container port -> host port), Compose project/service, and the OLS site they serve. Running containers only unless 'all' is true.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "all": { "type": "boolean", "description": "Include stopped containers. Default false." }
                }
            }),
        },
        McpTool {
            name: "ols_docker_container_action".into(),
            description: Some("Start, stop, restart, pause or unpause one Docker container by name or id. Does not remove containers or volumes.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Container name or id (from ols_docker_list_containers)." },
                    "action": { "type": "string", "enum": ["start", "stop", "restart", "pause", "unpause"] }
                },
                "required": ["id", "action"]
            }),
        },
        McpTool {
            name: "ols_docker_container_logs".into(),
            description: Some("Last log lines of a Docker container (default 200, max 2000). Passwords and tokens are redacted.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Container name or id." },
                    "tail": { "type": "integer", "description": "How many lines (1-2000). Default 200." }
                },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_docker_container_details".into(),
            description: Some("A container's settings (docker inspect): id, image, command, ports, mounts, networks, labels, restart policy, memory/CPU limits, health. Password-like environment values are removed.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Container name or id." } },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_docker_container_update".into(),
            description: Some("Change a container's restart policy (no, always, unless-stopped, on-failure + max_retries) and memory (MB) / CPU (cores) limits without recreating it. 0 removes a limit. Omitted fields are unchanged.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "restart_policy": { "type": "string", "enum": ["no", "always", "unless-stopped", "on-failure"] },
                    "max_retries": { "type": "integer" },
                    "memory_mb": { "type": "integer" },
                    "cpus": { "type": "number" }
                },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_docker_compose_list".into(),
            description: Some("List Docker Compose stacks: compose files in registered projects plus stacks Docker is running, with each service's image, ports and state.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_docker_compose_check".into(),
            description: Some("Check a compose file on disk: YAML syntax, services, duplicate host ports, unpinned images, passwords written in the file, then Docker's own `docker compose config`.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Full path to compose.yaml / docker-compose.yml (from ols_docker_compose_list)." } },
                "required": ["path"]
            }),
        },
        McpTool {
            name: "ols_docker_compose_action".into(),
            description: Some("Run `docker compose up|down|stop|start|restart|pull|build` for a stack (optionally one service) as a background task; returns the task id (follow it with ols_list_tasks). `down` keeps volumes. OLS passes the stack's stored passwords.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Full path to the compose file." },
                    "action": { "type": "string", "enum": ["up", "down", "stop", "start", "restart", "pull", "build"] },
                    "service": { "type": "string", "description": "Optional: only this service." }
                },
                "required": ["path", "action"]
            }),
        },
        McpTool {
            name: "ols_docker_compose_connect".into(),
            description: Some("Connect a Compose stack to OLS (or show its connection): when on, OLS adds a generated override on every run so containers reach OLS sites by name via host.docker.internal and trust OLS's local HTTPS certificate (public cert only, read-only). The compose file is not changed. Omit 'enabled' to only read the state, OLS service addresses and the override.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Full path to the compose file." },
                    "enabled": { "type": "boolean", "description": "Turn the connection on or off. Omit to just read." }
                },
                "required": ["path"]
            }),
        },
        McpTool {
            name: "ols_docker_compose_presets".into(),
            description: Some("Services the compose builder offers (MariaDB, MySQL, PostgreSQL, MongoDB, Redis, Memcached, RabbitMQ, Meilisearch, Mailpit, Adminer, phpMyAdmin, PHP+Apache, Node app, Nginx static) with their versions and ports.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_docker_compose_create".into(),
            description: Some("Create a compose.yaml in a folder from builder services. OLS picks free ports (127.0.0.1 only), generates passwords into the OS credential store (the file only has ${OLS_SECRET_*} variables), and never replaces an existing compose file from here. Set dry_run to only preview the file.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dir": { "type": "string", "description": "Full path of the folder (usually a project folder)." },
                    "name": { "type": "string", "description": "Optional stack name." },
                    "services": {
                        "type": "array",
                        "description": "Services: {preset, name, tag?, host_ports?, image? (custom), container_port? (custom), env?}. preset is an id from ols_docker_compose_presets or 'custom'.",
                        "items": { "type": "object" }
                    },
                    "dry_run": { "type": "boolean", "description": "Only return the planned file. Default false." }
                },
                "required": ["dir", "services"]
            }),
        },
        McpTool {
            name: "ols_docker_create_site".into(),
            description: Some("Give a running container a local site (e.g. shop.test) with HTTPS: a reverse proxy to the container's published port that follows the container if its port changes. Re-applies the web server config afterwards.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "container": { "type": "string", "description": "Container name or id." },
                    "container_port": { "type": "integer", "description": "The container's own port (it must be published)." },
                    "hostname": { "type": "string", "description": "Site domain, e.g. shop.test." },
                    "https": { "type": "boolean", "description": "Default true." }
                },
                "required": ["container", "container_port", "hostname"]
            }),
        },
        McpTool {
            name: "ols_restart_site_app".into(),
            description: Some("Restart the supervised background application process (Node.js dev server, Python app/uvicorn, etc.) for a site.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hostname": { "type": "string", "description": "The site hostname (e.g. 'mysite.test')" }
                },
                "required": ["hostname"]
            }),
        },
        McpTool {
            name: "ols_inspect_site_folder".into(),
            description: Some("Inspect a project folder to detect framework (Django, Flask, FastAPI, Node, PHP), virtual environments, package managers (npm, pip, uv, poetry), scripts, dependencies, and suggested start command and port.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Folder path of the project to inspect. If relative or omitted, resolved against OLS_SITES_DIR."
                    }
                }
            }),
        },
        McpTool {
            name: "ols_list_runtimes".into(),
            description: Some("List installed and available runtimes (Python, Node.js, PHP, MariaDB, etc.) with system PATH versions, OLS installed versions, and active defaults.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_install_runtime".into(),
            description: Some("Download and install an official managed runtime version (e.g. Python '3.12.9', Node.js '20.18.0') with verified SHA-256.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Runtime ID: 'python', 'node', 'php', 'mariadb', etc." },
                    "version": { "type": "string", "description": "Version string to install (e.g. '3.12.9')" }
                },
                "required": ["id", "version"]
            }),
        },
        McpTool {
            name: "ols_refresh_runtimes".into(),
            description: Some("Refresh online catalog versions for a runtime family from official vendor feeds (e.g. 'python', 'node', 'php').".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Runtime ID to refresh ('python', 'node', 'php', 'mariadb', etc.)" }
                },
                "required": ["id"]
            }),
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
        McpTool {
            name: "ols_list_projects".into(),
            description: Some("List all registered standalone and site-linked projects including name, path, project type (preset or custom, e.g. Tauri), health status, and standalone flag.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_add_project".into(),
            description: Some("Register a folder as a project in OpenLocalServer. Validates path uniqueness and auto-detects its project type when omitted or set to 'auto'. Set auto_site=true to also create a local site; this is false by default.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Folder path of the project. If relative, resolved against OLS_SITES_DIR."
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional display name for the project (defaults to folder name)."
                    },
                    "project_type": {
                        "type": "string",
                        "description": "Optional project type or framework preset (e.g. 'laravel', 'react', 'nextjs', 'custom:Tauri', 'custom:Rust') or 'auto'."
                    },
                    "auto_site": {
                        "type": "boolean",
                        "description": "Optional. Also create a local site when this project can be served directly. Defaults to false."
                    }
                },
                "required": ["path"]
            }),
        },
        McpTool {
            name: "ols_update_project".into(),
            description: Some("Update settings for an existing project by project ID.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The project ID." },
                    "name": { "type": "string", "description": "Optional updated display name." },
                    "project_type": { "type": "string", "description": "Optional updated project type / framework preset." },
                    "icon": { "type": "string", "description": "Optional updated Font Awesome icon name." }
                },
                "required": ["id"]
            }),
        },
        McpTool {
            name: "ols_inspect_project_folder".into(),
            description: Some("Inspect a directory to detect framework presets (Laravel, Symfony, WordPress, PHP, Node, Next.js, React, Vue, Go, Rust, .NET, Java, Static HTML, Tauri), check whether it is already registered or duplicate, and suggest project name.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Folder path to inspect. If relative, resolved against OLS_SITES_DIR."
                    }
                },
                "required": ["path"]
            }),
        },
        McpTool {
            name: "ols_list_project_types".into(),
            description: Some("List available project types: bundled framework presets and custom project types (e.g. Tauri).".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        McpTool {
            name: "ols_add_custom_project_type".into(),
            description: Some("Add a new custom project type to OpenLocalServer.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the custom project type (e.g. 'Tauri', 'Rust', 'SvelteKit')." }
                },
                "required": ["name"]
            }),
        },
        McpTool {
            name: "ols_rename_custom_project_type".into(),
            description: Some("Rename an existing custom project type, automatically updating all projects using it.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "old_name": { "type": "string", "description": "Current name of the custom project type." },
                    "new_name": { "type": "string", "description": "New name for the custom project type." }
                },
                "required": ["old_name", "new_name"]
            }),
        },
        McpTool {
            name: "ols_remove_custom_project_type".into(),
            description: Some("Remove a custom project type, safely resetting projects using it to Auto.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the custom project type to remove." }
                },
                "required": ["name"]
            }),
        },
        McpTool {
            name: "ols_list_editors".into(),
            description: Some("List auto-detected and configured development code editors/IDEs (VS Code, Cursor, Windsurf, Trae, Zed, JetBrains IDEs, etc.) and their visibility status.".to_string()),
            input_schema: json!({ "type": "object", "properties": {} }),
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

pub fn split_command_line(cmd: &str) -> (String, Vec<String>) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        (String::new(), Vec::new())
    } else {
        (
            parts[0].to_string(),
            parts[1..].iter().map(|s| s.to_string()).collect(),
        )
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

    Ok(match name {
        "ols_list_domains" => C::ListDomains,
        "ols_get_domain" => C::GetDomain {
            hostname: req_str!("hostname"),
        },
        "ols_add_domain" => {
            let mut args = args.clone();
            let hostname = req_str!("hostname");
            let root_str = args.get("root").and_then(|v| v.as_str());
            let resolved_root = resolve_domain_root(root_str, &hostname);
            args["root"] = json!(resolved_root.to_string_lossy());

            let kind_str = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());
            let port_opt = args.get("port").and_then(|v| v.as_u64()).map(|p| p as u16);
            let command_opt = opt_str!("command");
            let runtime_opt = opt_str!("runtime");
            let venv_opt = opt_str!("venv");

            match kind_str.as_deref() {
                Some("node") => {
                    let port = port_opt.unwrap_or(3000);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    let cmd_str = command_opt.as_deref().unwrap_or("npm run dev");
                    let (exe, cmd_args) = split_command_line(cmd_str);
                    args["app"] = json!({
                        "executable": exe,
                        "args": cmd_args,
                        "cwd": resolved_root.to_string_lossy(),
                        "runtime": runtime_opt.unwrap_or_else(|| "node".into())
                    });
                }
                Some("python") => {
                    let port = port_opt.unwrap_or(8000);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    let cmd_str = command_opt.as_deref().unwrap_or("python main.py");
                    let (mut exe, cmd_args) = split_command_line(cmd_str);
                    if let Some(ref venv) = venv_opt {
                        let vpath = std::path::Path::new(venv);
                        if vpath.ends_with("python.exe") || vpath.ends_with("python") {
                            exe = venv.clone();
                        } else {
                            let cand = resolved_root.join(vpath).join("Scripts").join("python.exe");
                            exe = cand.to_string_lossy().to_string();
                        }
                    }
                    args["app"] = json!({
                        "executable": exe,
                        "args": cmd_args,
                        "cwd": resolved_root.to_string_lossy(),
                        "runtime": runtime_opt.unwrap_or_else(|| "python".into())
                    });
                }
                Some("static") | Some("html") => {
                    args["kind"] = json!({"type": "static"});
                }
                Some("proxy") => {
                    let port = port_opt.unwrap_or(8080);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    if let Some(cmd_str) = command_opt {
                        let (exe, cmd_args) = split_command_line(&cmd_str);
                        args["app"] = json!({
                            "executable": exe,
                            "args": cmd_args,
                            "cwd": resolved_root.to_string_lossy(),
                            "runtime": runtime_opt
                        });
                    }
                }
                _ => {
                    if let Some(cmd_str) = command_opt {
                        let port = port_opt.unwrap_or(3000);
                        args["kind"] = json!({
                            "type": "proxy",
                            "upstream_port": port,
                            "upstream_host": null,
                            "upstream_https": false
                        });
                        let (exe, cmd_args) = split_command_line(&cmd_str);
                        args["app"] = json!({
                            "executable": exe,
                            "args": cmd_args,
                            "cwd": resolved_root.to_string_lossy(),
                            "runtime": runtime_opt
                        });
                    } else {
                        let php_ver = opt_str!("php");
                        args["kind"] = json!({"type": "php", "version": php_ver});
                    }
                }
            }

            let mut domain: ols_core::domain::Domain = serde_json::from_value(args)
                .map_err(|e| format!("failed to parse domain args: {e}"))?;
            domain.hostname = hostname;
            domain.root = resolved_root.to_string_lossy().to_string();
            if let Some(server) = opt_str!("server") {
                domain.server = Some(server);
            }
            let https = opt_bool!("https").unwrap_or(true);
            domain.https = https;
            domain.redirect_https = opt_bool!("redirect_https").unwrap_or(https);
            if let Some(enabled) = opt_bool!("enabled") {
                domain.enabled = enabled;
            }
            C::AddDomain { domain }
        }
        "ols_update_domain" => {
            let mut args = args.clone();
            let hostname = req_str!("hostname");
            let root_str = args.get("root").and_then(|v| v.as_str());
            let resolved_root = resolve_domain_root(root_str, &hostname);
            args["root"] = json!(resolved_root.to_string_lossy());

            let kind_str = args
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());
            let port_opt = args.get("port").and_then(|v| v.as_u64()).map(|p| p as u16);
            let command_opt = opt_str!("command");
            let runtime_opt = opt_str!("runtime");
            let venv_opt = opt_str!("venv");

            match kind_str.as_deref() {
                Some("node") => {
                    let port = port_opt.unwrap_or(3000);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    let cmd_str = command_opt.as_deref().unwrap_or("npm run dev");
                    let (exe, cmd_args) = split_command_line(cmd_str);
                    args["app"] = json!({
                        "executable": exe,
                        "args": cmd_args,
                        "cwd": resolved_root.to_string_lossy(),
                        "runtime": runtime_opt.unwrap_or_else(|| "node".into())
                    });
                }
                Some("python") => {
                    let port = port_opt.unwrap_or(8000);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    let cmd_str = command_opt.as_deref().unwrap_or("python main.py");
                    let (mut exe, cmd_args) = split_command_line(cmd_str);
                    if let Some(ref venv) = venv_opt {
                        let vpath = std::path::Path::new(venv);
                        if vpath.ends_with("python.exe") || vpath.ends_with("python") {
                            exe = venv.clone();
                        } else {
                            let cand = resolved_root.join(vpath).join("Scripts").join("python.exe");
                            if cand.exists() {
                                exe = cand.to_string_lossy().to_string();
                            }
                        }
                    }
                    args["app"] = json!({
                        "executable": exe,
                        "args": cmd_args,
                        "cwd": resolved_root.to_string_lossy(),
                        "runtime": runtime_opt.unwrap_or_else(|| "python".into())
                    });
                }
                Some("static") | Some("html") => {
                    args["kind"] = json!({"type": "static"});
                }
                Some("proxy") => {
                    let port = port_opt.unwrap_or(8080);
                    args["kind"] = json!({
                        "type": "proxy",
                        "upstream_port": port,
                        "upstream_host": null,
                        "upstream_https": false
                    });
                    if let Some(cmd_str) = command_opt {
                        let (exe, cmd_args) = split_command_line(&cmd_str);
                        args["app"] = json!({
                            "executable": exe,
                            "args": cmd_args,
                            "cwd": resolved_root.to_string_lossy(),
                            "runtime": runtime_opt
                        });
                    }
                }
                _ => {
                    if let Some(cmd_str) = command_opt {
                        let port = port_opt.unwrap_or(3000);
                        args["kind"] = json!({
                            "type": "proxy",
                            "upstream_port": port,
                            "upstream_host": null,
                            "upstream_https": false
                        });
                        let (exe, cmd_args) = split_command_line(&cmd_str);
                        args["app"] = json!({
                            "executable": exe,
                            "args": cmd_args,
                            "cwd": resolved_root.to_string_lossy(),
                            "runtime": runtime_opt
                        });
                    } else if let Some(php_ver) = opt_str!("php") {
                        args["kind"] = json!({"type": "php", "version": php_ver});
                    }
                }
            }

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
        "ols_list_databases" => {
            let engine = opt_str!("engine").unwrap_or_else(|| "mariadb".into());
            let engine_lower = engine.to_ascii_lowercase();
            let target_engine = match engine_lower.as_str() {
                "mysql" | "mariadb" => "mariadb",
                "postgres" | "postgresql" | "pgsql" => "postgres",
                _ => engine.as_str(),
            };
            C::ListDatabases {
                engine: target_engine.to_string(),
            }
        }
        "ols_create_database" => {
            let engine = req_str!("engine");
            let engine_lower = engine.to_ascii_lowercase();
            let norm_engine = match engine_lower.as_str() {
                "mysql" | "mariadb" => "mariadb",
                "postgres" | "postgresql" | "pgsql" => "postgres",
                "sqlite" | "sqlite3" => "sqlite",
                _ => engine.as_str(),
            };
            if norm_engine == "sqlite" {
                let name = req_str!("name");
                let pb = std::path::Path::new(&name);
                let target_path = if pb.is_absolute() {
                    pb.to_path_buf()
                } else {
                    resolve_sites_dir().join(pb)
                };
                C::CreateSqlite {
                    path: target_path.to_string_lossy().to_string(),
                    project_id: None,
                }
            } else {
                C::CreateDatabase {
                    engine: norm_engine.to_string(),
                    name: req_str!("name"),
                    user: opt_str!("user"),
                    password: opt_str!("password"),
                }
            }
        }
        "ols_list_tables" => C::ListDatabaseTables {
            engine: req_str!("engine"),
            database: req_str!("database"),
        },
        "ols_execute_sql" | "ols_query_database" => {
            let sql = args
                .get("sql")
                .or_else(|| args.get("query"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required argument: sql".to_string())?
                .to_string();
            C::ExecuteSql {
                engine: req_str!("engine"),
                database: opt_str!("database"),
                sql,
            }
        }
        "ols_apply_web" => C::ApplyWeb { overwrite: vec![] },
        "ols_validate_web" => C::ValidateWeb,
        "ols_list_tasks" => C::ListTasks,
        "ols_docker_container_logs" => C::GetDockerContainerLogs {
            id: req_str!("id"),
            tail: Some(
                args.get("tail")
                    .and_then(|v| v.as_u64())
                    .map(|t| t.clamp(1, 2000) as u32)
                    .unwrap_or(200),
            ),
        },
        "ols_docker_container_details" => C::GetDockerContainer { id: req_str!("id") },
        "ols_docker_container_update" => C::UpdateDockerContainer {
            id: req_str!("id"),
            update: ols_core::docker::details::ContainerUpdate {
                restart_policy: opt_str!("restart_policy"),
                max_retries: args.get("max_retries").and_then(|v| v.as_u64()),
                memory_mb: args.get("memory_mb").and_then(|v| v.as_u64()),
                cpus: args.get("cpus").and_then(|v| v.as_f64()),
            },
        },
        "ols_docker_compose_list" => C::ListDockerComposeStacks,
        "ols_docker_compose_check" => {
            let path = req_str!("path");
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("could not read {path}: {e}"))?;
            C::ValidateDockerCompose { path, content }
        }
        "ols_docker_compose_action" => {
            let action = req_str!("action");
            C::DockerComposeAction {
                path: req_str!("path"),
                action: serde_json::from_value(json!(action)).map_err(|_| {
                    format!("unknown action '{action}': use up, down, stop, start, restart, pull or build")
                })?,
                service: opt_str!("service"),
            }
        }
        "ols_docker_compose_connect" => match opt_bool!("enabled") {
            Some(enabled) => C::SetDockerComposeConnect {
                path: req_str!("path"),
                enabled,
            },
            None => C::GetDockerComposeConnect {
                path: req_str!("path"),
            },
        },
        "ols_docker_compose_presets" => C::ListDockerComposePresets,
        "ols_docker_compose_create" => {
            let spec: ols_core::docker::builder::BuildSpec = serde_json::from_value(json!({
                "dir": req_str!("dir"),
                "name": opt_str!("name"),
                "services": args.get("services").cloned().unwrap_or(json!([])),
            }))
            .map_err(|e| format!("invalid services: {e}"))?;
            if opt_bool!("dry_run").unwrap_or(false) {
                C::PlanDockerCompose { spec }
            } else {
                // Never replaces an existing file from an AI tool; the app asks first.
                C::CreateDockerCompose {
                    spec,
                    overwrite: false,
                }
            }
        }
        "ols_docker_create_site" => C::DockerCreateSite {
            request: ols_core::docker::sites::DockerSiteRequest {
                container: req_str!("container"),
                container_port: args
                    .get("container_port")
                    .and_then(|v| v.as_u64())
                    .and_then(|p| u16::try_from(p).ok())
                    .ok_or("missing or invalid argument: container_port")?,
                hostname: req_str!("hostname"),
                https: opt_bool!("https").unwrap_or(true),
            },
        },
        "ols_unignore_site" => C::UnignoreSite {
            project_id: req_str!("project_id"),
        },
        "ols_docker_status" => C::GetDockerStatus,
        "ols_docker_start" => C::StartDockerDesktop,
        "ols_docker_list_containers" => C::ListDockerContainers {
            all: opt_bool!("all").unwrap_or(false),
        },
        "ols_docker_container_action" => {
            let action = req_str!("action");
            C::DockerContainerAction {
                id: req_str!("id"),
                action: serde_json::from_value(json!(action)).map_err(|_| {
                    format!("unknown action '{action}': use start, stop, restart, pause or unpause")
                })?,
            }
        }
        "ols_restart_site_app" => C::RestartSiteApp {
            hostname: req_str!("hostname"),
        },
        "ols_inspect_site_folder" => {
            let path_str = opt_str!("path");
            let target_path = match path_str {
                Some(p) => {
                    let pb = std::path::Path::new(&p);
                    if pb.is_absolute() {
                        pb.to_path_buf()
                    } else {
                        resolve_sites_dir().join(pb)
                    }
                }
                None => resolve_sites_dir(),
            };
            C::InspectSiteFolder {
                path: target_path.to_string_lossy().to_string(),
            }
        }
        "ols_list_runtimes" => C::ListRuntimeCatalog,
        "ols_install_runtime" => C::InstallRuntime {
            id: req_str!("id"),
            version: req_str!("version"),
        },
        "ols_refresh_runtimes" => C::RefreshRuntimeCatalog { id: req_str!("id") },
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
        "ols_list_projects" => C::ListProjects,
        "ols_add_project" => {
            let path_raw = req_str!("path");
            let p = std::path::Path::new(&path_raw);
            let resolved_path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                resolve_sites_dir().join(p)
            };
            C::AddProject {
                path: resolved_path.to_string_lossy().to_string(),
                name: opt_str!("name"),
                project_type: opt_str!("project_type"),
                auto_site: opt_bool!("auto_site").unwrap_or(false),
            }
        }
        "ols_update_project" => C::UpdateProject {
            id: req_str!("id"),
            name: opt_str!("name"),
            icon: opt_str!("icon"),
            clear_icon: opt_bool!("clear_icon"),
            project_type: opt_str!("project_type"),
            clear_project_type: opt_bool!("clear_project_type"),
        },
        "ols_inspect_project_folder" => {
            let path_raw = req_str!("path");
            let p = std::path::Path::new(&path_raw);
            let resolved_path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                resolve_sites_dir().join(p)
            };
            C::InspectProjectFolder {
                path: resolved_path.to_string_lossy().to_string(),
            }
        }
        "ols_list_project_types" => C::ListCustomProjectTypes,
        "ols_add_custom_project_type" => C::AddCustomProjectType {
            name: req_str!("name"),
        },
        "ols_rename_custom_project_type" => C::RenameCustomProjectType {
            old_name: req_str!("old_name"),
            new_name: req_str!("new_name"),
        },
        "ols_remove_custom_project_type" => C::RemoveCustomProjectType {
            name: req_str!("name"),
        },
        "ols_list_editors" => C::ListEditors,
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
            let mut s = format!(
                "Domain {}:\n- Document root: {}\n- Server: {}\n- Enabled: {}\n- HTTPS: {}\n- URL: {}",
                d.hostname,
                d.root,
                d.server.as_deref().unwrap_or("-"),
                d.enabled,
                d.https,
                d.url()
            );
            if let Some(app) = &d.app {
                s.push_str(&format!(
                    "\n- Supervised app: {} {}\n  cwd: {}\n  runtime: {}",
                    app.executable,
                    app.args.join(" "),
                    app.cwd,
                    app.runtime.as_deref().unwrap_or("system")
                ));
            }
            s
        }
        CoreResponse::SiteFolderInspection { inspection } => {
            let mut out = format!(
                "Site Folder Inspection:\n- Framework: {}\n- Suggested command: {}\n- Suggested port: {}\n",
                inspection.framework,
                inspection.suggested_command.as_deref().unwrap_or("none"),
                inspection.suggested_port,
            );
            if let Some(pm) = &inspection.detected_package_manager {
                out.push_str(&format!("- Package manager: {pm}\n"));
            }
            if let Some(venv) = &inspection.detected_venv {
                out.push_str(&format!("- Active virtualenv: {venv}\n"));
            }
            if !inspection.available_venvs.is_empty() {
                out.push_str("- Available virtualenvs:\n");
                for v in &inspection.available_venvs {
                    out.push_str(&format!(
                        "  * {} (python: {}, exe: {})\n",
                        v.path,
                        v.version.as_deref().unwrap_or("unknown"),
                        v.python_exe
                    ));
                }
            }
            if !inspection.requirements_files.is_empty() {
                out.push_str(&format!(
                    "- Requirements files: {}\n",
                    inspection.requirements_files.join(", ")
                ));
            }
            if !inspection.entry_files.is_empty() {
                out.push_str(&format!(
                    "- Entry files: {}\n",
                    inspection.entry_files.join(", ")
                ));
            }
            if !inspection.package_scripts.is_empty() {
                out.push_str("- Package scripts:\n");
                for script in &inspection.package_scripts {
                    out.push_str(&format!("  * {}: {}\n", script.name, script.description));
                }
            }
            out
        }
        CoreResponse::RuntimeCatalog { entries } => {
            if entries.is_empty() {
                "No runtimes available.".into()
            } else {
                let mut out = String::from("Available runtimes:\n");
                for entry in entries {
                    let mut flags = Vec::new();
                    if entry.installed {
                        flags.push("installed");
                    }
                    if entry.is_default {
                        flags.push("default");
                    }
                    let flags_str = if flags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", flags.join(", "))
                    };
                    out.push_str(&format!(
                        "- {} ({}) {}{}\n",
                        entry.id, entry.name, entry.version, flags_str
                    ));
                    if let Some(sys) = &entry.system {
                        out.push_str(&format!(
                            "  System install found: {} ({})\n",
                            sys.version, sys.path
                        ));
                    }
                }
                out
            }
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
        CoreResponse::Projects { projects, .. } => {
            if projects.is_empty() {
                "No projects registered.".into()
            } else {
                let mut out = String::from("Registered projects:\n");
                for p in projects {
                    let ptype = p.project_type.as_deref().unwrap_or("auto");
                    out.push_str(&format!(
                        "- {} [{}] (type: {}, path: {}, standalone: {})\n",
                        p.name, p.id, ptype, p.path, p.standalone
                    ));
                }
                out
            }
        }
        CoreResponse::Project { project } => {
            format!(
                "Project '{}' [{}]:\n- Path: {}\n- Type: {}\n- Health: {:?}\n- Standalone: {}",
                project.name,
                project.id,
                project.path,
                project.project_type.as_deref().unwrap_or("auto"),
                project.health,
                project.standalone
            )
        }
        CoreResponse::ProjectFolderInspection { inspection } => {
            let mut out = format!(
                "Project Folder Inspection:\n- Path: {}\n- Exists: {}\n- Is directory: {}\n- Suggested name: {}\n- Detected type: {}\n",
                inspection.path,
                inspection.exists,
                inspection.is_dir,
                inspection.suggested_name,
                inspection.detected_type.as_deref().unwrap_or("unknown")
            );
            if let Some(dup) = &inspection.duplicate_of {
                out.push_str(&format!(
                    "- Duplicate of existing project: '{}' ({})\n",
                    dup.name, dup.id
                ));
            }
            out
        }
        CoreResponse::CustomProjectTypes { types } => {
            if types.is_empty() {
                "No custom project types registered.".into()
            } else {
                let mut out = String::from("Custom project types:\n");
                for t in types {
                    out.push_str(&format!("- {} ({})\n", t.name, t.id));
                }
                out
            }
        }
        CoreResponse::Editors { editors } => {
            if editors.is_empty() {
                "No editors found.".into()
            } else {
                let mut out = String::from("Configured & detected editors:\n");
                for e in editors {
                    let exe = e.path.as_deref().unwrap_or("none");
                    let installed = e.path.is_some();
                    out.push_str(&format!(
                        "- {} [{}] (path: {}, installed: {}, visible: {})\n",
                        e.name, e.id, exe, installed, !e.hidden
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
            if name == "ols_add_domain" {
                let cmd = match build_command(name, &args) {
                    Ok(c) => c,
                    Err(e) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32602, "message": e }),
                        );
                    }
                };
                let hostname = args
                    .get("hostname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                match send_command(cmd).await {
                    Ok(add_resp) => {
                        // Restart the server that this site is working on so SSL certificates are generated and applied
                        let mut server_id = args
                            .get("server")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| s.to_string());

                        if server_id.is_none() {
                            if let Ok(CoreResponse::WebConfig { config }) =
                                send_command(CoreCommand::GetWebConfig).await
                            {
                                server_id = Some(config.default_server);
                            }
                        }
                        let target_server = server_id.unwrap_or_else(|| "nginx".to_string());

                        let mut extra_info = String::new();
                        match send_command(CoreCommand::RestartService {
                            id: target_server.clone(),
                        })
                        .await
                        {
                            Ok(_) => {
                                extra_info.push_str(&format!(
                                    "\n\nWeb server '{target_server}' was restarted and SSL certificate applied."
                                ));
                            }
                            Err(e) => {
                                extra_info.push_str(&format!(
                                    "\n\nNote: site was added, but restarting web server '{target_server}' encountered: {e}"
                                ));
                            }
                        }

                        // If the site has an app (Node.js/Python), restart the supervised site app process
                        let has_app = args.get("command").is_some()
                            || args
                                .get("kind")
                                .and_then(|v| v.as_str())
                                .map_or(false, |k| {
                                    k.eq_ignore_ascii_case("node")
                                        || k.eq_ignore_ascii_case("python")
                                });
                        if has_app && !hostname.is_empty() {
                            let _ = send_command(CoreCommand::RestartSiteApp {
                                hostname: hostname.clone(),
                            })
                            .await;
                        }

                        let text = format!("{}{}", format_response(&add_resp), extra_info);
                        return JsonRpcResponse::ok(
                            req.id,
                            json!({ "content": [{ "type": "text", "text": text }] }),
                        );
                    }
                    Err(e) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32603, "message": e }),
                        );
                    }
                }
            }
            if name == "ols_docker_create_site" {
                let cmd = match build_command(name, &args) {
                    Ok(c) => c,
                    Err(e) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32602, "message": e }),
                        );
                    }
                };
                return match send_command(cmd).await {
                    Ok(resp) => {
                        // Same as a site added in the app: apply the web config so it answers.
                        let note = match send_command(CoreCommand::ApplyWeb { overwrite: vec![] }).await {
                            Ok(_) => "\n\nWeb server config applied; the site is live.".to_string(),
                            Err(e) => format!("\n\nNote: the site was created, but applying the web config failed: {e}"),
                        };
                        JsonRpcResponse::ok(
                            req.id,
                            json!({ "content": [{ "type": "text", "text": format!("{}{note}", format_response(&resp)) }] }),
                        )
                    }
                    Err(e) => JsonRpcResponse::err(req.id, json!({ "code": -32603, "message": e })),
                };
            }
            if name == "ols_list_tables" {
                let db_name = args.get("database").and_then(|v| v.as_str()).unwrap_or("");
                match build_command(name, &args) {
                    Ok(cmd) => match send_command(cmd).await {
                        Ok(CoreResponse::Names { names }) => {
                            let text = if names.is_empty() {
                                format!("No tables found in database '{db_name}'.")
                            } else {
                                format!(
                                    "Tables in database '{db_name}' ({} total):\n{}",
                                    names.len(),
                                    names
                                        .iter()
                                        .map(|t| format!("- {t}"))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                )
                            };
                            return JsonRpcResponse::ok(
                                req.id,
                                json!({ "content": [{ "type": "text", "text": text }] }),
                            );
                        }
                        Ok(other) => {
                            return JsonRpcResponse::ok(
                                req.id,
                                json!({ "content": [{ "type": "text", "text": format_response(&other) }] }),
                            );
                        }
                        Err(e) => {
                            return JsonRpcResponse::err(
                                req.id,
                                json!({ "code": -32603, "message": e }),
                            );
                        }
                    },
                    Err(e) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32603, "message": e }),
                        );
                    }
                }
            }
            if name == "ols_list_databases" {
                let engine_opt = args
                    .get("engine")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty() && s != "all");

                match engine_opt.as_deref() {
                    Some("sqlite") | Some("sqlite3") => {
                        match send_command(CoreCommand::ListSqlite).await {
                            Ok(CoreResponse::SqliteList { databases }) => {
                                if databases.is_empty() {
                                    return JsonRpcResponse::ok(
                                        req.id,
                                        json!({ "content": [{ "type": "text", "text": "No SQLite databases registered." }] }),
                                    );
                                }
                                let mut out = String::from("SQLite databases:\n");
                                for d in databases {
                                    out.push_str(&format!(
                                        "- {} ({} bytes)\n",
                                        d.path, d.size_bytes
                                    ));
                                }
                                return JsonRpcResponse::ok(
                                    req.id,
                                    json!({ "content": [{ "type": "text", "text": out }] }),
                                );
                            }
                            Ok(other) => {
                                return JsonRpcResponse::ok(
                                    req.id,
                                    json!({ "content": [{ "type": "text", "text": format_response(&other) }] }),
                                );
                            }
                            Err(e) => {
                                return JsonRpcResponse::err(
                                    req.id,
                                    json!({ "code": -32603, "message": e }),
                                );
                            }
                        }
                    }
                    Some(eng) => {
                        let target_engine = match eng {
                            "mysql" | "mariadb" => "mariadb",
                            "postgresql" | "pgsql" | "postgres" => "postgres",
                            other => other,
                        };
                        match send_command(CoreCommand::ListDatabases {
                            engine: target_engine.to_string(),
                        })
                        .await
                        {
                            Ok(CoreResponse::Names { names }) => {
                                let text = if names.is_empty() {
                                    format!("No databases found for {target_engine}.")
                                } else {
                                    format!(
                                        "{target_engine} databases:\n{}",
                                        names
                                            .iter()
                                            .map(|n| format!("- {n}"))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    )
                                };
                                return JsonRpcResponse::ok(
                                    req.id,
                                    json!({ "content": [{ "type": "text", "text": text }] }),
                                );
                            }
                            Ok(other) => {
                                return JsonRpcResponse::ok(
                                    req.id,
                                    json!({ "content": [{ "type": "text", "text": format_response(&other) }] }),
                                );
                            }
                            Err(e) => {
                                return JsonRpcResponse::err(
                                    req.id,
                                    json!({ "code": -32603, "message": e }),
                                );
                            }
                        }
                    }
                    None => {
                        // Query all engines: MariaDB, PostgreSQL, SQLite
                        let mut sections = Vec::new();

                        // 1. MariaDB
                        if let Ok(CoreResponse::Names { names }) =
                            send_command(CoreCommand::ListDatabases {
                                engine: "mariadb".into(),
                            })
                            .await
                        {
                            if !names.is_empty() {
                                sections.push(format!(
                                    "MariaDB / MySQL:\n{}",
                                    names
                                        .iter()
                                        .map(|n| format!("  - {n}"))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                ));
                            }
                        }

                        // 2. PostgreSQL
                        if let Ok(CoreResponse::Names { names }) =
                            send_command(CoreCommand::ListDatabases {
                                engine: "postgres".into(),
                            })
                            .await
                        {
                            if !names.is_empty() {
                                sections.push(format!(
                                    "PostgreSQL:\n{}",
                                    names
                                        .iter()
                                        .map(|n| format!("  - {n}"))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                ));
                            }
                        }

                        // 3. SQLite
                        if let Ok(CoreResponse::SqliteList { databases }) =
                            send_command(CoreCommand::ListSqlite).await
                        {
                            if !databases.is_empty() {
                                sections.push(format!(
                                    "SQLite:\n{}",
                                    databases
                                        .iter()
                                        .map(|d| format!("  - {} ({} bytes)", d.path, d.size_bytes))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                ));
                            }
                        }

                        let text = if sections.is_empty() {
                            "No databases found across engines (MariaDB, PostgreSQL, SQLite). Ensure database services are started.".to_string()
                        } else {
                            format!("Databases across engines:\n\n{}", sections.join("\n\n"))
                        };

                        return JsonRpcResponse::ok(
                            req.id,
                            json!({ "content": [{ "type": "text", "text": text }] }),
                        );
                    }
                }
            }
            if name == "ols_update_domain" {
                let hostname = match args.get("hostname").and_then(|v| v.as_str()) {
                    Some(h) if !h.trim().is_empty() => h.to_string(),
                    _ => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32602, "message": "missing required argument: hostname" }),
                        );
                    }
                };

                // Fetch existing domain so partial updates (e.g. toggling https) preserve all other fields
                match send_command(CoreCommand::GetDomain {
                    hostname: hostname.clone(),
                })
                .await
                {
                    Ok(CoreResponse::Domain { mut domain }) => {
                        if let Some(https) = args.get("https").and_then(|v| v.as_bool()) {
                            domain.https = https;
                            if https && !domain.redirect_https {
                                domain.redirect_https = true;
                            }
                        }
                        if let Some(redirect_https) =
                            args.get("redirect_https").and_then(|v| v.as_bool())
                        {
                            domain.redirect_https = redirect_https;
                        }
                        if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                            domain.enabled = enabled;
                        }
                        if let Some(server) = args.get("server").and_then(|v| v.as_str()) {
                            domain.server = Some(server.to_string());
                        }
                        if let Some(root_str) = args.get("root").and_then(|v| v.as_str()) {
                            domain.root = resolve_domain_root(Some(root_str), &hostname)
                                .to_string_lossy()
                                .to_string();
                        }
                        let kind_str = args
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_lowercase());
                        let port_opt = args.get("port").and_then(|v| v.as_u64()).map(|p| p as u16);
                        let command_opt = args
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let runtime_opt = args
                            .get("runtime")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let venv_opt = args
                            .get("venv")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let php_opt = args
                            .get("php")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let resolved_root = std::path::PathBuf::from(&domain.root);

                        match kind_str.as_deref() {
                            Some("node") => {
                                let port = port_opt.unwrap_or(3000);
                                domain.kind = ols_core::domain::SiteKind::Proxy {
                                    upstream_port: port,
                                    upstream_host: None,
                                    upstream_https: false,
                                };
                                let cmd_str = command_opt.as_deref().unwrap_or("npm run dev");
                                let (exe, cmd_args) = split_command_line(cmd_str);
                                domain.app = Some(ols_core::domain::AppSpec {
                                    executable: exe,
                                    args: cmd_args,
                                    cwd: domain.root.clone(),
                                    runtime: Some(runtime_opt.unwrap_or_else(|| "node".into())),
                                });
                            }
                            Some("python") => {
                                let port = port_opt.unwrap_or(8000);
                                domain.kind = ols_core::domain::SiteKind::Proxy {
                                    upstream_port: port,
                                    upstream_host: None,
                                    upstream_https: false,
                                };
                                let cmd_str = command_opt.as_deref().unwrap_or("python main.py");
                                let (mut exe, cmd_args) = split_command_line(cmd_str);
                                if let Some(ref venv) = venv_opt {
                                    let vpath = std::path::Path::new(venv);
                                    if vpath.ends_with("python.exe") || vpath.ends_with("python") {
                                        exe = venv.clone();
                                    } else {
                                        let cand = resolved_root
                                            .join(vpath)
                                            .join("Scripts")
                                            .join("python.exe");
                                        exe = cand.to_string_lossy().to_string();
                                    }
                                }
                                domain.app = Some(ols_core::domain::AppSpec {
                                    executable: exe,
                                    args: cmd_args,
                                    cwd: domain.root.clone(),
                                    runtime: Some(runtime_opt.unwrap_or_else(|| "python".into())),
                                });
                            }
                            Some("static") | Some("html") => {
                                domain.kind = ols_core::domain::SiteKind::Static;
                                domain.app = None;
                            }
                            Some("proxy") => {
                                let port = port_opt.unwrap_or(8080);
                                domain.kind = ols_core::domain::SiteKind::Proxy {
                                    upstream_port: port,
                                    upstream_host: None,
                                    upstream_https: false,
                                };
                                if let Some(cmd_str) = command_opt {
                                    let (exe, cmd_args) = split_command_line(&cmd_str);
                                    domain.app = Some(ols_core::domain::AppSpec {
                                        executable: exe,
                                        args: cmd_args,
                                        cwd: domain.root.clone(),
                                        runtime: runtime_opt,
                                    });
                                }
                            }
                            Some("php") => {
                                domain.kind = ols_core::domain::SiteKind::Php { version: php_opt };
                                domain.app = None;
                            }
                            None => {
                                if let Some(php_ver) = php_opt {
                                    domain.kind = ols_core::domain::SiteKind::Php {
                                        version: Some(php_ver),
                                    };
                                }
                                if let Some(cmd_str) = command_opt {
                                    let (exe, cmd_args) = split_command_line(&cmd_str);
                                    domain.app = Some(ols_core::domain::AppSpec {
                                        executable: exe,
                                        args: cmd_args,
                                        cwd: domain.root.clone(),
                                        runtime: runtime_opt,
                                    });
                                }
                            }
                            _ => {}
                        }

                        match send_command(CoreCommand::UpdateDomain { domain: *domain }).await {
                            Ok(response) => {
                                let text = format_response(&response);
                                return JsonRpcResponse::ok(
                                    req.id,
                                    json!({ "content": [{ "type": "text", "text": text }] }),
                                );
                            }
                            Err(e) => {
                                return JsonRpcResponse::err(
                                    req.id,
                                    json!({ "code": -32603, "message": e }),
                                )
                            }
                        }
                    }
                    Ok(_) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32603, "message": "unexpected response when fetching domain" }),
                        )
                    }
                    Err(e) => {
                        return JsonRpcResponse::err(
                            req.id,
                            json!({ "code": -32603, "message": e }),
                        )
                    }
                }
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
    use ols_core::domain::SiteKind;

    #[test]
    fn docker_tools_build_commands() {
        match build_command(
            "ols_docker_container_update",
            &json!({ "id": "db", "memory_mb": 512, "cpus": 1.5 }),
        )
        .unwrap()
        {
            CoreCommand::UpdateDockerContainer { id, update } => {
                assert_eq!(id, "db");
                assert_eq!((update.memory_mb, update.cpus), (Some(512), Some(1.5)));
                assert!(update.restart_policy.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
        let all: Vec<String> = mcp_tools().into_iter().map(|t| t.name).collect();
        assert!(
            !all.iter()
                .any(|n| n.starts_with("ols_docker")
                    && (n.contains("remove") || n.contains("rename"))),
            "no destructive container tools"
        );
        let names: Vec<String> = mcp_tools().into_iter().map(|t| t.name).collect();
        for n in [
            "ols_docker_status",
            "ols_docker_start",
            "ols_docker_list_containers",
            "ols_docker_container_action",
        ] {
            assert!(names.iter().any(|x| x == n), "{n} missing from tools/list");
        }
        assert!(matches!(
            build_command("ols_docker_list_containers", &json!({ "all": true })).unwrap(),
            CoreCommand::ListDockerContainers { all: true }
        ));
        assert!(matches!(
            build_command("ols_docker_list_containers", &json!({})).unwrap(),
            CoreCommand::ListDockerContainers { all: false }
        ));
        match build_command(
            "ols_docker_container_action",
            &json!({ "id": "shop-web-1", "action": "restart" }),
        )
        .unwrap()
        {
            CoreCommand::DockerContainerAction { id, action } => {
                assert_eq!(id, "shop-web-1");
                assert_eq!(action, ols_core::docker::ContainerAction::Restart);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            build_command("ols_unignore_site", &json!({ "project_id": "p1" })).unwrap(),
            CoreCommand::UnignoreSite { project_id } if project_id == "p1"
        ));
        assert!(matches!(
            build_command("ols_docker_compose_action", &json!({ "path": "C:/s/compose.yaml", "action": "up", "service": "web" })).unwrap(),
            CoreCommand::DockerComposeAction { service: Some(s), .. } if s == "web"
        ));
        assert!(build_command(
            "ols_docker_compose_action",
            &json!({ "path": "x", "action": "rm" })
        )
        .is_err());
        assert!(matches!(
            build_command(
                "ols_docker_compose_connect",
                &json!({ "path": "C:/s/compose.yaml" })
            )
            .unwrap(),
            CoreCommand::GetDockerComposeConnect { .. }
        ));
        assert!(matches!(
            build_command(
                "ols_docker_compose_connect",
                &json!({ "path": "C:/s/compose.yaml", "enabled": true })
            )
            .unwrap(),
            CoreCommand::SetDockerComposeConnect { enabled: true, .. }
        ));
        match build_command(
            "ols_docker_compose_create",
            &json!({ "dir": "C:/s", "services": [{ "preset": "redis", "name": "cache" }] }),
        )
        .unwrap()
        {
            CoreCommand::CreateDockerCompose { spec, overwrite } => {
                assert!(!overwrite, "MCP never replaces an existing compose file");
                assert_eq!(spec.services[0].name, "cache");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            build_command(
                "ols_docker_compose_create",
                &json!({ "dir": "C:/s", "services": [], "dry_run": true })
            )
            .unwrap(),
            CoreCommand::PlanDockerCompose { .. }
        ));
        match build_command(
            "ols_docker_create_site",
            &json!({ "container": "shop-web-1", "container_port": 80, "hostname": "shop.test" }),
        )
        .unwrap()
        {
            CoreCommand::DockerCreateSite { request } => {
                assert_eq!((request.container_port, request.https), (80, true));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(build_command(
            "ols_docker_create_site",
            &json!({ "container": "x", "container_port": 70000, "hostname": "a.test" })
        )
        .is_err());
        assert!(matches!(
            build_command(
                "ols_docker_container_logs",
                &json!({ "id": "x", "tail": 99999 })
            )
            .unwrap(),
            CoreCommand::GetDockerContainerLogs {
                tail: Some(2000),
                ..
            }
        ));
        let err = build_command(
            "ols_docker_container_action",
            &json!({ "id": "x", "action": "remove" }),
        )
        .unwrap_err();
        assert!(err.contains("unknown action"), "{err}");
    }

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
            assert!(domain.https, "should default https to true");
            assert!(
                domain.redirect_https,
                "should default redirect_https to true"
            );
        } else {
            panic!("expected AddDomain variant");
        }
    }

    #[test]
    fn build_command_add_domain_with_explicit_https_false() {
        let cmd = build_command(
            "ols_add_domain",
            &json!({
                "hostname": "http-only.local",
                "https": false
            }),
        )
        .expect("should build AddDomain");

        if let CoreCommand::AddDomain { domain } = cmd {
            assert!(!domain.https);
            assert!(!domain.redirect_https);
        } else {
            panic!("expected AddDomain variant");
        }
    }

    #[test]
    fn tools_schema_includes_https_parameters() {
        let tools = mcp_tools();
        let add_tool = tools.iter().find(|t| t.name == "ols_add_domain").unwrap();
        assert!(add_tool.input_schema["properties"]["https"].is_object());
        assert!(add_tool.input_schema["properties"]["redirect_https"].is_object());

        let update_tool = tools
            .iter()
            .find(|t| t.name == "ols_update_domain")
            .unwrap();
        assert!(update_tool.input_schema["properties"]["https"].is_object());
        assert!(update_tool.input_schema["properties"]["redirect_https"].is_object());
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

    #[test]
    fn build_command_add_domain_python_with_command_and_venv() {
        let cmd = build_command(
            "ols_add_domain",
            &json!({
                "hostname": "pyapp.test",
                "kind": "python",
                "command": "python main.py --host 0.0.0.0",
                "port": 8000,
                "runtime": "3.11",
                "venv": ".venv"
            }),
        )
        .expect("should build AddDomain for python");

        if let CoreCommand::AddDomain { domain } = cmd {
            assert_eq!(domain.hostname, "pyapp.test");
            match domain.kind {
                SiteKind::Proxy { upstream_port, .. } => assert_eq!(upstream_port, 8000),
                _ => panic!("expected Proxy site kind"),
            }
            let app = domain.app.expect("expected AppSpec");
            assert_eq!(app.args, vec!["main.py", "--host", "0.0.0.0"]);
            assert_eq!(app.runtime.as_deref(), Some("3.11"));
            assert!(app.executable.contains(".venv"));
        } else {
            panic!("expected AddDomain variant");
        }
    }

    #[test]
    fn build_command_add_domain_node() {
        let cmd = build_command(
            "ols_add_domain",
            &json!({
                "hostname": "nodeapp.test",
                "kind": "node",
                "command": "npm run start",
                "port": 3000,
                "runtime": "20.18.0"
            }),
        )
        .expect("should build AddDomain for node");

        if let CoreCommand::AddDomain { domain } = cmd {
            assert_eq!(domain.hostname, "nodeapp.test");
            match domain.kind {
                SiteKind::Proxy { upstream_port, .. } => assert_eq!(upstream_port, 3000),
                _ => panic!("expected Proxy site kind"),
            }
            let app = domain.app.expect("expected AppSpec");
            assert_eq!(app.executable, "npm");
            assert_eq!(app.args, vec!["run", "start"]);
            assert_eq!(app.runtime.as_deref(), Some("20.18.0"));
        } else {
            panic!("expected AddDomain variant");
        }
    }

    #[test]
    fn build_command_restart_site_app() {
        let cmd = build_command(
            "ols_restart_site_app",
            &json!({
                "hostname": "my-site.test"
            }),
        )
        .expect("should build RestartSiteApp");

        if let CoreCommand::RestartSiteApp { hostname } = cmd {
            assert_eq!(hostname, "my-site.test");
        } else {
            panic!("expected RestartSiteApp variant");
        }
    }

    #[test]
    fn build_command_runtime_tools() {
        let list_cmd = build_command("ols_list_runtimes", &json!({})).unwrap();
        assert!(matches!(list_cmd, CoreCommand::ListRuntimeCatalog));

        let install_cmd = build_command(
            "ols_install_runtime",
            &json!({ "id": "python", "version": "3.12.6" }),
        )
        .unwrap();
        if let CoreCommand::InstallRuntime { id, version } = install_cmd {
            assert_eq!(id, "python");
            assert_eq!(version, "3.12.6");
        } else {
            panic!("expected InstallRuntime");
        }

        let refresh_cmd =
            build_command("ols_refresh_runtimes", &json!({ "id": "python" })).unwrap();
        if let CoreCommand::RefreshRuntimeCatalog { id } = refresh_cmd {
            assert_eq!(id, "python");
        } else {
            panic!("expected RefreshRuntimeCatalog");
        }
    }

    #[test]
    fn build_command_database_tables_and_sql() {
        let tools = mcp_tools();
        assert!(tools.iter().any(|t| t.name == "ols_list_tables"));
        assert!(tools.iter().any(|t| t.name == "ols_execute_sql"));

        let list_tables_cmd = build_command(
            "ols_list_tables",
            &json!({ "engine": "mariadb", "database": "shop" }),
        )
        .unwrap();
        if let CoreCommand::ListDatabaseTables { engine, database } = list_tables_cmd {
            assert_eq!(engine, "mariadb");
            assert_eq!(database, "shop");
        } else {
            panic!("expected ListDatabaseTables");
        }

        let exec_cmd = build_command(
            "ols_execute_sql",
            &json!({ "engine": "mariadb", "database": "shop", "sql": "SELECT * FROM users" }),
        )
        .unwrap();
        if let CoreCommand::ExecuteSql {
            engine,
            database,
            sql,
        } = exec_cmd
        {
            assert_eq!(engine, "mariadb");
            assert_eq!(database.as_deref(), Some("shop"));
            assert_eq!(sql, "SELECT * FROM users");
        } else {
            panic!("expected ExecuteSql");
        }

        // Test with query alias
        let query_cmd = build_command(
            "ols_query_database",
            &json!({ "engine": "sqlite", "query": "SELECT 1" }),
        )
        .unwrap();
        if let CoreCommand::ExecuteSql {
            engine,
            database,
            sql,
        } = query_cmd
        {
            assert_eq!(engine, "sqlite");
            assert!(database.is_none());
            assert_eq!(sql, "SELECT 1");
        } else {
            panic!("expected ExecuteSql");
        }

        // Test ols_list_databases without engine argument (should default)
        let list_db_default = build_command("ols_list_databases", &json!({})).unwrap();
        if let CoreCommand::ListDatabases { engine } = list_db_default {
            assert_eq!(engine, "mariadb");
        } else {
            panic!("expected ListDatabases");
        }

        // Test ols_list_databases with mysql alias
        let list_db_mysql =
            build_command("ols_list_databases", &json!({ "engine": "mysql" })).unwrap();
        if let CoreCommand::ListDatabases { engine } = list_db_mysql {
            assert_eq!(engine, "mariadb");
        } else {
            panic!("expected ListDatabases");
        }

        // Test ols_create_database with sqlite
        let create_db_sqlite = build_command(
            "ols_create_database",
            &json!({ "engine": "sqlite", "name": "app.db" }),
        )
        .unwrap();
        if let CoreCommand::CreateSqlite { path, .. } = create_db_sqlite {
            assert!(path.ends_with("app.db"));
        } else {
            panic!("expected CreateSqlite");
        }
    }

    #[test]
    fn build_command_project_and_editor_tools() {
        let tools = mcp_tools();
        assert!(tools.iter().any(|t| t.name == "ols_list_projects"));
        assert!(tools.iter().any(|t| t.name == "ols_add_project"));
        assert!(tools.iter().any(|t| t.name == "ols_update_project"));
        assert!(tools.iter().any(|t| t.name == "ols_inspect_project_folder"));
        assert!(tools.iter().any(|t| t.name == "ols_list_project_types"));
        assert!(tools
            .iter()
            .any(|t| t.name == "ols_add_custom_project_type"));
        assert!(tools
            .iter()
            .any(|t| t.name == "ols_rename_custom_project_type"));
        assert!(tools
            .iter()
            .any(|t| t.name == "ols_remove_custom_project_type"));
        assert!(tools.iter().any(|t| t.name == "ols_list_editors"));

        let list_proj_cmd = build_command("ols_list_projects", &json!({})).unwrap();
        assert!(matches!(list_proj_cmd, CoreCommand::ListProjects));

        let add_proj_cmd = build_command(
            "ols_add_project",
            &json!({ "path": "my-tauri-app", "project_type": "custom:Tauri" }),
        )
        .unwrap();
        if let CoreCommand::AddProject {
            path, project_type, ..
        } = add_proj_cmd
        {
            assert!(path.ends_with("my-tauri-app"));
            assert_eq!(project_type.as_deref(), Some("custom:Tauri"));
        } else {
            panic!("expected AddProject");
        }

        let inspect_cmd = build_command(
            "ols_inspect_project_folder",
            &json!({ "path": "C:/dev/app" }),
        )
        .unwrap();
        if let CoreCommand::InspectProjectFolder { path } = inspect_cmd {
            assert_eq!(path, "C:/dev/app");
        } else {
            panic!("expected InspectProjectFolder");
        }

        let list_types_cmd = build_command("ols_list_project_types", &json!({})).unwrap();
        assert!(matches!(
            list_types_cmd,
            CoreCommand::ListCustomProjectTypes
        ));

        let add_custom_cmd =
            build_command("ols_add_custom_project_type", &json!({ "name": "Tauri" })).unwrap();
        if let CoreCommand::AddCustomProjectType { name } = add_custom_cmd {
            assert_eq!(name, "Tauri");
        } else {
            panic!("expected AddCustomProjectType");
        }
    }
}
