# Confirm-before Switches Behind the Save Button — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three Settings → General switches (Confirm before exit, Confirm before Stop all, Automatically fix safe problems) commit only through the page's Save button instead of writing to config the moment they are clicked.

**Architecture:** The General section's `useDirtyForm` draft gains the three flags, so the existing `DirtySaveBar` picks up changes for free. The `set_setting` calls inside the three `onChange` handlers are deleted; `saveAll()` becomes the only writer. The section-load effect reads all three settings together with the editor values and syncs the complete draft into the saved baseline so the bar does not open already dirty.

**Tech Stack:** React 19 + TypeScript, Tauri IPC (`runCommand`), `node:test` for pure-logic unit tests, oxlint + `tsc -b` for gates.

**Spec:** `docs/superpowers/specs/2026-11-03-confirm-before-save-button-design.md`

## Global Constraints

- Setting keys and defaults are unchanged: `exit.confirmation.enabled` (default on), `tray.stop_all.confirmation.enabled` (default on), `diagnostics.auto_fix` (default on).
- No backend change, no new `CoreCommand`.
- Appearance, editor install, helper install/remove, service start/stop and root-folder add/remove stay instant-apply.
- Errors surface as `Diagnostic { problem, cause, fix }` — no raw strings in the UI.
- Any code change updates `specs/` in the same commit (repo mandate, `AGENTS.md` §1).
- `ui/package.json` `test` script is `node --experimental-strip-types --test src/**/*.test.ts`.

## Review Focus

- **A switch flipped then the page left without saving.** The tray must keep the old behaviour, and the existing `warnBeforeLeavingUnsavedChanges` prompt must appear. Covered by Task 2, Step 6.
- **Save bar showing changes on section open.** If the loaded values never reach the saved baseline, General opens dirty on every visit. Covered by Task 2, Step 4.
- **A discarded change surviving a remount.** Module-level persistent caches must be written on save, not on toggle. Covered by Task 2, Step 3 and Step 5.
- **Editor + switch changed together.** Saving must write the editor keys and the three settings in one pass, not one or the other. Covered by Task 1 tests and Task 2, Step 3.
- **Custom editor selected with a switch flipped.** `editorId === 'custom'` maps to `editor` blank and `editor.command` = the path; the settings writes must not disturb that mapping. Covered by Task 1 tests.

---

### Task 1: Pure write-builder for the General settings payload

**Files:**
- Create: `ui/src/lib/settingsWrite.ts`
- Test: `ui/src/lib/settingsWrite.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces: `type GeneralSettingsDraft`, `type SettingWrite = { key: string; value: string | boolean }`, `function generalSettingWrites(draft: GeneralSettingsDraft): SettingWrite[]` — later tasks call this from `saveAll()`.

- [ ] **Step 1: Write the failing test**

`ui/src/lib/settingsWrite.test.ts`:

```ts
import test from 'node:test'
import assert from 'node:assert/strict'
import { generalSettingWrites, type GeneralSettingsDraft } from './settingsWrite.ts'

const base: GeneralSettingsDraft = {
  editor: '',
  editorId: 'vscode',
  confirmExit: true,
  confirmStopAll: true,
  autoFixDiagnostics: true,
}

test('writes the three confirmation and diagnostics switches with their current values', () => {
  const writes = generalSettingWrites({ ...base, confirmExit: false, confirmStopAll: false, autoFixDiagnostics: false })
  assert.deepEqual(
    writes.filter((w) => w.key.endsWith('.enabled') || w.key === 'diagnostics.auto_fix'),
    [
      { key: 'exit.confirmation.enabled', value: false },
      { key: 'tray.stop_all.confirmation.enabled', value: false },
      { key: 'diagnostics.auto_fix', value: false },
    ],
  )
})

test('preset editor writes its id and blanks the custom command', () => {
  const writes = generalSettingWrites({ ...base, editorId: 'vscode', editor: 'ignored' })
  assert.deepEqual(writes.slice(0, 2), [
    { key: 'editor', value: 'vscode' },
    { key: 'editor.command', value: '' },
  ])
})

