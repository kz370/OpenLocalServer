import {
  autoUpdate,
  flip,
  FloatingArrow,
  FloatingPortal,
  offset,
  shift,
  useFloating,
} from '@floating-ui/react'
import * as React from 'react'

import { cn } from '@/lib/utils'
import { parseTooltipShortcut } from '@/lib/tooltip'

export { parseTooltipShortcut } from '@/lib/tooltip'

export interface TooltipProps {
  content: React.ReactNode
  children: React.ReactElement
  side?: 'top' | 'right' | 'bottom' | 'left'
  align?: 'start' | 'center' | 'end'
  delay?: number
  disabled?: boolean
  className?: string
  withArrow?: boolean
}

/**
 * Modern themed tooltip content renderer.
 * Formats clean typography, high contrast surface, backdrop-blur, and optional keyboard shortcut badge.
 */
export function TooltipContentBody({
  content,
  shortcut,
}: {
  content: React.ReactNode
  shortcut?: string | null
}) {
  if (typeof content !== 'string') {
    return <>{content}</>
  }

  const parsed = shortcut !== undefined ? { label: content, shortcut } : parseTooltipShortcut(content)

  return (
    <div className="flex items-center gap-1.5 flex-wrap">
      <span className="min-w-0 [overflow-wrap:anywhere]">{parsed.label}</span>
      {parsed.shortcut && (
        <kbd className="inline-flex items-center rounded border border-white/20 bg-white/10 px-1 py-0.5 text-[10px] font-mono font-medium text-slate-200 dark:border-white/15 dark:bg-white/10">
          {parsed.shortcut}
        </kbd>
      )}
    </div>
  )
}

/**
 * Explicit Tooltip component for wrapping any JSX element.
 */
export function Tooltip({
  content,
  children,
  side = 'top',
  align = 'center',
  delay = 200,
  disabled = false,
  className,
  withArrow = false,
}: TooltipProps) {
  const [open, setOpen] = React.useState(false)
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const arrowRef = React.useRef<SVGSVGElement | null>(null)

  const placement = (align === 'center' ? side : `${side}-${align}`) as
    | 'top'
    | 'right'
    | 'bottom'
    | 'left'
    | 'top-start'
    | 'top-end'
    | 'bottom-start'
    | 'bottom-end'
    | 'right-start'
    | 'right-end'
    | 'left-start'
    | 'left-end'

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: setOpen,
    placement,
    whileElementsMounted: autoUpdate,
    middleware: [
      offset(withArrow ? 8 : 6),
      flip({ fallbackAxisSideDirection: 'start', padding: 8 }),
      shift({ padding: 8 }),
    ],
  })

  const clearTimer = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }

  const handlePointerEnter = () => {
    if (disabled || !content) return
    clearTimer()
    timerRef.current = setTimeout(() => setOpen(true), delay)
  }

  const handlePointerLeave = () => {
    clearTimer()
    setOpen(false)
  }

  React.useEffect(() => {
    return () => clearTimer()
  }, [])

  if (disabled || !content) {
    return children
  }

  return (
    <>
      <span
        ref={refs.setReference}
        onPointerEnter={handlePointerEnter}
        onPointerLeave={handlePointerLeave}
        onFocus={handlePointerEnter}
        onBlur={handlePointerLeave}
        onPointerDown={() => {
          clearTimer()
          setOpen(false)
        }}
        className="inline-flex"
      >
        {children}
      </span>
      {open && (
        <FloatingPortal>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            role="tooltip"
            className={cn(
              'pointer-events-none z-[9999] max-w-[min(20rem,calc(100vw-1rem))] select-none rounded-md px-2.5 py-1 text-[11.5px] font-medium leading-tight tracking-normal shadow-lg transition-opacity duration-150',
              'bg-slate-900/95 text-slate-100 border border-slate-800 shadow-slate-950/30 backdrop-blur-md',
              'dark:bg-[#0a1224]/95 dark:text-slate-100 dark:border-white/15 dark:shadow-black/50',
              className,
            )}
          >
            <TooltipContentBody content={content} />
            {withArrow && (
              <FloatingArrow
                ref={arrowRef}
                context={context}
                className="fill-slate-900 dark:fill-[#0a1224] stroke-slate-800 dark:stroke-white/15"
                strokeWidth={1}
              />
            )}
          </div>
        </FloatingPortal>
      )}
    </>
  )
}

/**
 * Global Tooltip Host.
 * Automatically catches hover on any element with `title` or `data-tooltip`,
 * suppresses the browser's native yellow/white OS tooltip,
 * and renders a modern themed floating tooltip with delay-group smoothing.
 */
