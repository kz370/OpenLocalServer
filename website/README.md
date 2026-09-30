# OLS project site

The public website. **`index.html` in the repository root is the site** — that is
what GitHub Pages publishes — and everything it needs lives in `website/assets/`.
No build step, no dependencies: it is a static page that any file server can host.

```text
index.html              the site (asset paths point into website/assets/)
website/assets/css/     site.css
website/assets/js/      site.js
website/assets/img/     the app's own screenshots, logo and favicon
website/index.html      redirect to ../index.html, so the old URL still works
```

## Run it locally

Serve the **repository root**, not `website/` — the paths in `index.html` are
relative to the root.

```powershell
python -m http.server 4173
# http://127.0.0.1:4173
```

Opening `index.html` as a `file://` path also works; there is nothing to fetch
across origins.

## Rules for changing it

- **Content comes from the repository.** Feature claims, versions, commands and
  platform support are read out of `README.md`, `docs/` and `CHANGELOG.md`. If the
  product changes, this page changes with it; nothing here is invented.
- **Screenshots are real captures.** `assets/img/*.webp` are copies of the images in
  `assets/`. Re-copy them when the UI changes:

  ```powershell
  foreach ($f in 'dashboard','sites','runtimes','version-manager','databases','webserver','tunnels') {
      Copy-Item "assets\$f.webp" "website\assets\img\$f.webp" -Force
  }
  Copy-Item 'data\home\logo-dark.svg' 'website\assets\img\logo.svg' -Force
  ```

- **The palette is the app palette.** The tokens at the top of
  `assets/css/site.css` are copied from `ui/src/index.css`. Do not introduce a new
  hue; `docs/BRAND.md` §2.2 states the constraint.
- **No web fonts.** `docs/BRAND.md` §2.3: a display face in a developer tool reads
  as marketing.
- **`Diagnostic{problem,cause,fix}`** is how the product explains a failure, so it
  is how the site explains one too.
- **Links to the repository use `blob/master`**, which is the default branch. The
  app itself opens `blob/main/docs/API.md` from `ReleaseCards.tsx` — that link is
  wrong on this repository, which is a separate bug.

Per `AGENTS.md` §1, a change here appends a line to `specs/runtime.md` under
`## Log`.
