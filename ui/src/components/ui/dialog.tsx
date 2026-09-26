import { X } from 'lucide-react'
import { type ReactNode, useEffect } from 'react'

import { usePresence } from '@/lib/hooks'
import { cn } from '@/lib/utils'

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
  layer,
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
  /** Use for a dialog opened from another dialog so it renders above its parent. */
  layer?: 'default' | 'top'
}) {
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose])

  const { mounted, state } = usePresence(open)
  if (!mounted) return null
  const width = size ?? (wide ? 'wide' : 'default')
  return (
    <div className={cn('fixed inset-0 flex items-center justify-center p-4', layer === 'top' ? 'z-[60]' : 'z-50')}>
      <div data-state={state} className="modal-backdrop absolute inset-0 bg-black/50" onClick={onClose} />
      <div
        data-state={state}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={cn(
          'modal-panel relative flex max-h-[90vh] w-full flex-col rounded-xl border border-border bg-card text-card-foreground shadow-2xl',
          width === 'wide' ? 'max-w-4xl' : width === 'form' ? 'max-w-2xl' : 'max-w-xl',
        )}
      >
        <div className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
          <div className="flex min-w-0 items-start gap-3">
            {icon && <span className="shrink-0">{icon}</span>}
            <div className="min-w-0">
              <h2 className="text-base font-semibold">{title}</h2>
              {description && <p className="mt-0.5 text-sm text-muted-foreground">{description}</p>}
            </div>
          </div>
          <button onClick={onClose} className="rounded-md p-1 text-muted-foreground hover:bg-accent" aria-label="Close">
            <X className="size-4" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer && <div className="flex flex-wrap justify-end gap-2 border-t border-border px-5 py-3">{footer}</div>}
      </div>
    </div>
  )
}
