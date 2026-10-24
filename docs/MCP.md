# OLS MCP Server

The OLS MCP (Model Context Protocol) server exposes Open Local Server as a stdio-based MCP service. Any MCP host (Claude Desktop, VS Code, Cursor, LM Studio, etc.) can connect to it and control OLS through JSON-RPC 2.0 tools.

## What it does

Each MCP tool maps one-to-one to an OLS `CoreCommand`, so an LLM can discover and safely call app operations:

- **Domains**: `ols_list_domains`, `ols_get_domain`, `ols_add_domain`, `ols_update_domain`, `ols_remove_domain`, `ols_set_domain_enabled`
- **Services**: `ols_list_services`, `ols_start_service`, `ols_stop_service`, `ols_restart_service`
- **Tunnels**: `ols_list_tunnels`, `ols_start_tunnel`, `ols_stop_tunnel`
- **Databases**: `ols_list_databases`, `ols_create_database`
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

Create a site:

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

Sets the default `root` for `ols_add_domain` / `ols_update_domain` when no `root` is provided. Falls back to the `domains.sites_dir` setting, then the install's `sites` folder.

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe",
      "env": {
        "OLS_SITES_DIR": "C:\\sites"
      }
    }
  }
}
```

### Expiry

Choose one expiry mode on the settings page:

- **Never expires** — token is valid until you rotate it.
- **Expires after (hours)** — token expires N hours after creation (TTL).
- **Expires at (date/time)** — absolute expiry date/time.

Leave on "Never expires" for no expiry. After expiry, all connections are rejected until you set a new expiry or regenerate the token.

The token is stored in the OS credential manager (Windows Credential Manager), never in settings files, logs, or the installer bundle.

## Domain creation notes

- `kind` accepts plain strings: `"php"`, `"static"`, `"html"`, `"proxy"` (case-insensitive). Omitted or unrecognized values default to PHP.
- `root` defaults to `OLS_SITES_DIR` env var, then the `domains.sites_dir` setting, then the install's `sites` folder. It must be an absolute folder path.
- `php` version (e.g. `"8.3"`) is used only when `kind` is `php`.

## Safety

Destructive operations (remove domain, remove tunnel, restore snapshot, import sites, env edits) are flagged as destructive by the core and require user confirmation in the app UI before they execute. The MCP server reports this as a diagnostic rather than running silently.

## Where it lives

- Crate: `crates/ols-mcp/`
- Entry: `crates/ols-mcp/src/main.rs`
- Library: `crates/ols-mcp/src/lib.rs`
- Registered in workspace `Cargo.toml` alongside `ols-core`, `ols-helper`, `ols-cli`.
