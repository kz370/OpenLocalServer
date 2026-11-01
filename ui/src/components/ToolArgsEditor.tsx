import { Plus, X } from 'lucide-react'
import { useRef } from 'react'

import { TechIcon } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { ToolArgs } from '@/core/types'
import {
  argsSections,
  getValidPlaceholderChips,
  isPerEngineArgs,
  joinArgs,
  toPerEngineArgs,
  toPlainArgs,
} from '@/lib/externalTools'
import { cn } from '@/lib/utils'

const ENGINE_LABELS: Record<string, string> = {
  mariadb: 'MariaDB',
  postgres: 'PostgreSQL',
  mongodb: 'MongoDB',
  redis: 'Redis',
  memcached: 'Memcached',
  sqlite: 'SQLite',
  default: 'Any other engine',
}

/**
 * One program argument per box, so a value with spaces or `|` needs no quoting. Clicking a
 * placeholder chip inserts it at the end of the box you last clicked, or adds a new box.
 */
function ArgList({
  value,
  onChange,
  engines,
}: {
  value: string[]
  onChange: (next: string[]) => void
  engines: string[]
}) {
  const focused = useRef<number | null>(null)
  const chips = getValidPlaceholderChips(engines)

  const setAt = (index: number, text: string) => onChange(value.map((a, i) => (i === index ? text : a)))

  const insert = (token: string) => {
    const at = focused.current
    if (at !== null && at < value.length) {
      setAt(at, value[at] + token)
    } else {
      onChange([...value, token])
      focused.current = value.length
    }
  }

  return (
    <div className="flex flex-col gap-2">
      {value.length === 0 && (
        <p className="text-xs text-muted-foreground">No arguments: the program is started on its own.</p>
      )}
      {value.map((arg, index) => (
        <div key={index} className="flex items-center gap-2">
          <span className="w-5 shrink-0 text-right text-[11px] tabular-nums text-muted-foreground">{index + 1}</span>
          <Input
            value={arg}
            onChange={(e) => setAt(index, e.target.value)}
            onFocus={() => (focused.current = index)}
            aria-label={`Argument ${index + 1}`}
            placeholder="e.g. -con"
            spellCheck={false}
            className="h-8 flex-1 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="size-8 shrink-0 p-0"
            aria-label={`Remove argument ${index + 1}`}
            title="Remove this argument"
            onClick={() => {
              focused.current = null
              onChange(value.filter((_, i) => i !== index))
            }}
          >
            <X className="size-3.5" />
          </Button>
        </div>
      ))}

      <div className="flex flex-wrap items-center gap-1.5 pl-7">
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 text-xs"
          onClick={() => {
            onChange([...value, ''])
            focused.current = value.length
          }}
        >
          <Plus className="size-3.5" /> Add argument
        </Button>
        <span className="px-1 text-[11px] text-muted-foreground">Insert:</span>
        {chips.map((c) => (
          <button
            key={c.token}
            type="button"
            title={c.label}
            // Keep focus in the box so the chip lands where the user was typing.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => insert(c.token)}
            className="rounded border border-border bg-card px-2 py-0.5 font-mono text-[11px] transition hover:border-primary hover:bg-primary/10"
          >
            {c.token}
          </button>
        ))}
      </div>

      {value.some((a) => a.trim()) && (
        <p className="pl-7 font-mono text-[11px] break-all text-muted-foreground">
          <span className="font-sans">Runs as: </span>
          program {joinArgs(value.filter((a) => a.trim()))}
        </p>
      )}
    </div>
  )
}

/**
 * Arguments editor for an external tool. "Same for every engine" is one list; "Different per
 * engine" shows one list for each ticked engine, for tools whose driver or URL differs.
 */
export function ToolArgsEditor({
  engines,
  value,
  onChange,
}: {
  engines: string[]
  value: ToolArgs
  onChange: (next: ToolArgs) => void
}) {
  const perEngine = isPerEngineArgs(value)
  const sections = argsSections(engines)

  const modeButton = (active: boolean, label: string, hint: string, onClick: () => void) => (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        'flex flex-1 flex-col rounded-lg border p-2 text-left text-xs transition',
        active ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border bg-card hover:bg-muted/30',
      )}
    >
      <span className="font-medium">{label}</span>
      <span className="mt-0.5 text-[11px] text-muted-foreground">{hint}</span>
    </button>
  )

  return (
    <div className="flex flex-col gap-3">
      <span className="text-xs font-medium text-muted-foreground">Arguments</span>
      <div className="flex gap-2">
        {modeButton(!perEngine, 'Same for every engine', 'One list of arguments.', () =>
          onChange(toPlainArgs(value)),
        )}
        {modeButton(perEngine, 'Different per engine', 'For tools that need a different driver or URL.', () =>
          onChange(toPerEngineArgs(value, sections)),
        )}
      </div>

      {perEngine ? (
        <div className="flex flex-col gap-3">
          {sections.map((section) => (
            <div key={section} className="flex flex-col gap-2 rounded-lg border border-border bg-muted/20 p-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                {section !== 'default' && <TechIcon id={section} className="size-4" />}
                {ENGINE_LABELS[section] ?? section}
              </div>
              <ArgList
                value={value[section] ?? []}
                onChange={(next) => onChange({ ...value, [section]: next })}
                engines={section === 'default' ? [] : [section]}
              />
            </div>
          ))}
          {engines.length === 0 && (
            <p className="text-[11px] text-muted-foreground">
              Tick the engines above to give each one its own arguments.
            </p>
          )}
        </div>
      ) : (
        <ArgList value={value as string[]} onChange={onChange} engines={engines} />
      )}
    </div>
  )
}

