import { autoUpdate, flip, FloatingPortal, offset, shift, useDismiss, useFloating, useInteractions } from '@floating-ui/react'
import { ChevronDown, Search, X } from 'lucide-react'
import * as React from 'react'

import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

export interface OverflowTab<T extends string> {
  id: T
  label: string
  badge?: React.ReactNode
  icon?: React.ReactNode
}

const GAP = 2
const DEFAULT_MORE_WIDTH = 96

/**
 * Underline tab row on a single line. Tabs that do not fit move into a searchable "+N more" dropdown.
 * A tab picked from the dropdown is swapped into the visible row.
 */
export function OverflowTabs<T extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: OverflowTab<T>[]
  value: T
  onChange: (id: T) => void
}) {
  const rowRef = React.useRef<HTMLDivElement>(null)
  const measureRef = React.useRef<HTMLDivElement>(null)
  const searchRef = React.useRef<HTMLInputElement>(null)
  const [available, setAvailable] = React.useState(0)
  const [widths, setWidths] = React.useState<Record<string, number>>({})
  const [moreWidth, setMoreWidth] = React.useState(DEFAULT_MORE_WIDTH)
  const [open, setOpen] = React.useState(false)
  const [search, setSearch] = React.useState('')

  // Measure every tab offscreen (labels and badges change with the data).
  const signature = tabs.map((t) => `${t.id}|${t.label}|${String(t.badge ?? '')}`).join(';')
  React.useLayoutEffect(() => {
    const root = measureRef.current
    if (!root) return
    const next: Record<string, number> = {}
    root.querySelectorAll<HTMLElement>('[data-measure-id]').forEach((el) => {
      const id = el.getAttribute('data-measure-id')
      const w = el.getBoundingClientRect().width
      if (!id) return
      if (id === '__more__') {
        if (w > 0) setMoreWidth(w + 6) // + left margin of the real button
      } else {
        next[id] = w
      }
    })
    setWidths(next)
  }, [signature])

  React.useEffect(() => {
    const el = rowRef.current
    if (!el || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) if (e.contentRect.width > 0) setAvailable(e.contentRect.width)
    })
    ro.observe(el)
    setAvailable(el.clientWidth)
    return () => ro.disconnect()
  }, [])

  const { visible, overflow } = React.useMemo(() => {
    const w = (id: string) => widths[id] ?? 90
    const total = tabs.reduce((acc, t) => acc + w(t.id) + GAP, -GAP)
    if (available <= 0 || total <= available) return { visible: tabs, overflow: [] as OverflowTab<T>[] }

    const max = Math.max(available - moreWidth - GAP, 80)
    const vis: OverflowTab<T>[] = []
    const over: OverflowTab<T>[] = []
    let used = 0
    for (const t of tabs) {
      const next = used + (vis.length ? GAP : 0) + w(t.id)
      if (next <= max) {
        vis.push(t)
        used = next
      } else {
        over.push(t)
      }
    }
    // Keep the active tab visible: swap it with the last visible one.
    const idx = over.findIndex((t) => t.id === value)
    if (idx !== -1) {
      if (vis.length > 0) {
        const displaced = vis[vis.length - 1]
        vis[vis.length - 1] = over[idx]
        over[idx] = displaced
      } else {
        vis.push(over[idx])
        over.splice(idx, 1)
      }
    }
    return { visible: vis, overflow: over }
  }, [tabs, widths, available, moreWidth, value])

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: (o) => {
      setOpen(o)
      if (!o) setSearch('')
    },
    placement: 'bottom-start',
    middleware: [offset(4), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
  })
  const dismiss = useDismiss(context)
  const { getReferenceProps, getFloatingProps } = useInteractions([dismiss])

  React.useEffect(() => {
    if (open) requestAnimationFrame(() => searchRef.current?.focus())
  }, [open])

  const term = search.trim().toLowerCase()
  const filtered = term ? overflow.filter((t) => t.label.toLowerCase().includes(term)) : overflow

  const tabClass = 'flex shrink-0 items-center gap-1.5 whitespace-nowrap border-b-2 px-2.5 py-2 text-sm font-medium transition-colors'
  const badgeEl = (t: OverflowTab<T>) =>
    t.badge !== undefined && <span className="rounded-full bg-muted px-1.5 text-[11px]">{t.badge}</span>

  return (
    <>
      <div ref={measureRef} aria-hidden="true" className="pointer-events-none absolute left-[-9999px] top-[-9999px] flex opacity-0">
        {tabs.map((t) => (
          <span key={t.id} data-measure-id={t.id} className={tabClass}>
            {t.icon}
            {t.label}
            {badgeEl(t)}
          </span>
        ))}
        <span data-measure-id="__more__" className={tabClass}>
          +{Math.max(overflow.length, 99)} more
          <ChevronDown className="size-3.5" />
        </span>
      </div>

      <div ref={rowRef} className="flex min-w-0 flex-nowrap gap-0.5 overflow-hidden shadow-[inset_0_-1px_0_var(--border)]">
        {visible.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => onChange(t.id)}
            className={cn(tabClass, value === t.id ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground')}
          >
            {t.icon}
            {t.label}
            {badgeEl(t)}
          </button>
        ))}
        {overflow.length > 0 && (
          <button
            ref={refs.setReference}
            {...getReferenceProps()}
            type="button"
            onClick={() => setOpen((p) => !p)}
            className={cn(
              'my-1 ml-1 flex shrink-0 cursor-pointer items-center gap-1 whitespace-nowrap rounded-md border border-border/80 bg-background px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-muted/70',
              open && 'border-primary/50 bg-muted',
            )}
            title={`${overflow.length} more filters`}
          >
            +{overflow.length} more
            <ChevronDown className={cn('size-3.5 transition-transform duration-150', open && 'rotate-180')} />
          </button>
        )}
      </div>

      {open && (
        <FloatingPortal>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            {...getFloatingProps()}
            className="z-50 w-64 rounded-lg border border-border bg-popover p-1.5 text-popover-foreground shadow-lg outline-none animate-in fade-in-0 zoom-in-95"
          >
            <div className="relative mb-1.5 px-0.5 pt-0.5">
              <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                ref={searchRef}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search filters..."
                className="h-8 pl-8 pr-7 text-xs"
              />
              {search && (
                <button
                  type="button"
                  onClick={() => setSearch('')}
                  className="absolute right-2 top-1/2 flex size-4 -translate-y-1/2 cursor-pointer items-center justify-center text-muted-foreground hover:text-foreground"
                >
                  <X className="size-3" />
                </button>
              )}
            </div>
            <div className="flex max-h-56 flex-col gap-0.5 overflow-y-auto pr-0.5">
              {filtered.length === 0 ? (
                <div className="py-4 text-center text-xs text-muted-foreground">No matching filters</div>
              ) : (
                filtered.map((t) => (
                  <button
                    key={t.id}
                    type="button"
                    onClick={() => {
                      onChange(t.id)
                      setOpen(false)
                      setSearch('')
                    }}
                    className={cn(
                      'flex w-full cursor-pointer items-center justify-between rounded-md px-2 py-1.5 text-left text-xs font-medium transition-colors',
                      value === t.id ? 'bg-primary/10 font-semibold text-primary' : 'text-foreground hover:bg-muted/70',
                    )}
                  >
                    <span className="mr-2 flex min-w-0 flex-1 items-center gap-2">
                      {t.icon}
                      <span className="truncate">{t.label}</span>
                    </span>
                    {t.badge !== undefined && <span className="shrink-0 rounded-full bg-muted px-1.5 text-[10px]">{t.badge}</span>}
                  </button>
                ))
              )}
            </div>
          </div>
        </FloatingPortal>
      )}
    </>
  )
}
