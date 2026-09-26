import { Copy, Minus, Square, X } from 'lucide-react'
import { useEffect, useState } from 'react'

import { cn } from '@/lib/utils'

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export function Titlebar() {
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    void (async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      setMaximized(await win.isMaximized().catch(() => false))
      unlisten = await win.onResized(async () => {
        setMaximized(await win.isMaximized().catch(() => false))
      })
    })()
    return () => unlisten?.()
  }, [])

  const action = (fn: (win: Awaited<ReturnType<typeof import('@tauri-apps/api/window').getCurrentWindow>>) => void) => async () => {
    if (!isTauri()) return
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    fn(getCurrentWindow())
  }

  return (
    <header
      data-tauri-drag-region
      onDoubleClick={action((win) => void win.toggleMaximize().catch(() => undefined))}
      className="flex h-9 shrink-0 select-none items-center gap-2 border-b border-sidebar-border bg-sidebar px-3"
    >
      <span className="flex items-center gap-2 text-xs font-medium text-sidebar-foreground">
        <span className="size-2 rounded-full bg-primary" aria-hidden />
        OpenLocalServer
      </span>
      <div data-tauri-drag-region className="min-w-0 flex-1" />
      {isTauri() && (
        <div className="flex items-center gap-1">
          <button
            onClick={action((win) => void win.minimize().catch(() => undefined))}
            title="Minimize"
            aria-label="Minimize"
            className="flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <Minus className="size-3.5" />
          </button>
          <button
            onClick={action((win) => void win.toggleMaximize().catch(() => undefined))}
            title={maximized ? 'Restore' : 'Maximize'}
            aria-label={maximized ? 'Restore' : 'Maximize'}
            className="flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            {maximized ? <Copy className={cn('size-3 rotate-180')} /> : <Square className="size-3" />}
          </button>
          <button
            onClick={action((win) => void win.close().catch(() => undefined))}
            title="Close"
            aria-label="Close"
            className="flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-destructive hover:text-destructive-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>
      )}
    </header>
  )
}
