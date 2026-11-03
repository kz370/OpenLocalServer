import { AlertTriangle, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { emit } from '@tauri-apps/api/event'

import { Button } from '@/components/ui/button'

export type TrayConfirmationAction = 'stop-all' | 'quit'

const CONFIRMATION_COPY: Record<
  TrayConfirmationAction,
  { title: string; message: string; confirmLabel: string }
> = {
  'stop-all': {
    title: 'Stop all?',
    message: 'Stop all running services, web servers, workers, and tunnels?',
    confirmLabel: 'Stop all',
  },
  quit: {
    title: 'Exit OpenLocalServer?',
    message: 'Exit OpenLocalServer and stop all running services, web servers, workers, and tunnels?',
    confirmLabel: 'Exit',
  },
}

export function TrayConfirmationWindow({ action }: { action: TrayConfirmationAction }) {
  const [error, setError] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const confirmButton = useRef<HTMLButtonElement>(null)
  const copy = CONFIRMATION_COPY[action]

  const respond = useCallback(async (confirmed: boolean) => {
    if (sending) return
    setSending(true)
    setError(null)
    try {
      await emit('ols:tray-confirmation-response', { action, confirmed })
    } catch (reason) {
      console.error('Could not send the tray confirmation response:', reason)
      setError('Could not send your response. Please try again or close this window.')
      setSending(false)
    }
  }, [action, sending])

  useEffect(() => {
    confirmButton.current?.focus()
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        void respond(false)
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [respond])

  return (
    <div className="flex h-screen w-screen bg-transparent text-foreground">
      <section className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-2xl">
        <header
          data-tauri-drag-region
          className="flex items-center justify-between gap-3 border-b border-border px-5 py-4"
        >
          <div data-tauri-drag-region className="flex min-w-0 items-center gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-destructive/10 text-destructive">
              <AlertTriangle className="size-4" />
            </span>
            <h1 data-tauri-drag-region className="text-base font-semibold">
              {copy.title}
            </h1>
          </div>
          <button
            type="button"
            className="rounded-md p-1 text-muted-foreground hover:bg-accent"
            aria-label="Close confirmation"
            disabled={sending}
            onClick={() => void respond(false)}
          >
            <X className="size-4" />
          </button>
        </header>
        <div className="flex-1 px-5 py-5">
          <p className="text-sm text-muted-foreground">{copy.message}</p>
          {error && (
            <p className="mt-3 text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="flex justify-end gap-2 border-t border-border px-5 py-3">
          <Button variant="ghost" disabled={sending} onClick={() => void respond(false)}>
            Cancel
          </Button>
          <Button
            ref={confirmButton}
            variant="destructive"
            disabled={sending}
            onClick={() => void respond(true)}
          >
            {copy.confirmLabel}
          </Button>
        </footer>
      </section>
    </div>
  )
}
