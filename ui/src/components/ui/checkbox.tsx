import { Check } from 'lucide-react'

import { cn } from '@/lib/utils'

/** A styled checkbox for choosing rows from a list (use `Switch` for on/off options). */
export function Checkbox({
  checked,
  onChange,
  disabled,
  label,
  className,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  disabled?: boolean
  /** Accessible name when no visible label is next to it. */
  label?: string
  className?: string
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'inline-flex size-4 shrink-0 items-center justify-center rounded border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50',
        checked ? 'border-primary bg-primary text-primary-foreground' : 'border-muted-foreground/40 bg-input/60 hover:border-muted-foreground',
        className,
      )}
    >
      {checked && <Check className="size-3" strokeWidth={3} />}
    </button>
  )
}
