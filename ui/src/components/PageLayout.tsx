import * as React from 'react'

import { cn } from '@/lib/utils'

/**
 * list   - content is a ScrollableList (or similar) that owns the only scroller.
 * scroll - content is a stack of cards/widgets; the content region scrolls as one.
 * fill   - content fills the remaining height and scrolls internally (logs, terminal, editor).
 */
export type PageLayoutMode = 'list' | 'scroll' | 'fill'

interface PageLayoutProps {
  header: React.ReactNode
  toolbar?: React.ReactNode
  footer?: React.ReactNode
  mode?: PageLayoutMode
  /** Scrollbar resets to the top when this value changes (search text, active tab). */
  resetKey?: React.Key | null
  className?: string
  contentClassName?: string
  children: React.ReactNode
}

/** Smallest height the scrolling region keeps before the page wrapper scrolls instead. */
const MIN_CONTENT = 'min-h-[240px]'

/**
 * Page frame. Header, toolbar and footer never scroll; only the content region does
 * (one scroller per page). Very short windows fall back to scrolling the wrapper.
 */
export function PageLayout({ header, toolbar, footer, mode = 'scroll', resetKey, className, contentClassName, children }: PageLayoutProps) {
  const scroller = React.useRef<HTMLDivElement>(null)
  React.useEffect(() => {
    if (mode === 'fill') return
    scroller.current?.scrollTo({ top: 0 })
  }, [resetKey, mode])

  return (
    <div className={cn('flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto overscroll-contain [scrollbar-gutter:stable]', className)}>
      <div className="shrink-0">{header}</div>
      {toolbar && <div className="shrink-0">{toolbar}</div>}
      <div
        ref={scroller}
        className={cn(
          'min-h-0 flex-1',
          MIN_CONTENT,
          mode === 'scroll' && 'ols-scroll pr-2.5',
          mode === 'list' && 'flex flex-col',
          mode === 'fill' && 'flex flex-col overflow-hidden',
          contentClassName,
        )}
      >
        {children}
      </div>
      {footer && <div className="shrink-0">{footer}</div>}
    </div>
  )
}
