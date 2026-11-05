import { RotateCcw, Save } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { discardDirtyForms, getDirtyForms, saveDirtyForms, useDirtyFormCount, useDirtyFormRevision, type DirtyFormScope } from '@/lib/dirtyForms'
import { cn } from '@/lib/utils'

export function DirtySaveBar({ scope }: { scope: DirtyFormScope }) {
  useDirtyFormRevision()
  const count = useDirtyFormCount(scope)
  const forms = getDirtyForms(scope)
  const error = forms.find((form) => form.error)?.error ?? null
  const busyFromForms = forms.some((form) => form.busy)
  const [saving, setSaving] = useState(false)
  const busy = busyFromForms || saving
  if (count === 0) return null

  return (
    <div
      className={cn(
        'dirty-save-bar z-[70] px-4 pb-4',
        scope === 'app'
          ? 'pointer-events-none fixed bottom-0 left-60 right-0 flex justify-center'
          : 'pointer-events-none absolute bottom-0 left-52 right-0 flex justify-center',
      )}
    >
      <div className={cn(
        'pointer-events-auto w-full max-w-3xl rounded-xl border border-border p-3 shadow-xl',
        // Solid on purpose: --card is translucent, and page content showed through the bar.
        'bg-[var(--dialog-surface,#ffffff)] dark:bg-[var(--dialog-surface,#0b1426)]',
      )}>
        {error && <ErrorCard error={error} />}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2 text-sm">
            <span className="size-2 shrink-0 rounded-full bg-warning" aria-hidden="true" />
            <span className="font-medium">Unsaved changes</span>
            {count > 1 && <span className="text-muted-foreground">in {count} forms</span>}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button variant="ghost" disabled={busy} onClick={() => discardDirtyForms(scope)}>
              <RotateCcw className="size-4" /> Discard
            </Button>
            <Button disabled={busy} onClick={() => { setSaving(true); void saveDirtyForms(scope).finally(() => setSaving(false)) }}>
              {busy ? <Spinner className="size-4" /> : <Save className="size-4" />} {busy ? 'Saving…' : 'Save'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
