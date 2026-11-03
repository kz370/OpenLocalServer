import { AlertTriangle, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'

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

export function TrayConfirmationWindow() {
  const [request, setRequest] = useState<{ action: TrayConfirmationAction; id: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [sending, setSending] = useState(false)
  const [animationId, setAnimationId] = useState(0)
  const confirmButton = useRef<HTMLButtonElement>(null)
  const requestId = useRef(0)
  const action = request?.action

  const respond = useCallback(async (confirmed: boolean) => {
    if (!action || sending) return
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
    let unlisten: (() => void) | undefined
    void listen<string>('ols:tray-confirmation-request', (event) => {
      if (event.payload !== 'stop-all' && event.payload !== 'quit') {
        console.error('Unknown tray confirmation request:', event.payload)
        return
      }
      requestId.current += 1
      setRequest({ action: event.payload, id: requestId.current })
      setSending(false)
      setError(null)
      setAnimationId(0)
    }).then((cleanup) => {
      unlisten = cleanup
      void emit('ols:tray-confirmation-ui-ready').catch((reason: unknown) => {
        console.error('Could not report that the tray confirmation is ready:', reason)
      })
    }).catch((reason: unknown) => {
      console.error('Could not listen for tray confirmation requests:', reason)
    })
    return () => unlisten?.()
  }, [])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen('ols:tray-confirmation-animate', () => {
      setAnimationId((current) => current + 1)
    }).then((cleanup) => {
      unlisten = cleanup
    }).catch((reason: unknown) => {
      console.error('Could not listen for the tray confirmation animation:', reason)
    })
    return () => unlisten?.()
  }, [])

  useEffect(() => {
    if (!request) return
    confirmButton.current?.focus()
    void emit('ols:tray-confirmation-show-ready').catch((reason: unknown) => {
      console.error('Could not show the tray confirmation window:', reason)
      setError('Could not display this confirmation. Please try again.')
    })
  }, [request])

  useEffect(() => {
    if (!request) return
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        void respond(false)
      } else if (event.key === 'Enter') {
        event.preventDefault()
        void respond(true)
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [request, respond])

  if (!request || !action) return null
  const copy = CONFIRMATION_COPY[action]

  return (
    <div className="h-screen w-screen bg-transparent text-foreground">
      <section
        key={animationId}
        className={`${animationId > 0 ? 'tray-confirmation-enter' : 'tray-confirmation-pending'} relative isolate h-full w-full overflow-hidden rounded-[18px] text-foreground shadow-2xl`}
        style={{ background: 'var(--app-background, var(--background))' }}
      >
        <div className="relative p-[26px]">
          <header data-tauri-drag-region className="flex items-center gap-[15px]">
            <span
              data-tauri-drag-region
              className="flex size-[50px] shrink-0 items-center justify-center rounded-[14px] border border-primary/20 bg-primary/10 text-primary"
            >
              <AlertTriangle className="size-6" />
            </span>
            <div data-tauri-drag-region className="min-w-0 flex-1">
              <h1 data-tauri-drag-region className="truncate text-[18px] font-semibold">
                {copy.title}
              </h1>
              <p data-tauri-drag-region className="mt-0.5 text-xs text-muted-foreground">
                OpenLocalServer
              </p>
            </div>
            <button
              type="button"
              className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              aria-label="Cancel confirmation"
              disabled={sending}
              onClick={() => void respond(false)}
            >
              <X className="size-4" />
            </button>
          </header>
          <p className="mb-6 mt-5 text-sm leading-[21px] text-muted-foreground">{copy.message}</p>
          {error && (
            <p className="mb-4 text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <footer className="flex justify-end gap-2">
            <Button variant="secondary" disabled={sending} onClick={() => void respond(false)}>
              Cancel <kbd className="ml-1 rounded bg-foreground/10 px-1.5 py-0.5 text-[10px]">Esc</kbd>
            </Button>
            <Button
              ref={confirmButton}
              variant="destructive"
              disabled={sending}
              onClick={() => void respond(true)}
            >
              {copy.confirmLabel}
              <kbd className="ml-1 rounded bg-destructive-foreground/15 px-1.5 py-0.5 text-[10px]">Enter</kbd>
            </Button>
          </footer>
        </div>
      </section>
    </div>
  )
}
