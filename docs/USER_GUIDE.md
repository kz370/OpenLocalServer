# OLS user guide

## First run
1. Open **Runtimes** and install a PHP version (and Node or Python if you use them). Downloads are checked against a SHA-256.
2. Open **Sites**, add a project folder (or a folder of projects) and give it a domain. `.test` names work without editing the hosts file.
3. **Web server**: apply the config, and trust the local certificate authority once so HTTPS sites don't warn.
4. Databases, mail and caches are on **Services** and **Databases**; Mailpit catches outgoing mail.

## Everyday work
- **Sites** is one table of sites and projects. A row's Settings button opens the site's web config, servers, terminal,
  `.env`, Git, workers, snapshots, repair and load testing.
- **Commands** lists what a project offers (Artisan, Composer, npm scripts).
- **Quick Apps** creates a new project from a recipe (Laravel, WordPress, Vite, Django…). Each recipe shows what it will run first.
- **Profiles** and `ols setup` rebuild an environment from `.openlocalserver/environment.yaml`.
- **Tunnels** make a site public on purpose; the first start asks for confirmation.
- **Plugins** add runtimes (Go, Bun, Java, .NET), Quick Apps, detections and health checks. See [PLUGINS.md](PLUGINS.md).
- **Ctrl+K** searches everything and runs commands.

## The command line
`ols status`, `ols start`, `ols stop`, `ols doctor`, `ols repair`, `ols setup`, `ols project …`, `ols service …`,
`ols plugin …`, `ols catalog …`, `ols test load`, `ols ai …`, `ols api …`, `ols update …`, `ols support-bundle`.
`ols <command> --help` explains each. The CLI talks to the app when it is open and starts a background core otherwise.

## When something is wrong
Run **Doctor** (command palette) or `ols doctor`. It lists problems as Problem, Cause, Fix. **Settings → Windows
integration, support and privacy → Save bundle** writes a redacted zip for a bug report.

## Privacy
OLS sends nothing anywhere on its own: no telemetry, no analytics. It contacts the internet only when you
install or update something, refresh a catalog, check for updates, start a tunnel, or use an AI provider you configured.

## AI assistant (optional)
Off by default. **Settings → AI assistant** turns it on and points it at a model you already have: LM Studio or Ollama
on this computer, or Hugging Face, OpenRouter or another OpenAI-compatible server with your own key. Once on, an
**Explain** / **Ask AI** button appears on diagnostics, failed setups, logs, web configs, tunnel traffic, the Git
commit box and the manifest editor, and the command palette gets **Ask the AI assistant…**. Each request shows where it
goes (on this computer, or off it) and can show the exact text first, with secrets hidden. Anything it proposes is a
list of steps you tick and approve; it never runs a command by itself. Details: [AI_ASSISTANT.md](AI_ASSISTANT.md).

## Updates
Settings → Updates checks a signed manifest; the installer is verified before it can be started. A build only trusts
updates when an update public key is configured.

More: [API.md](API.md), [PLUGINS.md](PLUGINS.md), [SECURITY_REVIEW.md](SECURITY_REVIEW.md), [LOAD_TESTING.md](LOAD_TESTING.md).
