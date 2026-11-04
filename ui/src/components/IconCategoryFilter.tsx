import { useMemo } from 'react'

import { FA_ICON_CATEGORIES, FA_ICONS } from '@/components/SiteLogo'
import { Select } from '@/components/ui/select'
import { cn } from '@/lib/utils'

/**
 * Category filter for the offline icon pickers. Wide containers show wrapped chips;
 * narrow ones (below the `@xl` container width) show a dropdown, so 18 options never
 * push the icon grid out of view. Each option shows how many icons match the search.
 */
export function IconCategoryFilter({
  value,
  onChange,
  search = '',
  className,
}: {
  value: string
  onChange: (id: string) => void
  /** Current search text; counts reflect it so empty categories are visible at a glance. */
  search?: string
  className?: string
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

  return (
    <div className={cn('@container', className)}>
      <div className="hidden flex-wrap gap-1.5 @xl:flex" role="tablist" aria-label="Icon category">
        {FA_ICON_CATEGORIES.map((c) => {
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
                'inline-flex cursor-pointer items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-medium transition',
                active
                  ? 'bg-primary text-primary-foreground'
                  : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground',
                !active && count === 0 && 'opacity-50',
              )}
            >
              {c.label}
              <span className={cn('tabular-nums text-[10px]', active ? 'text-primary-foreground/75' : 'text-muted-foreground/70')}>
                {count}
              </span>
            </button>
          )
        })}
      </div>
      <Select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full @xl:hidden"
        aria-label="Icon category"
      >
        {FA_ICON_CATEGORIES.map((c) => (
          <option key={c.id} value={c.id}>
            {`${c.label} (${counts.get(c.id) ?? 0})`}
          </option>
        ))}
      </Select>
    </div>
  )
}
