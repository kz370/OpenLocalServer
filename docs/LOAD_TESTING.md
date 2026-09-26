# Load testing with k6

Project tab **Load testing** (site dialog), or `ols test load [project] [script] [--site host] [--public]`.

- Scripts live in `.openlocalserver/k6/*.js` and are committed with the project. **smoke**, **load** and **spike** write a first script from the site.
- k6 comes from Runtimes (`ols runtime install k6`, SHA-256 verified) or an existing install on PATH.
- A run targets one of the project's own sites and gets `BASE_URL` set. A script that names any other host is refused.
- A tunnel's public address needs explicit confirmation (`--public` on the CLI): it sends real traffic through the provider.
- Virtual-user numbers in a script must stay under Settings → Resources → Load-test users (default 200). k6 runs at below-normal priority and can be stopped.
- Live numbers (requests/s, p50/p95/p99, error rate, users, checks) come from k6's JSON output; finished runs are kept and can be compared.
- Exit code: 0 passed, non-zero when a threshold failed or the run errored, so CI can use it.

Limits: the host and user-count checks read the script text, so a URL built at run time is not seen. They are guard rails, not a sandbox.
There is no CPU cap (Windows has no simple one) and no Quick Command entry yet. Not run against a real Laravel site in the app.