let lastClosedTime = 0
const RECENT_THRESHOLD_MS = 350

export function GlobalTooltipHost() {
  const [activeEl, setActiveEl] = React.useState<HTMLElement | null>(null)
  const [text, setText] = React.useState<string | null>(null)
  const [open, setOpen] = React.useState(false)
  const openTimer = React.useRef<ReturnType<typeof setTimeout> | null>(null)

  const { refs, floatingStyles } = useFloating({
    open,
    placement: 'top',
    whileElementsMounted: autoUpdate,
    elements: {
      reference: activeEl,
    },
    middleware: [
      offset(6),
      flip({ fallbackAxisSideDirection: 'start', padding: 8 }),
      shift({ padding: 8 }),
    ],
  })

  const dismiss = React.useCallback(() => {
    if (openTimer.current) {
      clearTimeout(openTimer.current)
      openTimer.current = null
    }
    if (open) {
      lastClosedTime = Date.now()
      setOpen(false)
    }
    setActiveEl(null)
    setText(null)
  }, [open])

  React.useEffect(() => {
    function handlePointerOver(e: PointerEvent) {
      const target = e.target as HTMLElement | null
      if (!target || typeof target.closest !== 'function') return

      const el = target.closest('[title], [data-tooltip]') as HTMLElement | null
      if (!el || el.hasAttribute('data-no-tooltip')) return

      // Migrate native title to data-tooltip to suppress ugly browser tooltip
      if (el.hasAttribute('title')) {
        const rawTitle = el.getAttribute('title')?.trim()
        if (rawTitle) {
          el.setAttribute('data-tooltip', rawTitle)
          el.removeAttribute('title')
          if (!el.hasAttribute('aria-label')) {
            el.setAttribute('aria-label', rawTitle)
          }
        }
      }

      const tooltipText = el.getAttribute('data-tooltip')?.trim()
      if (!tooltipText) {
        dismiss()
        return
      }

      // If already showing for this element, do nothing
      if (activeEl === el && open) return

      if (openTimer.current) {
        clearTimeout(openTimer.current)
        openTimer.current = null
      }

      setActiveEl(el)
      setText(tooltipText)

      // Delay group logic: if another tooltip was open recently, open immediately (no delay)
      const isWarm = Date.now() - lastClosedTime < RECENT_THRESHOLD_MS
      if (isWarm) {
        setOpen(true)
      } else {
        openTimer.current = setTimeout(() => {
          setOpen(true)
        }, 220)
      }
    }

    function handlePointerOut(e: PointerEvent) {
      if (!activeEl) return
      const related = e.relatedTarget as Node | null
      if (related && activeEl.contains(related)) {
        return // Still inside the active element
      }
      dismiss()
    }

    function handlePointerDown() {
      dismiss()
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        dismiss()
      }
    }

    function handleScroll() {
      dismiss()
    }

    document.addEventListener('pointerover', handlePointerOver, true)
    document.addEventListener('pointerout', handlePointerOut, true)
    document.addEventListener('pointerdown', handlePointerDown, true)
    document.addEventListener('keydown', handleKeyDown, true)
    window.addEventListener('scroll', handleScroll, true)

    return () => {
      document.removeEventListener('pointerover', handlePointerOver, true)
      document.removeEventListener('pointerout', handlePointerOut, true)
      document.removeEventListener('pointerdown', handlePointerDown, true)
      document.removeEventListener('keydown', handleKeyDown, true)
      window.removeEventListener('scroll', handleScroll, true)
    }
  }, [activeEl, open, dismiss])

  if (!open || !text || !activeEl) {
    return null
  }

  return (
    <FloatingPortal>
      <div
        ref={refs.setFloating}
        style={floatingStyles}
        role="tooltip"
        className={cn(
          'pointer-events-none z-[9999] max-w-[min(20rem,calc(100vw-1rem))] select-none rounded-md px-2.5 py-1 text-[11.5px] font-medium leading-tight tracking-normal shadow-lg transition-opacity duration-150',
          'bg-slate-900/95 text-slate-100 border border-slate-800 shadow-slate-950/30 backdrop-blur-md',
          'dark:bg-[#0a1224]/95 dark:text-slate-100 dark:border-white/15 dark:shadow-black/50',
        )}
      >
        <TooltipContentBody content={text} />
      </div>
    </FloatingPortal>
  )
}

/**
 * Root Tooltip Provider that mounts the GlobalTooltipHost.
 */
export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return (
    <>
      <GlobalTooltipHost />
      {children}
    </>
  )
}
