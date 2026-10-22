import { X } from 'lucide-react'
import { createContext, useContext, type ReactNode, useEffect } from 'react'

import { usePresence } from '@/lib/hooks'
import { cn } from '@/lib/utils'

/** Tracks modal nesting depth so inner dialogs are automatically layered above outer dialogs. */
export const DialogDepthContext = createContext<number>(0)

export type DialogLayer = 'default' | 'sub' | 'top' | 'ai' | 'confirm'

/** A plain modal: dimmed backdrop, Escape / backdrop click closes, content scrolls. */
export function Dialog({
  open,
  onClose,
  title,
  description,
  icon,
  children,
  footer,
  wide,
  size,
  layer = 'default',
}: {
  open: boolean
  onClose: () => void
  title: string
  description?: string
  /** Optional brand tile shown left of the title (e.g. Quick App wizards). Keeps form modals on one header grid. */
  icon?: ReactNode
  children: ReactNode
  footer?: ReactNode
  /** @deprecated Use `size` instead. `wide` maps to `size="wide"`. */
  wide?: boolean
  /** `default` = max-w-xl, `form` = max-w-2xl (shared form-modal width), `wide` = max-w-4xl (editors). */
  size?: 'default' | 'form' | 'wide'
  /** Hierarchy layer: 'confirm' (z-90), 'ai' (z-80), 'top'/'sub' (z-70), 'default' (z-60, auto-promotes to z-70 if nested). */
  layer?: DialogLayer
}) {
  const parentDepth = useContext(DialogDepthContext)

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  const { mounted, state } = usePresence(open)
  if (!mounted) return null
  const width = size ?? (wide ? 'wide' : 'default')

  // Layer hierarchy: confirm (90) > ai (80) > top/sub or nested (70) > base (60)
  const zClass =
    layer === 'confirm'
      ? 'z-[90]'
      : layer === 'ai'
      ? 'z-[80]'
      : layer === 'top' || layer === 'sub' || parentDepth > 0
      ? 'z-[70]'
      : 'z-[60]'

  const currentDepth = parentDepth + 1

  return (
    <DialogDepthContext.Provider value={currentDepth}>
      <div className={cn('fixed top-9 inset-x-0 bottom-0 flex items-center justify-center p-4', zClass)}>
        <div data-state={state} className="modal-backdrop absolute inset-0 bg-black/50" onClick={onClose} />
        <div
          data-state={state}
          role="dialog"
          aria-modal="true"
          aria-label={title}
          className={cn(
            'modal-panel relative flex max-h-[calc(100vh-5rem)] w-full flex-col rounded-xl border border-border bg-card text-card-foreground shadow-2xl',
            width === 'wide' ? 'max-w-4xl' : width === 'form' ? 'max-w-2xl' : 'max-w-xl',
          )}
        >
          <div data-tauri-drag-region className="flex items-start justify-between gap-4 border-b border-border px-5 py-4 select-none">
            <div data-tauri-drag-region className="flex min-w-0 flex-1 items-start gap-3">
              {icon && <span className="shrink-0">{icon}</span>}
              <div data-tauri-drag-region className="min-w-0 flex-1">
                <h2 data-tauri-drag-region className="text-base font-semibold">{title}</h2>
                {description && <p data-tauri-drag-region className="mt-0.5 text-sm text-muted-foreground">{description}</p>}
              </div>
            </div>
            <button onClick={onClose} className="rounded-md p-1 text-muted-foreground hover:bg-accent shrink-0 cursor-pointer" aria-label="Close">
              <X className="size-4" />
            </button>
          </div>
          <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer && <div className="flex flex-wrap justify-end gap-2 border-t border-border px-5 py-3">{footer}</div>}
        </div>
      </div>
    </DialogDepthContext.Provider>
  )
}
