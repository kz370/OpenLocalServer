import { MoreHorizontal } from 'lucide-react'
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export type MenuItem =
  | { label: string; icon?: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string }
  | 'separator'

export type MenuAnchor =
  | { type: 'button'; rect: DOMRect }
  | { type: 'cursor'; x: number; y: number }

const TITLEBAR_HEIGHT = 36
const MARGIN = 8
const MIN_TOP = TITLEBAR_HEIGHT + MARGIN // 44px: leaves an 8px margin below the 36px titlebar

/**
 * A floating menu that positions itself either relative to a button or at the cursor (for context menus).
 * Fixed-positioned so containers cannot clip it; closes on outside click, Escape, resize, or external scroll.
 */
export function FloatingMenu({
  items,
  anchor,
  onClose,
}: {
  items: MenuItem[]
  anchor: MenuAnchor
  onClose: () => void
}) {
  const menu = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState<{ top: number; left?: number; right?: number } | null>(null)

  useLayoutEffect(() => {
    if (!menu.current) return
    const menuEl = menu.current
    const width = menuEl.offsetWidth || 192
    const height = menuEl.offsetHeight || 200
    const winWidth = window.innerWidth
    const winHeight = window.innerHeight
    const maxBottom = winHeight - MARGIN

    if (anchor.type === 'cursor') {
      let top = anchor.y
      // If opening below would overflow the bottom edge, flip to open above cursor
      if (top + height > maxBottom) {
        top = anchor.y - height
      }
      // Never allow top to be tucked under the titlebar
      if (top < MIN_TOP) {
        top = MIN_TOP
      }
      // If it still overflows the bottom, clamp to bottom margin
      if (top + height > maxBottom) {
        top = Math.max(MIN_TOP, maxBottom - height)
      }

      let left = anchor.x
      // If opening to the right would overflow right edge, flip to open to the left of cursor
      if (left + width > winWidth - MARGIN) {
        left = anchor.x - width
      }
      if (left < MARGIN) {
        left = MARGIN
      }
      if (left + width > winWidth - MARGIN) {
        left = Math.max(MARGIN, winWidth - MARGIN - width)
      }

      setPos({ top, left })
    } else {
      const r = anchor.rect
      const fitsBelow = r.bottom + 4 + height <= maxBottom
      const fitsAbove = r.top - 4 - height >= MIN_TOP

      let top: number
      if (fitsBelow) {
        top = r.bottom + 4
      } else if (fitsAbove) {
        top = r.top - height - 4
      } else {
        const spaceBelow = maxBottom - (r.bottom + 4)
        const spaceAbove = r.top - 4 - MIN_TOP
        top = spaceBelow >= spaceAbove ? r.bottom + 4 : r.top - height - 4
      }

      if (top < MIN_TOP) {
        top = MIN_TOP
      }
      if (top + height > maxBottom) {
        top = Math.max(MIN_TOP, maxBottom - height)
      }

      const right = Math.max(MARGIN, Math.min(winWidth - r.right, winWidth - MARGIN - width))
      setPos({
        top,
        right,
      })
    }
  }, [anchor])

  useEffect(() => {
    const away = (e: MouseEvent) => {
      const t = e.target as Node
      if (!menu.current?.contains(t)) onClose()
    }
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    const handleScroll = (e: Event) => {
      const target = e.target as Node | null
      // Do not close when scrolling inside the floating menu itself
      if (menu.current && target && menu.current.contains(target)) return
      onClose()
    }
    const close = () => onClose()

    window.addEventListener('mousedown', away)
    window.addEventListener('keydown', esc)
    window.addEventListener('scroll', handleScroll, true)
    window.addEventListener('resize', close)
    return () => {
      window.removeEventListener('mousedown', away)
      window.removeEventListener('keydown', esc)
      window.removeEventListener('scroll', handleScroll, true)
      window.removeEventListener('resize', close)
    }
  }, [onClose])

  return (
    <div
      ref={menu}
      role="menu"
      style={
        pos
          ? {
              top: Math.max(MIN_TOP, pos.top),
              left: pos.left,
              right: pos.right,
              maxHeight: 'calc(100vh - 52px)',
            }
          : {
              visibility: 'hidden',
              top: MIN_TOP,
              left: 0,
              maxHeight: 'calc(100vh - 52px)',
            }
      }
      className="fixed z-[95] min-w-48 max-h-[calc(100vh-52px)] overflow-y-auto rounded-lg border border-border bg-card p-1 text-sm shadow-lg select-none"
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
              onClose()
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
  )
}

/**
 * A "more actions" button with a dropdown. The menu is fixed-positioned, so tables and
 * scrolling containers can't clip it; it closes on outside click, Escape or scroll.
 */
export function ActionMenu({ items, label = 'More actions' }: { items: MenuItem[]; label?: string }) {
  const [open, setOpen] = useState(false)
  const button = useRef<HTMLButtonElement>(null)

  return (
    <>
      <Button
        ref={button}
        size="sm"
        variant="ghost"
        className="h-8 w-8 shrink-0 cursor-pointer px-0"
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <MoreHorizontal className="size-4" />
      </Button>
      {open && button.current && (
        <FloatingMenu
          items={items}
          anchor={{ type: 'button', rect: button.current.getBoundingClientRect() }}
          onClose={() => {
            setOpen(false)
            button.current?.focus()
          }}
        />
      )}
    </>
  )
}
