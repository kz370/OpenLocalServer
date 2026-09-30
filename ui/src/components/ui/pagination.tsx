import { ChevronLeft, ChevronRight } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/form'
import { cn } from '@/lib/utils'

/** Page sizes offered by the dropdown. Small enough to be a scan, large enough to be a list. */
const PAGE_SIZES = [10, 25, 50, 100] as const

/**
 * Page controls for a long table: how many rows a page holds, which page you are on, and
 * how many rows there are in total.
 *
 * The caller owns `page` and `perPage` and the slice; this component only draws the
 * controls and clamps the page number for it. A `perPage` change always goes back to
 * page 1, because page 7 of 25 rows per page has no page 7 — leaving the caller on a page
 * past the new last page is the classic way a filter silently empties a table.
 */
export function Pagination({
  total,
  page,
  perPage,
  onPage,
  onPerPage,
  label = 'rows',
  className,
}: {
  total: number
  page: number
  perPage: number
  onPage: (page: number) => void
  onPerPage: (perPage: number) => void
  /** What is being counted, for the screen reader and the visible count. */
  label?: string
  className?: string
}) {
  const pages = Math.max(1, Math.ceil(total / perPage))
  // The caller may still be holding a page past the end after a filter or a delete
  // shrank the list; drawing from a clamped page keeps the numbering honest even for the
  // frame before the caller's effect corrects it.
  const current = Math.min(Math.max(1, page), pages)
  const from = total === 0 ? 0 : (current - 1) * perPage + 1
  const to = Math.min(current * perPage, total)

  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-3 px-4 py-2 text-sm text-muted-foreground', className)}>
      <p className="whitespace-nowrap" role="status">
        {total === 0 ? `No ${label}` : `${from}–${to} of ${total} ${label}`}
      </p>
      <div className="flex items-center gap-3">
        <label className="flex items-center gap-1.5 whitespace-nowrap">
          <span>Rows per page</span>
          <Select
            aria-label={`${label} per page`}
            className="h-8 w-20 text-sm"
            value={perPage}
            onChange={(e) => onPerPage(Number(e.target.value))}
          >
            {PAGE_SIZES.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </Select>
        </label>
        <div className="flex items-center gap-1">
          <Button
            size="sm"
            variant="ghost"
            className="size-8 cursor-pointer px-0"
            disabled={current <= 1}
            onClick={() => onPage(current - 1)}
            title="Previous page"
            aria-label="Previous page"
          >
            <ChevronLeft className="size-4" />
          </Button>
          <span className="whitespace-nowrap px-1 tabular-nums">
            Page {current} of {pages}
          </span>
          <Button
            size="sm"
            variant="ghost"
            className="size-8 cursor-pointer px-0"
            disabled={current >= pages}
            onClick={() => onPage(current + 1)}
            title="Next page"
            aria-label="Next page"
          >
            <ChevronRight className="size-4" />
          </Button>
        </div>
      </div>
    </div>
  )
}
