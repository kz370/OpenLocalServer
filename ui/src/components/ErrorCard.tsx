import type { Diagnostic } from '@/core'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

/** §112 / §115: every failure is shown as Problem / Cause / Fix. */
export function ErrorCard({ error, onDismiss, overlay = false, overlayOffset = 0 }: { error: Diagnostic | null; onDismiss?: () => void; overlay?: boolean; overlayOffset?: number }) {
  if (!error) return null
  return (
    <Card style={overlay ? { top: `${3.5 + overlayOffset * 9}rem` } : undefined} className={overlay ? 'fixed right-6 z-[90] max-h-[45vh] w-[min(30rem,calc(100vw-3rem))] overflow-auto border-destructive/40 bg-destructive/5 shadow-xl' : 'border-destructive/40 bg-destructive/5'}>
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
}

export function asDiagnostic(err: unknown): Diagnostic {
  if (err && typeof err === 'object' && 'problem' in err) return err as Diagnostic
  return { problem: 'Something went wrong.', cause: String(err), fix: null }
}
