# Plugins and catalogs

A plugin adds things to OLS without changing the app. This version supports **declarative plugins**:
data checked against a schema, with no code in them. Code plugins (`kind: wasm`) are recognised but refused,
because there is no sandbox for them yet.

## What a plugin can add

| Contribution | Needs permission | What it does |
|---|---|---|
| `runtimes` | `download` | Runtimes on the Runtimes page (download, SHA-256 check, install, like the built-in ones) |
| `quick_apps` | `quick_apps` | Quick App recipes from a folder inside the plugin. Each recipe still shows what it will run |
| `detections` | `read_projects` | "This folder is a Go project": marker files looked for in a project (nothing is changed) |
| `health_checks` | `network` | A TCP or HTTP check that appears in the environment health list |

A contribution without its permission makes the manifest invalid. A plugin stays **off** until you approve
exactly the permissions it lists; if the manifest changes afterwards (for example, a reinstall), the approval is
void and you are asked again.

Built-in plugins ship inside the app and are off until you turn them on: **Go**, **Bun**, **Java (Temurin
JDK 21)** and **.NET SDK**. Ruby is not included: RubyInstaller ships `.7z` archives and the Package Manager
reads zip only.

## `plugin.yaml`

```yaml
id: my-plugin            # lowercase letters, digits, - and _
name: My plugin
version: 1.0.0
description: What it is for.
author: You
kind: declarative
permissions: [download, quick_apps, read_projects, network]
contributes:
  runtimes:
    - id: mytool
      name: My tool
      version: "1.2.3"
      url: https://example.com/mytool-1.2.3-windows.zip   # HTTPS only
      sha256: <64 hex characters>                          # required
      archive_root: mytool-1.2.3                           # folder inside the zip to strip ("" for none)
      binary: bin/mytool.exe                               # relative to the install folder
      probe: { exe: mytool.exe, arg: --version }           # optional: find an existing install
  quick_apps: quick-apps            # folder of *.yaml recipes inside the plugin
  detections:
    - id: mytool-project
      name: My tool
      markers: [mytool.toml, "*.mytool"]   # any of these; `all: [...]` requires every one
  health_checks:
    - id: db
      name: Local database
      kind: tcp                     # tcp (host:port) or http (http://host[:port]/path)
      target: 127.0.0.1:5432
      hint: Start it from the Services page.
```

Paths (`binary`, `archive_root`, `quick_apps`, markers) must stay inside their folder: no `..`, no drive letters.

## Installing

- The Plugins page: **From folder** or **From .zip**.
- CLI: `olsc plugin install <folder|zip>`, then `olsc plugin enable <id>` (shows the permissions and asks).
- A zip is unpacked with every path checked (no writing outside the plugin folder), up to 500 files and 50 MB.
- Plugins live in `data/plugins/<id>/`.

## Signed catalogs

A catalog is a JSON file published with a [minisign](https://jedisct1.github.io/minisign/) signature at the
same address plus `.minisig`:

```json
{
  "name": "Acme catalog",
  "runtimes": [ { "id": "acme-tool", "name": "Acme tool", "version": "1.0.0", "url": "https://...", "sha256": "...", "binary": "acme.exe" } ],
  "plugins":  [ { "id": "acme", "name": "Acme", "version": "1.0.0", "description": "...", "url": "https://.../acme.zip", "sha256": "..." } ],
  "quick_app_sources": [ { "name": "Acme recipes", "url": "https://github.com/acme/recipes", "description": "..." } ]
}
```

- You add a catalog with the publisher's **public key**. That key decides who may publish to it.
- A file whose signature doesn't verify is never read; the previous good copy stays.
- The downloaded copy is verified again every time it is loaded, so editing it on disk does nothing.
- Catalog runtimes carry a SHA-256 like any other. Catalog plugins are checked against their listed SHA-256 and
  install switched off, like a local plugin.
- Quick App sources in a catalog are pointers only: importing one goes through the normal untrusted-until-approved flow.
- Sign a catalog with `minisign -Sm catalog.json`. `https://` addresses and local files (a share) both work.

CLI: `olsc catalog add|list|refresh|remove|install`.

## Not done yet

- Code plugins (WASM sandbox) for tunnel providers, diagnostics and UI panels.
- Plugins that add a database or service definition, and external tool definitions. Custom services already
  cover running your own program.
- A built-in default catalog: there is no official catalog or key yet, so none is preconfigured.
