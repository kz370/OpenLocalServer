import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { useAppMarkPath } from '@/lib/hooks'

export function ExitingOverlay() {
  const [exiting, setExiting] = useState(false)
  const mark = useAppMarkPath()

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen('ols:exiting', () => {
      setExiting(true)
    }).then((cleanup) => {
      unlisten = cleanup
    })
    return () => unlisten?.()
  }, [])

  if (!exiting) return null

  return (
    <div
      role="alert"
      aria-live="assertive"
      className="fixed inset-0 z-[999999] flex flex-col items-center justify-center select-none bg-background/85 backdrop-blur-md transition-all duration-300"
    >
      <div className="relative flex flex-col items-center p-8 rounded-2xl border border-border/60 bg-card/90 shadow-2xl shadow-primary/10 max-w-sm text-center">
        {/* Glow backdrop behind mark */}
        <div className="relative mb-5 flex items-center justify-center">
          <div className="absolute -inset-3 rounded-full bg-primary/20 blur-xl animate-pulse" />
          {/* Rotating spinning border ring */}
          <div className="absolute -inset-1.5 rounded-full border-2 border-transparent border-t-primary border-r-primary/50 animate-spin" />
          <img
            src={mark}
            alt="OLS"
            className="size-12 rounded-full shadow-md"
          />
        </div>

        <h2 className="text-base font-semibold tracking-tight text-foreground">
          Shutting down OpenLocalServer…
        </h2>
        <p className="mt-1.5 text-xs text-muted-foreground leading-relaxed">
          Stopping services, web servers, and releasing resources safely.
        </p>

        {/* Indeterminate animated loading bar */}
        <div className="mt-5 h-1.5 w-48 overflow-hidden rounded-full bg-muted/80">
          <div className="h-full w-2/5 rounded-full bg-primary animate-[ols-indeterminate_1.4s_infinite_ease-in-out]" />
        </div>
      </div>
    </div>
  )
}
