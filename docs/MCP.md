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

The cleanest way is to put the token **in the MCP server config itself** (next to the `command`), so every `tools/call` is authenticated automatically — you never edit individual request JSON.

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

Get the token from **OLS Settings → MCP / AI tools** (enable authentication, then generate or paste a token). Rotating the token there invalidates all connected MCP hosts on the next request; just update the config with the new token.

When authentication is enabled, every `tools/call` request (except `initialize`, `ping`, `resources/list`, `prompts/list`) must carry a bearer token:

- **Header form** (preferred, MCP 2025-06-18): send `params._meta.authorization` = `Bearer <token>`
- **Fallback form**: send `params.arguments.token` = `<token>`

The token is stored in the OS credential manager (Windows Credential Manager), never in settings files, logs, or the installer bundle.

### Token rotation

Open OLS Settings → MCP / AI tools and enter a new token, or leave blank to auto-generate one. Changing the token invalidates all connected MCP hosts on the next request; reconfigure each host with the new token. No reinstall needed.

### Expiration

Set a token lifetime (hours) on the settings page. `0` means never expires. After expiry, all authenticated requests are rejected until you rotate the token.

### Example (Claude Desktop with auth enabled)

```json
{
  "mcpServers": {
    "ols": {
      "command": "C:\\OLS\\ols-mcp.exe"
    }
  }
}
```

Then configure your MCP host to send the bearer token with each `tools/call`. If your host supports `_meta.authorization`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "_meta": { "authorization": "Bearer <your-token>" },
    "name": "ols_list_domains",
    "arguments": {}
  }
}
```

Otherwise use the `arguments.token` fallback:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "ols_list_domains",
    "arguments": { "token": "<your-token>" }
  }
}
```

### Domain creation notes

- `kind` accepts plain strings: `"php"`, `"static"`, `"proxy"` (case-insensitive). Omitted or unrecognized values default to PHP.
- `root` defaults to the installation's `sites` folder if not provided. It must be an absolute folder path.
- `php` version (e.g. `"8.3"`) is used only when `kind` is `php`.

## Safety

Destructive operations (remove domain, remove tunnel, restore snapshot, import sites, env edits) are flagged as destructive by the core and require user confirmation in the app UI before they execute. The MCP server reports this as a diagnostic rather than running silently.

## Where it lives

- Crate: `crates/ols-mcp/`
- Entry: `crates/ols-mcp/src/main.rs`
- Library: `crates/ols-mcp/src/lib.rs`
- Registered in workspace `Cargo.toml` alongside `ols-core`, `ols-helper`, `ols-cli`.

