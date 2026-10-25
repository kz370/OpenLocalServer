# OLS MCP Server

The OLS MCP (Model Context Protocol) server exposes Open Local Server as a stdio-based MCP service. Any MCP host (Claude Desktop, VS Code, Cursor, LM Studio, etc.) can connect to it and control OLS through JSON-RPC 2.0 tools.

## What it does

Each MCP tool maps one-to-one to an OLS `CoreCommand`, so an LLM can discover and safely call app operations:

- **Domains & Sites**: `ols_list_domains`, `ols_get_domain`, `ols_add_domain`, `ols_update_domain`, `ols_remove_domain`, `ols_set_domain_enabled`, `ols_restart_site_app`
- **Folder Inspection**: `ols_inspect_site_folder` (detects framework, package manager, virtual environments, requirements files, entry scripts, and suggested commands)
- **Runtimes**: `ols_list_runtimes`, `ols_install_runtime`, `ols_refresh_runtimes` (query and install Python, Node, and PHP runtimes)
- **Services**: `ols_list_services`, `ols_start_service`, `ols_stop_service`, `ols_restart_service`
- **Tunnels**: `ols_list_tunnels`, `ols_start_tunnel`, `ols_stop_tunnel`
- **Databases**: `ols_list_databases`, `ols_create_database`, `ols_list_tables` (list tables in MariaDB/MySQL, PostgreSQL, or SQLite), `ols_execute_sql` (view table content, schema, or modify data and tables via SQL)
- **Web server**: `ols_apply_web`, `ols_validate_web`
- **Tasks**: `ols_list_tasks`
- **Escape hatch**: `ols_run_command` runs any `CoreCommand` variant by name with JSON args.

## How to use it

The MCP server ships **with the OLS installer** — you do not need to build it yourself. It connects to the **already running** OLS app via its local control channel (named pipe + `control.json` session token), so OLS must be running (desktop app or `ols daemon`).

Point your MCP host at the `ols-mcp` binary (installed alongside `OLS.exe`). For example, in Claude Desktop's config:

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe"
    }
  }
}
```

Then restart Claude Desktop and you can ask it to control OLS:

> "List all my domains"
> "Create a new site called shop.test at C:\sites\shop with PHP 8.3"
> "Start the MariaDB service"
> "Create a MariaDB database called shop"

## Example tool call (JSON-RPC 2.0)

List all domains:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "ols_list_domains",
    "arguments": {}
  }
}
```

Create a site (PHP):

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "ols_add_domain",
    "arguments": {
      "hostname": "shop.test",
      "root": "C:\\sites\\shop",
      "php": "8.3",
      "server": "nginx",
      "enabled": true
    }
  }
}
```

Create a supervised Python or Node site:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "ols_add_domain",
    "arguments": {
      "hostname": "fastapi.test",
      "root": "C:\\sites\\fastapi-app",
      "kind": "python",
      "command": "python -m uvicorn main:app --port 8000",
      "port": 8000,
      "venv": ".venv",
      "runtime": "3.12.6"
    }
  }
}
```

Inspect a project folder (detect frameworks, virtualenvs, dependencies, and suggested start commands):

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "ols_inspect_site_folder",
    "arguments": {
      "path": "C:\\sites\\fastapi-app"
    }
  }
}
```

List tables in a database:

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {
    "name": "ols_list_tables",
    "arguments": {
      "engine": "mariadb",
      "database": "shop"
    }
  }
}
```

View table content or execute SQL:

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "tools/call",
  "params": {
    "name": "ols_execute_sql",
    "arguments": {
      "engine": "mariadb",
      "database": "shop",
      "sql": "SELECT id, username, email FROM users LIMIT 10;"
    }
  }
}
```

## Authentication

MCP authentication is **off by default** for backward compatibility. Enable it in OLS Settings → MCP / AI tools.

The token lives **only in the MCP host config** (next to the `command`) — not inside individual `tools/call` requests. Set it once, and every call is authenticated automatically.

Get the token from **OLS Settings → MCP / AI tools** (enable authentication, then generate or copy a token). Use the **Copy** button next to the token field. Rotating the token there invalidates all connected MCP hosts on the next request; just update the config with the new token.

### Option A: `--token` argument (recommended)

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe",
      "args": ["--token", "your-secret-token-here"]
    }
  }
}
```

### Option B: `OLS_MCP_TOKEN` environment variable

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe",
      "env": {
        "OLS_MCP_TOKEN": "your-secret-token-here"
      }
    }
  }
}
```

### Option C: `OLS_SITES_DIR` environment variable (default site root)

Sets the base sites directory for `ols_add_domain` / `ols_update_domain` and `ols_get_sites_dir`. Falls back to the `domains.sites_dir` setting, then the install's `sites` folder.

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe",
      "env": {
        "OLS_MCP_TOKEN": "your-token-here",
        "OLS_SITES_DIR": "C:\\sites"
      }
    }
  }
}
```

