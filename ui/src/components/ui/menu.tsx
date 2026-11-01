import {
  autoUpdate,
  flip,
  FloatingPortal,
  offset,
  shift,
  size,
  useDismiss,
  useFloating,
  useInteractions,
  type VirtualElement,
} from '@floating-ui/react'
import { MoreHorizontal } from 'lucide-react'
import { type ReactNode, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export type MenuItem =
  | { label: string; icon?: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string }
  | 'separator'

export type MenuAnchor =
  | { type: 'button'; rect: DOMRect; element?: HTMLElement | null }
  | { type: 'cursor'; x: number; y: number }

const TITLEBAR_HEIGHT = 36
const MARGIN = 8
const MIN_TOP = TITLEBAR_HEIGHT + MARGIN // 44px: leaves an 8px margin below the 36px titlebar

const VIEWPORT_PADDING = {
  top: MIN_TOP,
  right: MARGIN,
  bottom: MARGIN,
  left: MARGIN,
}

/**
 * A floating menu that positions itself either relative to a button or at the cursor (for context menus).
 * Portaled via FloatingPortal so ancestor transforms/backdrop-filters cannot shift or clip it;
 * automatically flips, shifts, and resizes within the viewport; closes on outside click or Escape.
 */
export function FloatingMenu({
  items,
  anchor,
  header,
  onClose,
}: {
  items: MenuItem[]
  anchor: MenuAnchor
  header?: ReactNode
  onClose: () => void
}) {
  const referenceElement = useMemo<Element | VirtualElement>(() => {
    if (anchor.type === 'button' && anchor.element) {
      return anchor.element
    }
    if (anchor.type === 'cursor') {
      const { x, y } = anchor
      return {
        getBoundingClientRect: () => ({
          width: 0,
          height: 0,
          x,
          y,
          top: y,
          left: x,
          right: x,
          bottom: y,
        }),
      }
    }
    const { rect } = anchor
    return {
      getBoundingClientRect: () => rect,
    }
  }, [anchor])

  const { refs, floatingStyles, context, update, isPositioned } = useFloating({
    open: true,
    onOpenChange: (isOpen) => {
      if (!isOpen) onClose()
    },
    strategy: 'fixed',
    placement: anchor.type === 'button' ? 'bottom-end' : 'bottom-start',
    whileElementsMounted: autoUpdate,
    elements: {
      reference: anchor.type === 'button' ? (anchor.element ?? null) : null,
    },
    middleware: [
      offset(anchor.type === 'button' ? 4 : 2),
      flip({
        padding: VIEWPORT_PADDING,
        fallbackAxisSideDirection: 'end',
      }),
      shift({
        padding: VIEWPORT_PADDING,
        crossAxis: true,
      }),
      size({
        padding: VIEWPORT_PADDING,
        apply({ availableHeight, availableWidth, elements }) {
          Object.assign(elements.floating.style, {
            maxHeight: `${Math.max(96, Math.floor(availableHeight))}px`,
            maxWidth: `${Math.max(192, Math.floor(availableWidth))}px`,
          })
        },
      }),
    ],
  })

  const dismiss = useDismiss(context, {
    outsidePress: true,
    outsidePressEvent: 'mousedown',
    escapeKey: true,
  })

  const { getFloatingProps } = useInteractions([dismiss])

  useLayoutEffect(() => {
    refs.setPositionReference(referenceElement)
    update()
  }, [referenceElement, items.length, refs, update])

  useEffect(() => {
    const handleScroll = (e: Event) => {
      const target = e.target as Node | null
      const floatingEl = refs.floating.current
      // Do not close when scrolling inside the floating menu itself
      if (floatingEl && target && floatingEl.contains(target)) return
      onClose()
    }
    window.addEventListener('scroll', handleScroll, true)
    return () => {
      window.removeEventListener('scroll', handleScroll, true)
    }
  }, [onClose, refs.floating])

  return (
    <FloatingPortal>
      <div
        ref={refs.setFloating}
        role="menu"
        style={{
          ...floatingStyles,
          visibility: isPositioned ? undefined : 'hidden',
        }}
        onContextMenu={(e) => e.preventDefault()}
        {...getFloatingProps()}
        className="z-[95] min-w-48 overflow-y-auto rounded-lg border border-border dark:border-white/15 bg-popover/95 backdrop-blur-md p-1 text-sm text-popover-foreground shadow-xl select-none focus:outline-none"
      >
        {header && (
          <>
            <div
              role="presentation"
              aria-disabled="true"
              className="flex max-w-64 cursor-default items-center gap-2 px-2.5 py-1.5 text-xs font-semibold text-muted-foreground select-none"
            >
              {header}
            </div>
            <div className="my-1 h-px bg-border" />
          </>
        )}
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
                'flex w-full cursor-pointer items-center gap-2 rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-accent/80 hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-3.5 [&_svg]:shrink-0',
                item.danger ? 'text-destructive hover:bg-destructive/10 hover:text-destructive' : '',
              )}
            >
              {item.icon}
              {item.label}
            </button>
          ),
        )}
      </div>
    </FloatingPortal>
  )
}

/**
 * A "more actions" button with a dropdown. The menu is portaled and fixed-positioned, so tables and
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
          anchor={{ type: 'button', rect: button.current.getBoundingClientRect(), element: button.current }}
          onClose={() => {
            setOpen(false)
            button.current?.focus()
          }}
        />
      )}
    </>
  )
}
