import type { ReactNode } from 'react'

import { CORNER_ERRORS_ID, OVERLAY_ROOT_ID } from '@/lib/overlayHost'

/**
 * Portal hosts for dialogs and corner notifications. Mount once, inside the app root.
 * `toasts` is rendered into the bottom-right stack below the overlay ErrorCards, so the two
 * never cover each other.
 */
export function OverlayHosts({ toasts }: { toasts: ReactNode }) {
  return (
    <>
      <div id={OVERLAY_ROOT_ID} />
      {/* Column-reverse: first child (toasts) sits at the bottom, errors stack above it. */}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[9999] flex w-[min(30rem,calc(100vw-2rem))] flex-col-reverse items-end gap-2.5">
        {toasts}
        <div id={CORNER_ERRORS_ID} className="flex w-full flex-col-reverse items-end gap-2.5" />
      </div>
    </>
  )
}
