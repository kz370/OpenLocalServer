import * as React from 'react'

import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'

interface ScrollableListProps extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'> {
  /** Search box, tabs, bulk bar. Fixed above the rows. */
  toolbar?: React.ReactNode
  /** Scrolls to the top when this changes (search text, active tab) - not on row updates. */
  resetKey?: React.Key | null
  /** Decorations over the card (progress bar, overlays). */
  overlay?: React.ReactNode
  /** Fixed below the rows, e.g. pagination. */
  footer?: React.ReactNode
  className?: string
  scrollerClassName?: string
  children: React.ReactNode
}

/**
 * Card for list pages inside PageLayout mode="list": toolbar stays put, the rows scroll.
 * Put a <Table wrapperClassName="overflow-visible"> with <TableHeader sticky> inside so the column
 * header sticks to the top of the scroller.
 */
export function ScrollableList({ toolbar, resetKey, overlay, footer, className, scrollerClassName, children, ...rest }: ScrollableListProps) {
  const scroller = React.useRef<HTMLDivElement>(null)
  React.useEffect(() => {
    scroller.current?.scrollTo({ top: 0 })
  }, [resetKey])

  return (
    <Card className={cn('relative flex min-h-0 flex-1 flex-col', className)} {...rest}>
      {overlay}
      {toolbar && <div className="shrink-0">{toolbar}</div>}
      <div ref={scroller} className={cn('ols-scroll min-h-0 flex-1', scrollerClassName)}>
        {children}
      </div>
      {footer && <div className="shrink-0">{footer}</div>}
    </Card>
  )
}
