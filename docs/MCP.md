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
      "command": "C:\\OpenLocalServer\\ols-mcp.exe"
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

## Safety

Destructive operations (remove domain, remove tunnel, restore snapshot, import sites, env edits) are flagged as destructive by the core and require user confirmation in the app UI before they execute. The MCP server reports this as a diagnostic rather than running silently.

## Where it lives

- Crate: `crates/ols-mcp/`
- Entry: `crates/ols-mcp/src/main.rs`
- Library: `crates/ols-mcp/src/lib.rs`
- Registered in workspace `Cargo.toml` alongside `ols-core`, `ols-helper`, `ols-cli`.
