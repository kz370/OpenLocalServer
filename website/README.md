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
website/README.md       this file
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

## On release: bump three things in `index.html`

`scripts/upload-release.bat` publishes `OLS-<version>-setup.exe`,
`OLS-<version>-portable-win-x64.zip` and `OLS-<version>-SHA256SUMS.txt`.
`/releases/latest/download/<asset>` resolves against the newest release, so the
asset *name* is the only thing that needs changing, in four places:

- the nav **Download** button
- the hero **Download for Windows** button, and the `OLS-1.1.0-setup.exe`,
  portable-zip and checksum links in the note under it
- the **Verify a download** action in the open-source section
- the final call to action, and the copy in steps 1 and the "Download for Windows"
  buttons

The same version appears in the "OLS 1.1.0" fact badge and in the footer. A grep
for `1\.1\.0` on `index.html` finds every one.

## Rules for changing it

- **Content comes from the repository.** Feature claims, versions, commands and
  platform support are read out of `README.md`, `docs/`, `crates/ols-core/src/catalog.rs`
  and `CHANGELOG.md`. If the product changes, this page changes with it; nothing
  here is invented. Every claim on the page was checked against one of those.
- **Screenshots are real captures.** `assets/img/*.webp` are the app's own
  screenshots; keep them at the size they are, and set the `width` and `height`
  attributes to the file's real pixel size so nothing is stretched.
- **The palette is the app palette.** The tokens at the top of
  `assets/css/site.css` are copied from `ui/src/index.css`. Do not introduce a new
  hue; `docs/BRAND.md` §2.2 states the constraint.
- **No web fonts.** `docs/BRAND.md` §2.3: a display face in a developer tool reads
  as marketing.
- **`Diagnostic{problem,cause,fix}`** is how the product explains a failure, so it
  is how the site explains one too.
- **Links to the repository use `blob/master`**, which is the default branch. The
  app's own `ReleaseCards.tsx` opens `blob/main/docs/API.md`, which is wrong on this
  repository — a separate bug.
- **Do not name competing tools on the page.** The comparison is made in general
  terms ("other local stacks", "other common Windows stacks"), not by name.

Per `AGENTS.md` §1, a change here appends a line to `specs/runtime.md` under
`## Log`.