When creating sites (`ols_add_domain`):
- `root` is **optional**. If omitted, it automatically defaults to `<sites_dir>\<hostname>` (e.g. `C:\sites\myshop.test`).
- If a relative path is provided (e.g. `"myshop"` or `"myshop/public"`), it is automatically resolved under `<sites_dir>`.
- If an absolute path is provided, it is used as-is.
- AI clients can call the `ols_get_sites_dir` tool or read the `ols://sites-dir` resource to find where files should be placed.

### Expiry

Choose one expiry mode on the settings page:

- **Never expires** — token is valid until you rotate it.
- **Expires after (hours)** — token expires N hours after creation (TTL).
- **Expires at (date/time)** — absolute expiry date/time.

Leave on "Never expires" for no expiry. After expiry, all connections are rejected until you set a new expiry or regenerate the token.

The token is stored in the OS credential manager (Windows Credential Manager), never in settings files, logs, or the installer bundle.

## Domain creation notes

- `kind` accepts plain strings: `"php"`, `"static"`, `"html"`, `"proxy"`, `"node"`, or `"python"` (case-insensitive). Omitted or unrecognized values default to PHP.
- `root` is optional and defaults to `<sites_dir>\<hostname>` using `OLS_SITES_DIR`, then the `domains.sites_dir` setting, then the install's `sites` folder. Relative paths are resolved against this folder.
- `php` version (e.g. `"8.3"`) is used only when `kind` is `php`.
- **Node & Python sites**:
  - Setting `kind` to `"python"` or `"node"` configures a reverse proxy and runs a supervised background application process (`AppSpec`).
  - `command` specifies the launch command (e.g. `"python main.py"`, `"uvicorn main:app --port 8000"`, `"npm run start"`).
  - `port` specifies the listening port that the web server reverse proxies to (defaults to 8000 for Python, 3000 for Node).
  - `runtime` pins a specific runtime version (e.g. `"3.12.6"`, `"20.18.0"`). OLS prepends the matching runtime's binary directory to the process `PATH`.
  - `venv` (Python only) specifies the virtualenv folder or executable path relative to the site root (e.g. `".venv"` or `".venv/Scripts/python.exe"`).
- `ols_restart_site_app` takes `{"hostname": "mysite.test"}` to restart the supervised Python or Node dev server / application process.
- `ols_inspect_site_folder` inspects any project folder to discover frameworks (FastAPI, Flask, Django, Next.js, Express, etc.), package managers, virtual environments, requirements files, scripts, and suggested commands/ports.
- `ols_list_runtimes`, `ols_install_runtime`, `ols_refresh_runtimes` manage the system and OLS-managed versions for Python, Node, and PHP.
- `https` (boolean) enables SSL for the site. `redirect_https` (boolean) redirects HTTP to HTTPS.
- `enabled` (boolean) toggles the site on/off.

### SSL / HTTPS notes

- `https` (boolean, defaults to `true`) enables SSL for the site. `redirect_https` (boolean) automatically redirects HTTP traffic to HTTPS.
- OLS includes a built-in Local Certificate Authority (Local CA) that automatically provisions and installs valid, trusted SSL certificates for all local domains (including `.local`, `.test`, `.localhost`, and custom tlds) without requiring public DNS or manual configuration.
- To enable HTTPS on an existing domain via MCP, call `ols_update_domain` with `{"hostname": "mysite.local", "https": true, "redirect_https": true}`, followed by `ols_apply_web` to reload web server configurations.
- `ols_apply_web` regenerates web server configurations and ensures local certificates are minted for all domains where `https: true`. It does not take arguments and affects active web servers (Nginx, Apache, Caddy).

### write_web_config (via ols_run_command)

`ols_run_command` runs any `CoreCommand` variant. For `write_web_config`, `args` must include:

```json
{
  "type": "write_web_config",
  "hostname": "shop.test",
  "part": "site",
  "content": "<nginx config text here>"
}
```

- `type` must be `"write_web_config"` (internally-tagged CoreCommand variant).
- `hostname` — the site hostname.
- `part` — one of `"main"`, `"site"`, `"custom"` (maps to `ConfigPart`).
- `content` — the full config file text (required).

Missing `content` or an invalid `part` produces a clear serde error.


## Safety

Destructive operations (remove domain, remove tunnel, restore snapshot, import sites, env edits) are flagged as destructive by the core and require user confirmation in the app UI before they execute. The MCP server reports this as a diagnostic rather than running silently.

## Where it lives

- Crate: `crates/ols-mcp/`
- Entry: `crates/ols-mcp/src/main.rs`
- Library: `crates/ols-mcp/src/lib.rs`
- Registered in workspace `Cargo.toml` alongside `ols-core`, `ols-helper`, `ols-cli`.
