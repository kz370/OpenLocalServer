import * as React from 'react'

import { cn } from '@/lib/utils'

const Table = React.forwardRef<HTMLTableElement, React.HTMLAttributes<HTMLTableElement> & { wrapperClassName?: string }>(
  ({ className, wrapperClassName, ...props }, ref) => (
    <div className={cn('@container w-full overflow-x-auto border border-border/80 bg-card/60 backdrop-blur-md', wrapperClassName ?? 'rounded-lg')}>
      <table ref={ref} className={cn('w-full caption-bottom break-words text-sm', className)} {...props} />
    </div>
  ),
)
Table.displayName = 'Table'

const TableHeader = React.forwardRef<HTMLTableSectionElement, React.HTMLAttributes<HTMLTableSectionElement> & { sticky?: boolean }>(
  ({ className, sticky, ...props }, ref) => (
    <thead
      ref={ref}
      className={cn(
        'bg-muted/75 border-b border-border/80 backdrop-blur-xs',
        // Opaque so rows never show through; needs a Table with wrapperClassName="overflow-visible".
        sticky && 'sticky top-0 z-10 bg-[var(--dialog-surface)] backdrop-blur-none [&_th]:bg-[var(--dialog-surface)] [&_th]:shadow-[inset_0_-1px_0_var(--border)]',
        className,
      )}
      {...props}
    />
  ),
)
TableHeader.displayName = 'TableHeader'

const TableBody = React.forwardRef<HTMLTableSectionElement, React.HTMLAttributes<HTMLTableSectionElement>>(
  ({ className, ...props }, ref) => (
    <tbody ref={ref} className={cn('[&_tr:last-child]:border-0', className)} {...props} />
  ),
)
TableBody.displayName = 'TableBody'

const TableRow = React.forwardRef<HTMLTableRowElement, React.HTMLAttributes<HTMLTableRowElement>>(
  ({ className, ...props }, ref) => (
    <tr
      ref={ref}
      className={cn(
        'border-b border-border/70 transition-colors last:border-0 hover:bg-muted/40 data-[state=selected]:bg-muted/60',
        className,
      )}
      {...props}
    />
  ),
)
TableRow.displayName = 'TableRow'

const TableHead = React.forwardRef<HTMLTableCellElement, React.ThHTMLAttributes<HTMLTableCellElement>>(
  ({ className, ...props }, ref) => (
    <th
      ref={ref}
      className={cn(
        'h-9 px-3 text-left align-middle text-xs font-semibold uppercase tracking-wider text-muted-foreground first:pl-4 last:pr-4',
        className,
      )}
      {...props}
    />
  ),
)
TableHead.displayName = 'TableHead'

const TableCell = React.forwardRef<HTMLTableCellElement, React.TdHTMLAttributes<HTMLTableCellElement>>(
  ({ className, ...props }, ref) => (
    <td ref={ref} className={cn('px-3 py-2 align-middle first:pl-4 last:pr-4', className)} {...props} />
  ),
)
TableCell.displayName = 'TableCell'

/** How many columns the table's header row shows right now (hidden headers don't count). */
function visibleColumns(table: HTMLTableElement): number {
  const row = table.tHead?.rows[0]
  if (!row) return 0
  return Array.from(row.cells)
    .filter((c) => getComputedStyle(c).display !== 'none')
    .reduce((n, c) => n + c.colSpan, 0)
}

/**
 * A cell spanning the whole row: group headings, empty and loading rows.
 *
 * A fixed `colSpan` counts columns a container query hides on narrow windows, and in a
 * `table-fixed` table those hidden columns come back as empty ones that take a share of
 * the width. That pushed the Actions column into the middle of the row. This cell spans
 * exactly the columns the header shows, and recounts when the table is resized.
 */
const TableSpanCell = ({ colSpan, ...props }: React.TdHTMLAttributes<HTMLTableCellElement>) => {
  const ref = React.useRef<HTMLTableCellElement>(null)
  const [span, setSpan] = React.useState(colSpan)
  React.useLayoutEffect(() => {
    const table = ref.current?.closest('table')
    if (!table) return
    const count = () => {
      const n = visibleColumns(table)
      if (n > 0) setSpan(n)
    }
    count()
    const observer = new ResizeObserver(count)
    observer.observe(table)
    return () => observer.disconnect()
  }, [])
  return <TableCell ref={ref} colSpan={span} {...props} />
}

export { Table, TableHeader, TableBody, TableRow, TableHead, TableCell, TableSpanCell }