test('custom editor blanks the id and writes the path', () => {
  const writes = generalSettingWrites({ ...base, editorId: 'custom', editor: 'C:\\bin\\code.cmd' })
  assert.deepEqual(writes.slice(0, 2), [
    { key: 'editor', value: '' },
    { key: 'editor.command', value: 'C:\\bin\\code.cmd' },
  ])
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npm test`
Expected: FAIL — `Cannot find module './settingsWrite.ts'`.

- [ ] **Step 3: Implement `ui/src/lib/settingsWrite.ts`**

```ts
export type GeneralSettingsDraft = {
  editor: string
  editorId: string
  confirmExit: boolean
  confirmStopAll: boolean
  autoFixDiagnostics: boolean
}

export type SettingWrite = { key: string; value: string | boolean }

export function generalSettingWrites(draft: GeneralSettingsDraft): SettingWrite[] {
  return [
    { key: 'editor', value: draft.editorId === 'custom' ? '' : draft.editorId },
    { key: 'editor.command', value: draft.editorId === 'custom' ? draft.editor : '' },
    { key: 'exit.confirmation.enabled', value: draft.confirmExit },
    { key: 'tray.stop_all.confirmation.enabled', value: draft.confirmStopAll },
    { key: 'diagnostics.auto_fix', value: draft.autoFixDiagnostics },
  ]
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ui && npm test`
Expected: PASS, all three new tests green.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/settingsWrite.ts ui/src/lib/settingsWrite.test.ts
git commit -m "test(settings): add pure builder for the general settings write payload"
```

---

### Task 2: Defer the three switches to the Save button

**Files:**
- Modify: `ui/src/pages/Settings.tsx` — `SettingsDraft` type (line 44-48), draft `useMemo` (200-205), `onRestore` (212-215), `saveAll` (419-450), general load effect (250-282), delete the auto-fix load effect (352-361), three `SwitchRow`s (825-874).

**Interfaces:**
- Consumes: `generalSettingWrites`, `GeneralSettingsDraft`, `SettingWrite` from Task 1.
- Produces: nothing other tasks import.

- [ ] **Step 1: Widen the General draft type**

`ui/src/pages/Settings.tsx` line 45 becomes:

```ts
  | { section: 'general'; editor: string; editorId: string; confirmExit: boolean; confirmStopAll: boolean; autoFixDiagnostics: boolean }
```

- [ ] **Step 2: Put the three flags in the draft and restore them on Discard**

In the `settingsDraft` `useMemo` (line 200-205), the fallthrough branch becomes:

```ts
    return { section: 'general', editor, editorId, confirmExit, confirmStopAll, autoFixDiagnostics }
```

and its dependency array gains `autoFixDiagnostics, confirmExit, confirmStopAll`.

In `onRestore` (line 213-215), the general branch becomes:

```ts
      if (draft.section === 'general') {
        setEditor(draft.editor)
        setEditorId(draft.editorId)
        setConfirmExit(draft.confirmExit)
        setConfirmStopAll(draft.confirmStopAll)
        setAutoFixDiagnostics(draft.autoFixDiagnostics)
      }
```

- [ ] **Step 3: Make `saveAll()` the only writer**

Replace the editor writes at the top of `saveAll()` (lines 427-432) with the shared builder, and add the cache writes for the diagnostics flag:

```ts
    for (const { key, value } of generalSettingWrites({ editor, editorId, confirmExit, confirmStopAll, autoFixDiagnostics })) {
      await runCommand({ type: 'set_setting', key, value })
    }
    editorCache.set(editorId === 'custom' ? editor : '')
    editorIdCache.set(editorId)
    autoFixDiagnosticsCache.set(autoFixDiagnostics)
```

Import at the top of the file, next to the other `@/lib` imports:

```ts
import { generalSettingWrites } from '@/lib/settingsWrite'
```

- [ ] **Step 4: Load all three settings in one pass and sync the baseline**

Replace the two `get_setting` calls at the start of the general load effect (lines 250-261) and the editor `Promise.all` (lines 274-281) with a single load, and delete the separate auto-fix effect (lines 352-361):

```ts
    void Promise.all([
      runCommand({ type: 'get_setting', key: 'exit.confirmation.enabled' }),
      runCommand({ type: 'get_setting', key: 'tray.stop_all.confirmation.enabled' }),
      runCommand({ type: 'get_setting', key: 'diagnostics.auto_fix' }),
      getString('editor'),
      getString('editor.command'),
    ]).then(([exit, stopAll, autoFix, id, cmd]) => {
      const exitVal = exit.type === 'setting' ? exit.value !== false : true
      const stopVal = stopAll.type === 'setting' ? stopAll.value !== false : true
      const fixVal = autoFix.type === 'setting' ? autoFix.value !== false : true
      setConfirmExit(exitVal)
      setConfirmStopAll(stopVal)
      setAutoFixDiagnostics(fixVal)
      autoFixDiagnosticsCache.set(fixVal)
      setEditor(cmd)
      editorCache.set(cmd)
      const resolvedId = cmd ? 'custom' : id || 'vscode'
      setEditorId(resolvedId)
      editorIdCache.set(resolvedId)
      settingsForm.syncSaved({ section: 'general', editor: cmd, editorId: resolvedId, confirmExit: exitVal, confirmStopAll: stopVal, autoFixDiagnostics: fixVal }, 'general')
    })
```

The `list_editors` and `get_helper_service` calls in that effect stay as they are. `syncSaved` is called without `force` so a toggle made while loading still wins.

- [ ] **Step 5: Stop the switches from writing on change**

The three `SwitchRow`s (lines 825-874) lose their `run(...)`/`set_setting` bodies:

```tsx
                  <SwitchRow
                    checked={confirmExit}
                    onChange={setConfirmExit}
                    title="Confirm before exit"
                    hint="Ask before exiting OpenLocalServer. This does not affect the Stop all confirmation. Saved with the Save button below."
                  />
                  <SwitchRow
                    checked={confirmStopAll}
                    onChange={setConfirmStopAll}
                    title="Confirm before Stop all"
                    hint="Ask before Stop all from the tray. This does not affect the Exit confirmation. Saved with the Save button below."
                  />
```

and, in the Diagnostics card:

```tsx
                  <SwitchRow
                    checked={autoFixDiagnostics}
                    onChange={setAutoFixDiagnostics}
                    title="Automatically fix safe problems"
                    hint="Runs at startup and every five minutes. Fixes that may replace or remove data always ask first. Saved with the Save button below."
                  />
```

- [ ] **Step 6: Verify the guard and the gates**

Run: `cd ui && npm run lint && npm run build`
Expected: lint clean (no unused `run` import/identifier left behind — `run` is still used by the helper and editor actions), build exit 0.

Manual check, all four must hold:
1. Open Settings → General: no Save bar.
2. Flip **Confirm before exit**: Save bar appears; **Discard** puts the switch back.
3. Flip it again, press **Save**: bar disappears; reload the page — still flipped.
4. Flip it, then click another sidebar section: the "unsaved changes" prompt appears.

- [ ] **Step 7: Commit**

```bash
git add ui/src/pages/Settings.tsx
git commit -m "fix(settings): save the confirm-before and auto-fix switches with the Save button"
```

---

### Task 3: Sync the specs

**Files:**
- Modify: `specs/catalog.txt` — the `SettingsPage` row (line 147).
- Modify: `specs/full_documentation.md` — line 71, the `DirtySaveBar` paragraph.
- Modify: `specs/runtime.md` — append one line under `## Log`; keep `Files Processed` equal to `Files to Process`.

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: Update the `SettingsPage` catalog row**

In the `SettingsPage` sentence, replace the claim that the confirmation toggles are instant-apply with the fact that the General card's three switches — Confirm before exit, Confirm before Stop all, and Diagnostics auto-fix — are part of the deferred General draft and commit only through the bottom Save bar, while appearance and service actions remain instant.

- [ ] **Step 2: Update the `DirtySaveBar` paragraph**

In `specs/full_documentation.md` line 71, after "Instant-apply toggles, service actions, and explicit actions such as “Back up now” are not registered.", add that the General card's confirmation and diagnostics switches are registered (they are no longer instant-apply).

- [ ] **Step 3: Append the runtime log line**

Add one line to `specs/runtime.md` under `## Log` in the existing format:

```text
- [2026-11-03] fix(settings): General → Confirm before exit, Confirm before Stop all, and Diagnostics auto-fix now commit only through the Settings Save bar instead of writing on toggle. Added `ui/src/lib/settingsWrite.ts` (pure write-payload builder, unit-tested) and folded the three flags into the General `useDirtyForm` draft; `saveAll()` is the only writer and the load effect syncs the full draft into the saved baseline. Updated catalog.txt, full_documentation.md, and runtime.md. Files Processed remains <N>, matching Files to Process.
```

Replace `<N>` with the current number from the file header.

- [ ] **Step 4: Commit**

```bash
git add specs/catalog.txt specs/full_documentation.md specs/runtime.md
git commit -m "docs(specs): record the deferred General switches"
```
