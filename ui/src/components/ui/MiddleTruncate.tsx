import { cn } from '@/lib/utils'

/**
 * Path / URL that shortens in the middle. The last segment (folder name) is never cut;
 * the head (prefix + drive + first folders) gets the ellipsis when space runs out.
 * Full value shows in the native tooltip.
 */
export function MiddleTruncate({
  text,
  prefix = '',
  title,
  className,
}: {
  text: string
  /** Fixed lead-in text that belongs to the head (e.g. "→ "). */
  prefix?: string
  title?: string
  className?: string
}) {
  const trimmed = text.replace(/[\\/]+$/, '')
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  const head = prefix + (cut > 0 ? trimmed.slice(0, cut) : '')
  const tail = cut > 0 ? trimmed.slice(cut) : trimmed
  return (
    <span className={cn('flex min-w-0 flex-1 overflow-hidden', className)} title={title ?? prefix + text}>
      {head && <span className="min-w-0 truncate">{head}</span>}
      <span className="shrink-0 whitespace-nowrap">{tail}</span>
    </span>
  )
}
