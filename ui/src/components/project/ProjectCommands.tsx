import { listen } from '@tauri-apps/api/event'
import { AlertTriangle, Blocks, Clipboard, History, Play, RefreshCw, SquareTerminal } from 'lucide-react'
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CommandSource, type DiscoveredCommand, type HistoryEntry, type ProcessEvent, type ProcessState, type QuickCommand, runCommand } from '@/core'
import { timeAgo, useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { waitForProcessExit } from '@/lib/wait'
import { CommandForm, SOURCE_ICON, StateBadge, commandLine, quote, sourceCache } from '@/pages/Commands'

/** A suggestion under the command box. */
interface Suggestion {
  key: string
  title: string
  detail: string
  pick: () => void
}

const PLACEHOLDER: Record<string, string> = {
  artisan: 'Type an artisan command: migrate, make:model Post, route:list…',
  console: 'Type a console command: cache:clear, make:controller…',
  composer: 'Type a Composer command: install, require vendor/package, dump-autoload…',
  scripts: 'Type a script name: dev, build, test…',
  django: 'Type a manage.py command: migrate, runserver, createsuperuser…',
  shell: 'Any command, run in the project folder: php -v, git log…',
  saved: 'Search your saved commands',
  recent: 'Search the commands you ran here',
}

/**
 * The site dialog's command box: pick a tool, type its command ("migrate", not
 * "php artisan migrate"), choose from the suggestions. The tool's prefix is added when it runs.
 */
export function ProjectCommands({ projectId, framework, onShowIssues }: { projectId: string; framework: string; onShowIssues: () => void }) {
  const [sources, setSources] = useState<CommandSource[]>(sourceCache.get(projectId) ?? [])
  const [loading, setLoading] = useState(false)
  const [quick, setQuick] = useState<QuickCommand[]>([])
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [mode, setMode] = useState('')
  const [text, setText] = useState('')
  const [open, setOpen] = useState(false)
  const [highlight, setHighlight] = useState(0)
  const [selected, setSelected] = useState<DiscoveredCommand | null>(null)
  const [output, setOutput] = useState<string[]>([])
  const [title, setTitle] = useState('')
  const [processId, setProcessId] = useState<number | null>(null)
  const [processState, setProcessState] = useState<ProcessState | null>(null)
  const { busy, error, setError, run } = useAction()
  const inputRef = useRef<HTMLInputElement>(null)
  const outRef = useRef<HTMLPreElement>(null)

  async function discover(force = false) {
    const cached = sourceCache.get(projectId)
    if (cached && !force) return setSources(cached)
    setLoading(true)
    try {
      const r = await runCommand({ type: 'discover_commands', project_id: projectId })
      if (r.type === 'command_sources') {
        sourceCache.set(projectId, r.sources)
        setSources(r.sources)
      }
    } finally {
      setLoading(false)
    }
  }

  async function refresh() {
    const [c, h] = await Promise.all([runCommand({ type: 'list_quick_commands' }), runCommand({ type: 'list_history' })])
    if (c.type === 'quick_commands') setQuick(c.commands.filter((q) => q.applies_to.length === 0 || q.applies_to.includes(framework)))
    if (h.type === 'history') setHistory(h.entries.filter((e) => e.project_id === projectId))
  }

  useEffect(() => {
    void run('discover', () => discover())
    refresh().catch(() => undefined)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])

  useEffect(() => {
    const un = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (processId === null || e.id !== processId) return
      if (e.kind === 'output') setOutput((prev) => [...prev.slice(-1999), e.line])
      else setProcessState(e.state)
    })
    return () => {
      un.then((f) => f())
    }
  }, [processId])

  useEffect(() => {
    outRef.current?.scrollTo({ top: outRef.current.scrollHeight })
  }, [output.length])

  const modes: { id: string; label: string; icon: ReactNode; badge?: number }[] = [
    ...sources.map((s) => ({ id: s.id, label: s.label, icon: <TechIcon id={SOURCE_ICON[s.id] ?? s.prefix[0]} />, badge: s.commands.length })),
    { id: 'shell', label: 'Shell', icon: <SquareTerminal className="size-4" /> },
    { id: 'saved', label: 'Saved', icon: <Blocks className="size-4" />, badge: quick.length },
    { id: 'recent', label: 'Recent', icon: <History className="size-4" />, badge: history.length },
  ]
  const active = modes.some((m) => m.id === mode) ? mode : (modes[0]?.id ?? 'shell')
  const source = sources.find((s) => s.id === active) ?? null

  function switchMode(id: string) {
    setMode(id)
    setText('')
    setSelected(null)
    setOpen(false)
    inputRef.current?.focus()
  }

  async function follow(res: Awaited<ReturnType<typeof runCommand>>, label: string) {
    const id = res.type === 'process_started' ? res.id : res.type === 'maybe_process' ? res.id : null
    if (id !== null) {
      setOutput([])
      setTitle(label)
      setProcessState('starting')
      setProcessId(id)
    }
    await refresh()
  }

  /** Runs `shown` (what the user sees) as `full` (with the tool's prefix). */
  const runLine = (full: string, shown: string) =>
    run('line', async () => {
      setOpen(false)
      await follow(await runCommand({ type: 'run_command_line', line: full, cwd: null, project_id: projectId }), shown)
    })

  const withPrefix = (line: string) => (source ? [...source.prefix.map(quote), line].join(' ') : line)

  const term = text.trim().toLowerCase()
  const firstWord = term.split(/\s+/)[0] ?? ''
  const suggestions: Suggestion[] = useMemo(() => {
    if (source) {
      if (!firstWord || term.includes(' ')) return []
      const hits = source.commands.filter((c) => c.name.toLowerCase().includes(firstWord) || c.description.toLowerCase().includes(firstWord))
      // Names that start with what was typed come first, then other name matches, then descriptions.
      const rank = (c: DiscoveredCommand) => (c.name.toLowerCase().startsWith(firstWord) ? 0 : c.name.toLowerCase().includes(firstWord) ? 1 : 2)
      return hits
        .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
        .slice(0, 8)
        .map((c) => ({
          key: c.name,
          title: c.name,
          detail: c.description,
          pick: () => {
            setText(`${c.name} `)
            setSelected(c)
            setOpen(false)
            inputRef.current?.focus()
          },
        }))
    }
    if (active === 'saved')
      return quick
        .filter((q) => !term || `${q.name} ${q.description} ${commandLine(q)}`.toLowerCase().includes(term))
        .slice(0, 8)
        .map((q) => ({
          key: q.id,
          title: q.name,
          detail: q.description || commandLine(q),
          pick: () => {
            setText('')
            setOpen(false)
            void run(q.id, async () => follow(await runCommand({ type: 'run_quick_command', id: q.id, project_id: projectId }), q.name))
          },
        }))
    if (active === 'recent')
      return history
        .filter((h) => !term || h.line.toLowerCase().includes(term))
        .slice(0, 8)
        .map((h) => ({
          key: String(h.id),
          title: h.line,
          detail: timeAgo(h.timestamp_ms),
          pick: () => {
            setText(h.line)
            setOpen(false)
            inputRef.current?.focus()
          },
        }))
    return []
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, active, term, firstWord, quick, history])

  const showList = open && suggestions.length > 0

  function submit() {
    if (showList) return suggestions[Math.min(highlight, suggestions.length - 1)].pick()
    const line = text.trim()
    if (!line || active === 'saved') return
    void runLine(active === 'recent' ? line : withPrefix(line), line)
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (!open) return setOpen(true)
      const n = suggestions.length
      if (n) setHighlight((h) => (h + (e.key === 'ArrowDown' ? 1 : n - 1)) % n)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      submit()
    } else if (e.key === 'Tab' && showList) {
      e.preventDefault()
      suggestions[Math.min(highlight, suggestions.length - 1)].pick()
    } else if (e.key === 'Escape' && showList) {
      // Close the suggestions, not the whole dialog.
      e.stopPropagation()
      setOpen(false)
    }
  }

  const running = processId !== null && (processState === 'starting' || processState === 'running' || processState === 'stopping')

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="flex items-end justify-between gap-2">
        <Tabs tabs={modes} value={active} onChange={switchMode} />
        <Button variant="ghost" size="icon" title="Re-read the project's commands" disabled={loading} onClick={() => run('discover', () => discover(true))}>
          {loading ? <Spinner /> : <RefreshCw />}
        </Button>
      </div>

      {/* Only when there is nothing to suggest; the details live in Logs & issues. */}
      {source?.error && (
        <button className="flex items-center gap-1.5 self-start text-xs text-muted-foreground hover:text-foreground" onClick={onShowIssues}>
          <AlertTriangle className="size-3.5 text-warning" /> The {source.label} commands could not be read. See Logs &amp; issues.
        </button>
      )}

      <div className="relative">
        <div className="flex items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <span className="pointer-events-none absolute left-3 top-1/2 flex -translate-y-1/2 text-muted-foreground [&_svg]:size-4">{modes.find((m) => m.id === active)?.icon}</span>
            <Input
              ref={inputRef}
              value={text}
              onChange={(e) => {
                setText(e.target.value)
                setHighlight(0)
                setOpen(true)
                if (selected && e.target.value.trim().split(/\s+/)[0] !== selected.name) setSelected(null)
              }}
              onFocus={() => setOpen(true)}
              onBlur={() => setTimeout(() => setOpen(false), 150)}
              onKeyDown={onKeyDown}
              placeholder={PLACEHOLDER[active] ?? `Type a ${source?.label ?? ''} command`}
              className="pl-9 font-mono"
              role="combobox"
              aria-expanded={showList}
              aria-autocomplete="list"
              autoFocus
            />
          </div>
          <Button disabled={!text.trim() || busy !== null || active === 'saved'} onClick={submit}>
            {busy === 'line' ? <Spinner /> : <Play />} Run
          </Button>
        </div>

        {showList && (
          <div role="listbox" className="absolute left-0 right-24 top-full z-20 mt-1 max-h-72 overflow-y-auto rounded-lg border border-border bg-popover p-1 shadow-lg">
            {suggestions.map((s, i) => (
              <button
                key={s.key}
                role="option"
                aria-selected={i === highlight}
                // mousedown, so the input's blur doesn't close the list first.
                onMouseDown={(e) => {
                  e.preventDefault()
                  s.pick()
                }}
                onMouseEnter={() => setHighlight(i)}
                className={cn('flex w-full min-w-0 items-baseline gap-3 rounded-md px-2.5 py-1.5 text-left', i === highlight && 'bg-accent')}
              >
                <span className="shrink-0 font-mono text-[13px]">{s.title}</span>
                <span className="min-w-0 truncate text-xs text-muted-foreground">{s.detail}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {source && selected && (
        <CommandForm
          key={`${source.id}:${selected.name}`}
          source={source}
          command={selected}
          busy={busy !== null}
          needsProject={false}
          hidePrefix
          onRun={(full) => void runLine(full, full.split(' ').slice(source.prefix.length).join(' '))}
          onEdit={(shown) => {
            setText(shown)
            inputRef.current?.focus()
          }}
          onSave={() => undefined}
        />
      )}

      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2 text-sm font-medium">
            <span className="truncate font-mono">{title ? title : 'Output'}</span>
            {processState && <StateBadge state={processState} />}
          </div>
          <div className="flex shrink-0 gap-1">
            {output.length > 0 && (
              <Button size="sm" variant="ghost" title="Copy output" onClick={() => navigator.clipboard.writeText(output.join('\n'))}>
                <Clipboard className="size-3.5" />
              </Button>
            )}
            {running && processId !== null && (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy === 'stop'}
                onClick={() =>
                  run('stop', async () => {
                    await runCommand({ type: 'stop_process', id: processId })
                    await waitForProcessExit(processId)
                  })
                }
              >
                {busy === 'stop' ? <Spinner /> : <StopIcon />} {busy === 'stop' ? 'Stopping…' : 'Stop'}
              </Button>
            )}
          </div>
        </div>
        <pre ref={outRef} className="h-64 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-lg bg-muted/40 p-3 font-mono text-xs">
          {output.join('\n') || 'Run something to see its output here.'}
        </pre>
      </div>
    </div>
  )
}
