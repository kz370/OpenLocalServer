# Design: put the "Confirm before…" switches behind the Save button

Date: 2026-11-03
Size: small change

## Intent

Three switches in **Settings → General** write to config the moment they are
clicked:

- Confirm before exit
- Confirm before Stop all
- Automatically fix safe problems

They call `set_setting` directly from `onChange`, so they sit outside the page's
shared unsaved-changes system. A mis-click is written immediately, cannot be
undone, and the bottom **Save / Discard** bar never reacts.

They should behave like the rest of the page: flipping a switch marks the page as
having unsaved changes, and **Save** is what actually writes the value.

## What changes

All in `ui/src/pages/Settings.tsx`.

1. The General draft (`settingsDraft`) gains `confirmExit`, `confirmStopAll`,
   `autoFixDiagnostics`.
2. The three `set_setting` calls inside the `onChange` handlers are removed; the
   handlers only call the local setter.
3. `saveAll()` writes the three keys (`exit.confirmation.enabled`,
   `tray.stop_all.confirmation.enabled`, `diagnostics.auto_fix`).
4. After the section-load effects read the current values, they call
   `settingsForm.syncSaved(...)` for `'general'` so the saved baseline matches what
   is on screen and the bar does not open already dirty.
5. The persistent module caches for these values are updated on **save**, not on
   toggle, so a discarded change does not survive a remount.

No backend change. No new `CoreCommand`. Setting keys and defaults are unchanged.

## Non-goals

- Other instant-apply controls stay instant: appearance, editor install, helper
  install/remove, service start/stop, root-folder add/remove.
- No change to what the tray does with these preferences, only to when they are
  persisted.

## Anti-goals

- A switch that looks flipped but was never saved when you leave the page. The
  existing `warnBeforeLeavingUnsavedChanges` guard must still catch it.
- The Save bar appearing on page open because the baseline was never synced.

## Consequence to accept

Turning off a confirmation no longer takes effect until you press Save. That is
the requested behaviour, but it means the tray keeps asking until the change is
saved.

## Verification

- `cd ui && npm run lint && npm run build`
- Manual: flip each switch → bar appears; **Discard** reverts the switch; **Save**
  persists it across a reload; Ctrl+S saves.

## Specs to update (repo mandate)

- `specs/catalog.txt` — `SettingsPage` entry currently says instant-apply toggles
  remain outside the bar; these three no longer do.
- `specs/full_documentation.md` — Settings → General description.
- `specs/runtime.md` — append a log line, keep `Files Processed` equal to
  `Files to Process`.
