import { Copy, Minus, Square, X } from 'lucide-react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { NotificationsPopover } from '@/components/NotificationsPopover'
import { useAppMarkPath, useWindowState } from '@/lib/hooks'
import { useThemeSafe } from '@/lib/theme'
import { cn } from '@/lib/utils'

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export function Titlebar({ onNavigateLogs }: { onNavigateLogs?: (source?: string | null) => void }) {
  const { maximized } = useWindowState()
  const theme = useThemeSafe()
  const isGradient = theme?.options?.background?.type === 'gradient'
  // Same four colourways (light/dark × running/stopped) the tray and taskbar use.
  const mark = useAppMarkPath()

  const action = (fn: (win: ReturnType<typeof getCurrentWindow>) => void) => () => {
    if (!isTauri()) return
    fn(getCurrentWindow())
  }

  return (
    <header
      data-tauri-drag-region
      onDoubleClick={action((win) => void win.toggleMaximize().catch(() => undefined))}
      className={cn(
        'relative z-[100] flex h-9 shrink-0 select-none items-center gap-2 border-b border-sidebar-border backdrop-blur-xl px-3 transition-colors duration-300',
        isGradient ? 'bg-sidebar/40' : 'bg-sidebar/85',
      )}
    >
      <span className="flex items-center gap-2 text-xs font-medium text-sidebar-foreground">
        {/* Same four colourways (light/dark × running/stopped) the tray and the
            taskbar use. */}
        <img
          src={mark}
          alt=""
          className="size-4 rounded-full"
          aria-hidden
        />
        {/* The window is the one surface that is always on screen, so it carries the
            full name and not just the short mark — "OLS" alone means nothing the first
            time someone sees it. The expansion is muted, and the same treatment the
            About card already uses, so the two read as one brand. */}
         <span className="text-sidebar-foreground">Open Local Server</span>
      </span>
      <div data-tauri-drag-region className="min-w-0 flex-1" />
      <div className="flex items-center gap-1.5">
        <NotificationsPopover onNavigateLogs={onNavigateLogs} />
        {isTauri() && (
          <div className="flex items-center gap-1 ml-1 border-l border-sidebar-border/60 pl-1.5">
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
      </div>
    </header>
  )
}
