import * as React from 'react'

import { cn } from '@/lib/utils'

import { Switch } from './switch'

const fieldBase =
  'flex w-full rounded-lg border border-transparent bg-input/60 px-3.5 text-sm transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50'

export const Select = React.forwardRef<HTMLSelectElement, React.SelectHTMLAttributes<HTMLSelectElement>>(
  ({ className, children, ...props }, ref) => (
    <select ref={ref} className={cn(fieldBase, 'h-9 py-1', className)} {...props}>
      {children}
    </select>
  ),
)
Select.displayName = 'Select'

export const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(
  ({ className, ...props }, ref) => <textarea ref={ref} className={cn(fieldBase, 'min-h-20 py-2 font-mono', className)} {...props} />,
)
Textarea.displayName = 'Textarea'

/** A labelled on/off setting: text on the left, a switch on the right. */
export function Toggle({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label: string
  hint?: string
  disabled?: boolean
}) {
  return (
    <label className={cn('flex cursor-pointer items-start justify-between gap-4 text-sm', disabled && 'cursor-not-allowed opacity-50')}>
      <span className="min-w-0">
        {label}
        {hint && <span className="block text-xs text-muted-foreground">{hint}</span>}
      </span>
      <Switch checked={checked} onChange={onChange} disabled={disabled} label={label} className="mt-0.5" />
    </label>
  )
}

/** One setting in a settings card: title and hint on the left, its control on the right. */
export function SettingRow({
  title,
  hint,
  children,
  stacked,
}: {
  title: string
  hint?: string
  children: React.ReactNode
  /** Put the control under the text instead of beside it (wide controls such as paths). */
  stacked?: boolean
}) {
  return (
    <div className={cn('flex gap-4 border-b border-border py-3.5 first:pt-0 last:border-b-0 last:pb-0', stacked ? 'flex-col' : 'items-center justify-between')}>
      <div className="min-w-0">
        <div className="text-sm font-medium">{title}</div>
        {hint && <div className="mt-0.5 text-xs text-muted-foreground">{hint}</div>}
      </div>
      <div className={cn(stacked ? 'w-full' : 'shrink-0')}>{children}</div>
    </div>
  )
}

/** A switch as a settings row. */
export function SwitchRow({ checked, onChange, title, hint, disabled }: { checked: boolean; onChange: (v: boolean) => void; title: string; hint?: string; disabled?: boolean }) {
  return (
    <SettingRow title={title} hint={hint}>
      <Switch checked={checked} onChange={onChange} disabled={disabled} label={title} />
    </SettingRow>
  )
}

export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string
  hint?: string
  error?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">{label}</label>
      {children}
      {error ? <p className="text-xs text-destructive">{error}</p> : hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: { id: T; label: string; badge?: string | number; icon?: React.ReactNode }[]
  value: T
  onChange: (id: T) => void
}) {
  return (
    <div className="flex flex-wrap gap-0.5 shadow-[inset_0_-1px_0_var(--border)]">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onChange(t.id)}
          className={cn(
            'flex shrink-0 items-center gap-1.5 whitespace-nowrap border-b-2 px-2.5 py-2 text-sm font-medium transition-colors',
            value === t.id ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground',
          )}
        >
          {t.icon}
          {t.label}
          {t.badge !== undefined && <span className="rounded-full bg-muted px-1.5 text-[11px]">{t.badge}</span>}
        </button>
      ))}
    </div>
  )
}
