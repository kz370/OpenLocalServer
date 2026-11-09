import { createPortal } from 'react-dom'

import type { Diagnostic } from '@/core'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { CORNER_ERRORS_ID, useOverlayHost } from '@/lib/overlayHost'

/**
 * §112 / §115: every failure is shown as Problem / Cause / Fix.
 *
 * `overlay` cards are portaled into the bottom-right corner stack (see `OverlayHosts`), above
 * the toasts; several overlay cards stack upward on their own. `overlayOffset` is kept for
 * callers but no longer needed: the stack orders them.
 */
export function ErrorCard({ error, onDismiss, overlay = false }: { error: Diagnostic | null; onDismiss?: () => void; overlay?: boolean; overlayOffset?: number }) {
  const host = useOverlayHost(CORNER_ERRORS_ID)
  if (!error) return null
  const card = (
    <Card className={overlay ? 'pointer-events-auto max-h-[45vh] w-full overflow-auto border-destructive/40 bg-destructive/5 shadow-xl' : 'border-destructive/40 bg-destructive/5'}>
      <CardHeader className="flex-row items-start justify-between space-y-0">
        <CardTitle className="text-destructive">{error.problem}</CardTitle>
        {onDismiss && (
          <button onClick={onDismiss} className="cursor-pointer text-xs text-muted-foreground hover:text-foreground">
            Dismiss
          </button>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-1">
        <pre className="whitespace-pre-wrap font-sans text-sm">{error.cause}</pre>
        {error.fix && <p className="text-sm text-success">{error.fix}</p>}
      </CardContent>
    </Card>
  )
  if (!overlay) return card
  if (!host) return null
  // No corner stack mounted (tests, early boot): place it in the window corner directly.
  if (host === document.body) {
    return createPortal(
      <div className="pointer-events-none fixed bottom-4 right-4 z-[9999] w-[min(30rem,calc(100vw-2rem))]">{card}</div>,
      host,
    )
  }
  return createPortal(card, host)
}
