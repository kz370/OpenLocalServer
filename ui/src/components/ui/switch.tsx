import { cn } from '@/lib/utils'

/** An on/off switch. Renders a real button with `role="switch"` so keyboard and screen readers work. */
export function Switch({
  checked,
  onChange,
  disabled,
  size = 'md',
  label,
  className,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  disabled?: boolean
  size?: 'sm' | 'md'
  /** Accessible name when no visible label is next to it. */
  label?: string
  className?: string
}) {
  const sm = size === 'sm'
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative inline-flex shrink-0 items-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50',
        sm ? 'h-4 w-7' : 'h-5 w-9',
        checked ? 'bg-primary' : 'bg-muted-foreground/30',
        className,
      )}
    >
      <span
        className={cn(
          'pointer-events-none block rounded-full bg-white shadow transition-transform',
          sm ? 'size-3' : 'size-4',
          checked ? (sm ? 'translate-x-3.5' : 'translate-x-[1.125rem]') : 'translate-x-0.5',
        )}
      />
    </button>
  )
}
