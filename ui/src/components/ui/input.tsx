import { ChevronDown, ChevronUp } from 'lucide-react'
import * as React from 'react'

import { cn } from '@/lib/utils'

const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  ({ className, type, ...props }, ref) => (
    <input
      type={type}
      className={cn(
        'flex h-9 w-full rounded-lg border border-transparent bg-input/60 px-3.5 py-1 text-sm transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      ref={ref}
      {...props}
    />
  ),
)
Input.displayName = 'Input'

export interface NumberInputProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'value' | 'onChange'> {
  /** null means "empty" — the field is allowed to hold nothing. */
  value: number | null
  onChange: (value: number | null) => void
  min?: number
  max?: number
  step?: number
  /** Names the field for the stepper buttons' screen-reader labels. */
  label?: string
}

/**
 * A number field with our own stepper. The native `type="number"` arrows are a
 * browser widget: they ignore the app's radius, border and dark palette, and they
 * appear only on hover in Chromium, so a screenshot of the form never shows them.
 * This is a text input with `inputMode="numeric"` plus two chevron buttons, which
 * means the same affordance is always visible, on every platform.
 */
const NumberInput = React.forwardRef<HTMLInputElement, NumberInputProps>(
  ({ className, value, onChange, min, max, step = 1, label = 'value', id, disabled, ...props }, ref) => {
    // The field keeps its own text while focused so a half-typed number is not
    // rewritten under the cursor; the number is parsed on every keystroke.
    const [text, setText] = React.useState(value === null ? '' : String(value))
    const [editing, setEditing] = React.useState(false)
    // Set by the stepper (arrow keys and the chevron buttons). The field stays
    // focused across a step, so without this the props change lands but the text
    // is left behind and the box shows the old number until blur.
    const stepped = React.useRef(false)
    React.useEffect(() => {
      if (!editing || stepped.current) {
        stepped.current = false
        setText(value === null ? '' : String(value))
      }
    }, [value, editing])

    const clamp = (n: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, n))

    const commit = (raw: string) => {
      setText(raw)
      if (raw.trim() === '' || raw.trim() === '-') {
        onChange(null)
        return
      }
      const n = Number(raw)
      if (!Number.isFinite(n)) return
      onChange(Math.trunc(n))
    }

    const bump = (dir: 1 | -1) => {
      if (disabled) return
      // Step from the number on screen, so the text, the value and the stepper
      // never disagree while a half-typed number sits in the field.
      const shown = text.trim() === '' || text.trim() === '-' ? NaN : Number(text)
      const base = Number.isFinite(shown) ? shown : (value ?? min ?? 0)
      const next = Math.trunc(clamp(base + dir * step))
      stepped.current = true
      setText(String(next))
      onChange(next)
    }

    return (
      <div className="relative inline-flex w-full min-w-0 items-center">
        <input
          id={id}
          ref={ref}
          type="text"
          inputMode="numeric"
          pattern="[0-9]*"
          role="spinbutton"
          aria-valuenow={value ?? undefined}
          aria-valuemin={min}
          aria-valuemax={max}
          aria-label={props['aria-label'] ?? (label === 'value' ? undefined : label)}
          autoComplete="off"
          spellCheck={false}
          value={text}
          disabled={disabled}
          onFocus={() => {
            stepped.current = false
            setEditing(true)
          }}
          onBlur={() => {
            stepped.current = false
            setEditing(false)
            if (text.trim() !== '' && Number.isFinite(Number(text))) setText(String(Math.trunc(Number(text))))
          }}
          onChange={(e) => commit(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowUp') {
              e.preventDefault()
              bump(1)
            } else if (e.key === 'ArrowDown') {
              e.preventDefault()
              bump(-1)
            }
          }}
          className={cn(
            'flex h-9 w-full rounded-lg border border-transparent bg-input/60 py-1 text-sm transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50',
            // Right padding leaves room for the stepper; the arrows are gone for good.
            'px-3.5 pr-8 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
            className,
          )}
          {...props}
        />
        <span className="absolute right-1.5 top-1/2 flex -translate-y-1/2 flex-col gap-px">
          <StepButton
            label={`Increase ${label}`}
            disabled={disabled || (max !== undefined && value !== null && value >= max)}
            onClick={() => bump(1)}
          >
            <ChevronUp className="size-3" strokeWidth={2.5} />
          </StepButton>
          <StepButton
            label={`Decrease ${label}`}
            disabled={disabled || (min !== undefined && value !== null && value <= min)}
            onClick={() => bump(-1)}
          >
            <ChevronDown className="size-3" strokeWidth={2.5} />
          </StepButton>
        </span>
      </div>
    )
  },
)
NumberInput.displayName = 'NumberInput'

/** Keeps the field focused while the button is pressed — a stepper that blurs the input loses the caret. */
function StepButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string
  disabled: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      disabled={disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className="flex size-3.5 items-center justify-center rounded-[4px] text-muted-foreground/80 transition-colors hover:bg-muted hover:text-foreground active:bg-accent active:text-accent-foreground disabled:pointer-events-none disabled:opacity-30"
    >
      {children}
    </button>
  )
}

export { Input, NumberInput }
