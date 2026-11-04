import { LayoutGrid, Search, X } from 'lucide-react'
import { type ReactNode, useMemo, useState } from 'react'

import { FA_ICON_CATEGORIES, FA_ICONS, FontAwesomeIcon } from '@/components/SiteLogo'
import { Select } from '@/components/ui/select'
import { cn } from '@/lib/utils'

/**
 * Two-pane layout for the offline icon pickers: a vertical category list on the left
 * and the icon grid (`children`) on the right, each scrolling on its own. Below the
 * `@xl` container width the list collapses into a dropdown above the grid, so a
 * narrow picker never loses grid space to 18 category rows.
 *
 * Each category shows its icon, its name and how many icons match the current search;
 * categories with no matches are dimmed so an empty result is visible before clicking.
 * The category list has its own search box (and the narrow dropdown is searchable),
 * which filters categories by name without touching the icon search.
 */
export function IconCategoryFilter({
  value,
  onChange,
  search = '',
  heightClassName = 'h-64',
  className,
  children,
}: {
  value: string
  onChange: (id: string) => void
  /** Current icon search text; counts reflect it. */
  search?: string
  /** Height shared by the category list and the grid pane. */
  heightClassName?: string
  className?: string
  /** The icon grid. It is placed in a scrolling pane, so it should not set its own height. */
  children: ReactNode
}) {
  const counts = useMemo(() => {
    const term = search.trim().toLowerCase()
    const map = new Map<string, number>()
    let all = 0
    for (const icon of FA_ICONS) {
      if (term && !icon.id.includes(term) && !icon.name.toLowerCase().includes(term)) continue
      all += 1
      map.set(icon.category, (map.get(icon.category) ?? 0) + 1)
    }
    map.set('all', all)
    return map
  }, [search])

  const [categoryQuery, setCategoryQuery] = useState('')
  const { visible, matched } = useMemo(() => {
    const term = categoryQuery.trim().toLowerCase()
    if (!term) return { visible: FA_ICON_CATEGORIES, matched: FA_ICON_CATEGORIES.length }
    const hits = FA_ICON_CATEGORIES.filter((c) => c.label.toLowerCase().includes(term) || c.id.includes(term))
    // Keep the selected category listed so the highlighted row never vanishes.
    const withSelected = hits.some((c) => c.id === value)
      ? hits
      : FA_ICON_CATEGORIES.filter((c) => c.id === value || hits.includes(c))
    return { visible: withSelected, matched: hits.length }
  }, [categoryQuery, value])

  return (
    <div className={cn('@container', className)}>
      <div className="flex flex-col gap-2 @xl:flex-row">
        {/* Narrow: searchable dropdown */}
        <Select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full @xl:hidden"
          aria-label="Icon category"
          searchable
          searchPlaceholder="Search categories…"
        >
          {FA_ICON_CATEGORIES.map((c) => (
            <option key={c.id} value={c.id}>
              {`${c.label} (${counts.get(c.id) ?? 0})`}
            </option>
          ))}
        </Select>

        {/* Wide: searchable vertical category list */}
        <div
          className={cn(
            'hidden w-48 shrink-0 flex-col overflow-hidden rounded-md border border-border bg-muted/20 @xl:flex',
            heightClassName,
          )}
        >
          <div className="relative shrink-0 border-b border-border p-1">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-3 -translate-y-1/2 text-muted-foreground" />
            <input
              value={categoryQuery}
              onChange={(e) => setCategoryQuery(e.target.value)}
              onKeyDown={(e) => {
                // Esc clears the box first instead of closing the surrounding dialog.
                if (e.key === 'Escape' && categoryQuery) {
                  e.stopPropagation()
                  setCategoryQuery('')
                }
              }}
              placeholder="Search categories…"
              aria-label="Search categories"
              className="h-7 w-full rounded bg-background/60 pl-7 pr-6 text-xs text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            />
            {categoryQuery && (
              <button
                type="button"
                onClick={() => setCategoryQuery('')}
                className="absolute right-2.5 top-1/2 flex size-4 -translate-y-1/2 cursor-pointer items-center justify-center text-muted-foreground hover:text-foreground"
                aria-label="Clear category search"
              >
                <X className="size-3" />
              </button>
            )}
          </div>
          <div
            role="tablist"
            aria-orientation="vertical"
            aria-label="Icon category"
            className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-1"
          >
            {visible.map((c) => {
              const count = counts.get(c.id) ?? 0
              const active = value === c.id
              return (
                <button
                  key={c.id}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => onChange(c.id)}
                  className={cn(
                    'flex w-full shrink-0 cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-left text-xs transition-colors',
                    active
                      ? 'bg-primary/15 font-medium text-primary'
                      : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                    !active && count === 0 && 'opacity-45',
                  )}
                >
                  {c.icon ? (
                    <FontAwesomeIcon name={c.icon} color="currentColor" className="size-3.5" />
                  ) : (
                    <LayoutGrid className="size-3.5 shrink-0" />
                  )}
                  <span className="min-w-0 flex-1 truncate">{c.label}</span>
                  <span className={cn('shrink-0 tabular-nums text-[10px]', active ? 'text-primary/80' : 'text-muted-foreground/70')}>
                    {count}
                  </span>
                </button>
              )
            })}
            {matched === 0 && (
              <p className="px-2 py-2 text-center text-[11px] text-muted-foreground">
                No category matches &ldquo;{categoryQuery.trim()}&rdquo;.
              </p>
            )}
          </div>
        </div>

        {/* Grid pane */}
        <div className={cn('min-w-0 flex-1 overflow-y-auto rounded-md border border-border p-2', heightClassName)}>
          {children}
        </div>
      </div>
    </div>
  )
}
