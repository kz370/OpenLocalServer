import { MoreHorizontal } from 'lucide-react'
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export type MenuItem =
  | { label: string; icon?: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string }
  | 'separator'

/**
 * A "more actions" button with a dropdown. The menu is fixed-positioned, so tables and
 * scrolling containers can't clip it; it closes on outside click, Escape or scroll.
 */
export function ActionMenu({ items, label = 'More actions' }: { items: MenuItem[]; label?: string }) {
  const [open, setOpen] = useState(false)
  const [pos, setPos] = useState<{ top: number; right: number; up: boolean } | null>(null)
  const button = useRef<HTMLButtonElement>(null)
  const menu = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    if (!open || !button.current) return
    const r = button.current.getBoundingClientRect()
    const height = menu.current?.offsetHeight ?? 0
    const up = r.bottom + height + 8 > window.innerHeight && r.top > height
    setPos({ top: up ? r.top - height - 4 : r.bottom + 4, right: window.innerWidth - r.right, up })
  }, [open])

  useEffect(() => {
    if (!open) return
    const away = (e: MouseEvent) => {
      const t = e.target as Node
      if (!menu.current?.contains(t) && !button.current?.contains(t)) setOpen(false)
    }
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false)
        button.current?.focus()
      }
    }
    const close = () => setOpen(false)
    window.addEventListener('mousedown', away)
    window.addEventListener('keydown', esc)
    window.addEventListener('scroll', close, true)
    window.addEventListener('resize', close)
    return () => {
      window.removeEventListener('mousedown', away)
      window.removeEventListener('keydown', esc)
      window.removeEventListener('scroll', close, true)
      window.removeEventListener('resize', close)
    }
  }, [open])

  return (
    <>
      <Button ref={button} size="sm" variant="ghost" className="h-8 w-8 shrink-0 cursor-pointer px-0" title={label} aria-label={label} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <MoreHorizontal className="size-4" />
      </Button>
      {open && (
        <div
          ref={menu}
          role="menu"
          style={pos ? { top: pos.top, right: pos.right } : { visibility: 'hidden', top: 0, right: 0 }}
          className="fixed z-50 min-w-48 rounded-lg border border-border bg-card p-1 text-sm shadow-lg"
        >
          {items.map((item, i) =>
            item === 'separator' ? (
              <div key={`sep-${i}`} className="my-1 h-px bg-border" />
            ) : (
              <button
                key={item.label}
                role="menuitem"
                disabled={item.disabled}
                title={item.hint}
                onClick={() => {
                  setOpen(false)
                  item.onSelect()
                }}
                className={cn(
                  'flex w-full cursor-pointer items-center gap-2 rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-3.5 [&_svg]:shrink-0',
                  item.danger ? 'text-destructive hover:bg-destructive/10' : '',
                )}
              >
                {item.icon}
                {item.label}
              </button>
            ),
          )}
        </div>
      )}
    </>
  )
}
