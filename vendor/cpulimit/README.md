# cpulimit (vendored)

`cpulimit.exe` is the CPU limiter the app shells out to for the Resources → CPU cap (§129).
It is a third-party utility and is **vendored, not built here**: it comes from
[kz370/win-utils](https://github.com/kz370/win-utils/tree/main/cpulimiter), `cpulimiter/cpulimit.cpp`,
MIT licensed, and works by putting the target program in a Windows job object with a hard CPU
rate cap (`JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION` with `HARD_CAP | ENABLE`).

## Provenance

| | |
|---|---|
| Source | `https://raw.githubusercontent.com/kz370/win-utils/main/cpulimiter/cpulimit.exe` |
| Retrieved | 2026-09-29 |
| Size | 308224 bytes |
| SHA-256 | `A54EACA4BD1BCCDCBAA69E31BFFE0EEDC2019E9EFCDE7BDE06F31542B7746DA3` |

Upstream publishes no release assets and no checksum sidecar for this binary, so the hash above
is **self-pinned**: taken from the retrieved file and checked on every build, the same approach
used for nginx in `catalog.rs`. A build fails rather than shipping a binary nobody vouched for.

Verify after any re-download:

```powershell
Get-FileHash vendor\cpulimit\cpulimit.exe -Algorithm SHA256
```

If the upstream `main` branch moves, update the hash in the same commit as the binary and say so
in `specs/runtime.md`. The app does not download this at runtime; it only looks for it on disk.

## Where it goes

`installer/open-local-server.iss` installs it next to `OLS.exe`, and
`scripts/build-installer.bat` stages it beside the portable executable. The app finds it there
first (`resources.rs::find_cpu_limiter` checks the current exe's directory before anything else),
so a default install needs no configuration. A portable copy, a dev build and an install all
resolve it the same way.
