# OLS project site

The public website: `index.html`, one stylesheet, one script, and the app's own
screenshots and logo. No build step and no dependencies — it is a static folder
that any file server or GitHub Pages can host.

## Run it locally

```powershell
cd website
python -m http.server 4173
# http://127.0.0.1:4173
```

## Rules for changing it

- **Content comes from the repository.** Feature claims, versions, commands and
  platform support are read out of `README.md`, `docs/` and `CHANGELOG.md`. If the
  product changes, this page changes with it; nothing here is invented.
- **Screenshots are real captures.** `assets/img/*.webp` are copies of the images
  in `assets/`. Re-copy them when the UI changes:

  ```powershell
  foreach ($f in 'dashboard','sites','runtimes','version-manager','databases','webserver','tunnels') {
      Copy-Item "..\assets\$f.webp" "assets\img\$f.webp" -Force
  }
  Copy-Item '..\data\home\logo-dark.svg' 'assets\img\logo.svg' -Force
  ```

- **The palette is the app palette.** The tokens at the top of
  `assets/css/site.css` are copied from `ui/src/index.css`. Do not introduce a new
  hue; `docs/BRAND.md` §2.2 states the constraint.
- **No web fonts.** `docs/BRAND.md` §2.3: a display face in a developer tool reads
  as marketing.
- **`Diagnostics{problem,cause,fix}`** is how the product explains a failure, so it
  is how the site explains one too.

Per `AGENTS.md` §1, a change here appends a line to `specs/runtime.md` under
`## Log`.
