import { useEffect, useState } from 'react'

import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

/** The five cron fields, in the order the core's parser reads them. */
const FIELDS = [
  { key: 'minute', label: 'Min', hint: '0–59' },
  { key: 'hour', label: 'Hour', hint: '0–23' },
  { key: 'day', label: 'Day', hint: '1–31' },
  { key: 'month', label: 'Month', hint: '1–12' },
  { key: 'weekday', label: 'Day of week', hint: '0–6, 0 = Sunday' },
] as const

/** A stored schedule that is a name rather than an expression, shown as what it means. */
const NAME_TO_CRON: Record<string, string> = {
  every_minute: '* * * * *',
  hourly: '0 * * * *',
  daily: '0 0 * * *',
  weekly: '0 0 * * 0',
  monthly: '0 0 1 * *',
}

/** Splits a schedule into five field values, whatever form it was stored in. */
export function cronFields(schedule: string): string[] {
  const parts = (NAME_TO_CRON[schedule] ?? schedule).trim().split(/\s+/)
  return FIELDS.map((_, i) => parts[i] ?? '*')
}

export function joinCron(fields: string[]): string {
  return FIELDS.map((_, i) => fields[i]?.trim() || '*').join(' ')
}

/**
 * A cron schedule as five inputs rather than one line, because the single line is where
 * the mistakes happen: a user who cannot see the five fields cannot tell a missing one from
 * a wildcard. Nothing is written until a field loses focus, so opening the editor cannot
 * leave a schedule that does not parse.
 */
export function CronFields({
  value,
  onCommit,
  disabled,
  className,
}: {
  value: string
  onCommit: (expression: string) => void
  disabled?: boolean
  className?: string
}) {
  const [fields, setFields] = useState(() => cronFields(value))

  // Re-seed when the stored schedule changes under us, e.g. after a rejected expression.
  useEffect(() => setFields(cronFields(value)), [value])

  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      <div className="flex flex-wrap items-end gap-2">
        {FIELDS.map((field, i) => (
          <label key={field.key} className="flex flex-col gap-1">
            <span className="text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{field.label}</span>
            <Input
              aria-label={`${field.label} (cron)`}
              className="w-16 px-2 py-1 text-center font-mono"
              value={fields[i]}
              disabled={disabled}
              spellCheck={false}
              autoComplete="off"
              onChange={(e) => setFields((prev) => prev.map((f, j) => (j === i ? e.target.value : f)))}
              onBlur={() => {
                const next = joinCron(fields)
                if (next !== cronFields(value).join(' ')) onCommit(next)
              }}
            />
            <span className="text-[10px] text-muted-foreground">{field.hint}</span>
          </label>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">
        <code>{joinCron(fields)}</code> — each field accepts <code>*</code> for every value, and lists like{' '}
        <code>1,15</code> or <code>1-5</code>.
      </p>
    </div>
  )
}
