# The local HTTP API

Off by default. Turn it on in Settings → Local API, or `ols api enable`.

- Listens on `127.0.0.1` only (default port 7420).
- Every request needs `Authorization: Bearer <token>`. The token is shown once when you make it; only its hash is
  stored. A new token revokes the old one.
- Requests with an `Origin` header (web pages) or a `Host` other than `127.0.0.1:<port>` / `localhost:<port>` are refused.
- Read-only mode allows queries (`list_*`, `get_*`, `check_*`, diagnostics, Git status and log, `read_log`).
  Operate mode adds: start/stop/restart services, workers and the web server, apply/validate the web config, install
  runtimes, register projects, run a schedule, create a snapshot, back up a database, run diagnostics.
- Never available: running programs, terminals, settings, secrets, credentials, tunnel tokens, plugins, updates,
  the Explorer menu, and this API's own settings.

```
curl -H "Authorization: Bearer ols_..." http://127.0.0.1:7420/v1/ping
curl -H "Authorization: Bearer ols_..." -H "Content-Type: application/json" \
     -d '{"type":"list_services"}' http://127.0.0.1:7420/v1/command
```

The body is a `CoreCommand` (the same JSON the app and CLI use). A success is `{"ok":true,"result":{...}}` and a
failure is `{"ok":false,"error":{"problem","cause","fix"}}` with an HTTP 4xx status.
