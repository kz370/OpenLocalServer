# Load testing with k6

Project tab **Load testing** (site dialog), or `ols test load [project] [script] [--profile smoke|load|stress|spike|soak|<yours>] [--site host] [--public]`.

- A test is set up from a form: users at peak, stages (seconds and users), the paths to visit (method and path, as many as you need), the pause between rounds, and pass criteria (p95, p99, failed-request percent). Ready-made tests: **Smoke**, **Load**, **Stress**, **Spike**, **Soak**. Change one and **Save as my test** to keep your own; your tests are stored in `loadtest_profiles.json` in the data folder.
- **Headers, tokens and bodies:** add headers that go with every request (`Authorization: Bearer {{TOKEN}}`), and a body for POST, PUT, PATCH and DELETE paths. `{{NAME}}` refers to a variable. Variable values are handed to k6 when the test runs (`--env`) and are **never written into the script**; a variable marked secret is kept in the system keyring, not in the saved test. Custom scripts read them as `__ENV.NAME`. CLI: `--var NAME=VALUE`.
- Running a test writes its script to `.openlocalserver/k6/<name>.js` (committed with the project). You can also add your own k6 scripts there, start a new one from the form (**New script**), and edit the code in the app; they run against the same sites with the same guard rails.
- k6 comes from Runtimes (`ols runtime install k6`, SHA-256 verified) or an existing install on PATH.
- A run targets one of the project's own sites and gets `BASE_URL` set. A script that names any other host is refused.
- A tunnel's public address needs explicit confirmation (`--public` on the CLI): it sends real traffic through the provider.
- Virtual-user numbers in a script must stay under Settings → Resources → Load-test users (default 200). k6 runs at below-normal priority and can be stopped.
- Live numbers (requests/s, p50/p95/p99, error rate, users, checks) come from k6's JSON output; finished runs are kept and can be compared.
- Exit code: 0 passed, non-zero when a threshold failed or the run errored, so CI can use it.

Limits: headers apply to every request (no per-path headers yet); the host and user-count checks read the script text, so a URL built at run time is not seen. They are guard rails, not a sandbox.
There is no CPU cap (Windows has no simple one) and no Quick Command entry yet. Not run against a real Laravel site in the app.
