import { listen } from '@tauri-apps/api/event'
import { Clipboard, Pencil, Play, Plus, RefreshCw, Save, Search, SquareTerminal, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { TechIcon } from '@/components/TechIcon'
import { Field, Select, Tabs, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CommandSource, type DiscoveredCommand, type HistoryEntry, type ProcessEvent, type ProcessState, type Project, type QuickCommand, runCommand } from '@/core'
import { timeAgo, useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { waitForProcessExit } from '@/lib/wait'
import { confirmThen } from '@/lib/confirm'

const CATEGORY_ORDER = ['custom', 'laravel', 'php', 'node', 'python', 'tools']
const CATEGORY_LABELS: Record<string, string> = {
  custom: 'Yours',
  laravel: 'Laravel',
  php: 'PHP',
  node: 'Node',
  python: 'Python',
  tools: 'Tools',
  other: 'Other',
}
const categoryRank = (c: string) => {
  const i = CATEGORY_ORDER.indexOf(c)
  return i === -1 ? CATEGORY_ORDER.length : i
}
const categoryLabel = (c: string) => CATEGORY_LABELS[c] ?? c.charAt(0).toUpperCase() + c.slice(1)

/** Brand icon for each discovered source. */
const SOURCE_ICON: Record<string, string> = {
  artisan: 'laravel',
  console: 'symfony',
  composer: 'composer',
  django: 'django',
}
/** The Quick Command category a source's saved commands land in. */
const SOURCE_CATEGORY: Record<string, string> = {
  artisan: 'laravel',
  console: 'php',
  composer: 'php',
  django: 'python',
  scripts: 'node',
}
const FRAMEWORKS = ['laravel', 'symfony', 'wordpress', 'generic_php', 'node', 'django', 'flask', 'fast_api', 'generic_python']

/** Discovery runs the project's tools, which takes a second or two: keep it for the session. */
const sourceCache = new Map<string, CommandSource[]>()

/** Quotes one argument the way the backend's command-line splitter reads it back. */
function quote(arg: string): string {
  return arg === '' || /[\s"']/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg
}

/** The inverse of `quote` for a whole line: whitespace splits, "..." and '...' group. */
function splitLine(line: string): string[] {
  const out: string[] = []
  let cur = ''
  let q: string | null = null
  let started = false
  for (let i = 0; i < line.length; i++) {
    const c = line[i]
    if (q) {
      if (c === q) q = null
      else if (q === '"' && c === '\\' && line[i + 1] === '"') cur += line[++i]
      else cur += c
    } else if (c === '"' || c === "'") {
      q = c
      started = true
    } else if (/\s/.test(c)) {
      if (started || cur) out.push(cur)
      cur = ''
      started = false
    } else cur += c
  }
  if (started || cur) out.push(cur)
  return out
}

const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48) || 'command'

const commandLine = (c: QuickCommand) => (c.command?.executable ? [c.command.executable, ...c.command.arguments].map(quote).join(' ') : '')

/** "make:model" is in "make"; commands without a colon are grouped as general. */
const namespaceOf = (name: string) => (name.includes(':') ? name.slice(0, name.indexOf(':')) : '')

type Selection = { kind: 'discovered'; source: string; name: string } | { kind: 'quick'; id: string } | null

/** §89–93, Plesk-style: every command the project's tools offer, custom commands, a free runner and history. */
export function CommandsPage() {
  const [projects, setProjects] = useState<Project[]>([])
  const [projectId, setProjectId] = useState<string>('')
  const [framework, setFramework] = useState<string>('')
  const [sources, setSources] = useState<CommandSource[]>([])
  const [loadingSources, setLoadingSources] = useState(false)
  const [commands, setCommands] = useState<QuickCommand[]>([])
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [tab, setTab] = useState('')
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<Selection>(null)
  const [line, setLine] = useState('')
  const [output, setOutput] = useState<string[]>([])
  const [title, setTitle] = useState('')
  const [processId, setProcessId] = useState<number | null>(null)
  const [processState, setProcessState] = useState<ProcessState | null>(null)
  const [editing, setEditing] = useState<QuickCommand | null>(null)
  const { busy, error, setError, run } = useAction()
  const outRef = useRef<HTMLPreElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)
  const lineRef = useRef<HTMLInputElement>(null)

  async function refresh() {
    const [c, h] = await Promise.all([runCommand({ type: 'list_quick_commands' }), runCommand({ type: 'list_history' })])
    if (c.type === 'quick_commands') setCommands(c.commands)
    if (h.type === 'history') setHistory(h.entries)
  }

  async function loadSources(id: string, force = false) {
    if (!id) return setSources([])
    const cached = sourceCache.get(id)
    if (cached && !force) return setSources(cached)
    setLoadingSources(true)
    try {
      const r = await runCommand({ type: 'discover_commands', project_id: id })
      if (r.type === 'command_sources') {
        sourceCache.set(id, r.sources)
        setSources(r.sources)
      }
    } catch (e) {
      setSources([])
      throw e
    } finally {
      setLoadingSources(false)
    }
  }

  function pickProject(id: string) {
    if (id === projectId) return
    setProjectId(id)
    setSelected(null)
    setFramework('')
    setSources(id ? (sourceCache.get(id) ?? []) : [])
    if (!id) return
    runCommand({ type: 'get_project_detail', id }).then((r) => r.type === 'project_detail' && setFramework(r.detail.detection.framework))
    void run('discover', () => loadSources(id))
  }

  useEffect(() => {
    runCommand({ type: 'list_projects' }).then((r) => {
      if (r.type === 'projects') {
        setProjects(r.projects)
        if (r.projects[0]) pickProject(r.projects[0].id)
      }
    })
    refresh().catch(() => undefined)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

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

  // "/" jumps to the search box, like most command palettes.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement
      if (e.key === '/' && !['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName)) {
        e.preventDefault()
        searchRef.current?.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const visibleQuick = commands.filter((c) => c.applies_to.length === 0 || c.applies_to.includes(framework))
  const q = query.trim().toLowerCase()
  const matches = (...fields: string[]) => !q || fields.some((f) => f.toLowerCase().includes(q))
  const quickShown = visibleQuick.filter((c) => matches(c.name, c.description, commandLine(c)))
  const historyShown = history.filter((h) => matches(h.line))
  const sourceShown = (s: CommandSource) => s.commands.filter((c) => matches(c.name, c.description))

  const tabs = [
    ...sources.map((s) => ({
      id: s.id,
      label: s.label,
      icon: <TechIcon id={SOURCE_ICON[s.id] ?? s.prefix[0]} />,
      badge: sourceShown(s).length,
    })),
    {
      id: 'custom',
      label: 'Quick & custom',
      icon: <TechIcon id="custom" />,
      badge: quickShown.length,
    },
    { id: 'recent', label: 'Recent', badge: historyShown.length },
  ]
  const activeTab = tabs.some((t) => t.id === tab) ? tab : (tabs[0]?.id ?? 'custom')
  const activeSource = sources.find((s) => s.id === activeTab) ?? null

  const selectedDiscovered = useMemo(() => {
    if (selected?.kind !== 'discovered') return null
    const source = sources.find((s) => s.id === selected.source)
    const command = source?.commands.find((c) => c.name === selected.name)
    return source && command ? { source, command } : null
  }, [selected, sources])
  const selectedQuick = selected?.kind === 'quick' ? (commands.find((c) => c.id === selected.id) ?? null) : null

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

  const runQuick = (c: QuickCommand) =>
    run(c.id, async () =>
      follow(
        await runCommand({
          type: 'run_quick_command',
          id: c.id,
          project_id: projectId || null,
        }),
        c.name,
      ),
    )
  const runLine = (text: string, pid = projectId) =>
    run('line', async () =>
      follow(
        await runCommand({
          type: 'run_command_line',
          line: text,
          cwd: null,
          project_id: pid || null,
        }),
        text,
      ),
    )

  const running = processId !== null && (processState === 'starting' || processState === 'running' || processState === 'stopping')

  const newCustom = (lineText = '', name = '', category = 'custom', appliesTo: string[] = []): QuickCommand => {
    const [executable = '', ...rest] = splitLine(lineText)
    return {
      id: '',
      name,
      description: '',
      category,
      applies_to: appliesTo,
      working_directory: '{{project_path}}',
      command: { executable, arguments: rest },
      environment: { use_project_runtime: true },
      action: null,
      with: {},
      builtin: false,
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Commands</h1>
          <p className="text-sm text-muted-foreground">Every command your project's tools offer, plus your own. Pick one, fill in the blanks, run it.</p>
        </div>
        <div className="flex items-end gap-2">
          <Field label="Project">
            <Select value={projectId} onChange={(e) => pickProject(e.target.value)} className="w-64">
              <option value="">— none (global) —</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </Field>
          <Button variant="ghost" size="icon" title="Re-read the project's commands" disabled={!projectId || loadingSources} onClick={() => run('discover', () => loadSources(projectId, true))}>
            {loadingSources ? <Spinner /> : <RefreshCw />}
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <Card>
        <CardContent className="flex items-center gap-2 pt-4">
          <SquareTerminal className="size-4 shrink-0 text-muted-foreground" />
          <Input
            ref={lineRef}
            value={line}
            onChange={(e) => setLine(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && line.trim() && runLine(line)}
            placeholder="Type any command: php artisan tinker · npm run build · composer require x/y"
            className="font-mono"
          />
          <Button disabled={!line.trim() || busy !== null} onClick={() => runLine(line)}>
            <Play /> Run
          </Button>
          <Button variant="ghost" size="icon" title="Save as a custom command" disabled={!line.trim()} onClick={() => setEditing(newCustom(line, line.slice(0, 40)))}>
            <Save />
          </Button>
        </CardContent>
      </Card>

      <div className="grid items-start gap-4 lg:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
        <Card className="min-w-0">
          <CardContent className="flex flex-col gap-3 pt-4">
            <Tabs tabs={tabs} value={activeTab} onChange={(t) => setTab(t)} />
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input ref={searchRef} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search commands   ( / )" className="pl-9" />
            </div>

            <div className="-mx-1 max-h-[62vh] overflow-y-auto px-1">
              {activeSource && (
                <SourceList
                  source={activeSource}
                  commands={sourceShown(activeSource)}
                  selected={selectedDiscovered?.command.name ?? null}
                  onSelect={(name) =>
                    setSelected({
                      kind: 'discovered',
                      source: activeSource.id,
                      name,
                    })
                  }
                />
              )}

              {activeTab === 'custom' && (
                <QuickList commands={quickShown} selected={selectedQuick?.id ?? null} onSelect={(id) => setSelected({ kind: 'quick', id })} onNew={() => setEditing(newCustom('', '', 'custom', []))} />
              )}

              {activeTab === 'recent' && (
                <div className="flex flex-col gap-1.5">
                  {history.length > 0 && (
                    <div className="flex justify-end">
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          confirmThen('Clear the whole command history?', () =>
                            run('clear', async () => {
                              await runCommand({ type: 'clear_history' })
                              await refresh()
                            }),
                          )
                        }
                      >
                        Clear history
                      </Button>
                    </div>
                  )}
                  {historyShown.slice(0, 100).map((h) => (
                    <div key={h.id} className="group flex items-center justify-between gap-2 rounded-lg border border-border px-3 py-1.5">
                      <div className="min-w-0">
                        <code className="block truncate text-xs">{h.line}</code>
                        <span className="text-[11px] text-muted-foreground">
                          {timeAgo(h.timestamp_ms)}
                          {h.project_id && ` · ${projects.find((p) => p.id === h.project_id)?.name ?? ''}`}
                        </span>
                      </div>
                      <div className="flex shrink-0">
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Run again"
                          onClick={() => {
                            pickProject(h.project_id ?? '')
                            void runLine(h.line, h.project_id ?? '')
                          }}
                        >
                          <Play className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Edit in the command line"
                          onClick={() => {
                            setLine(h.line)
                            lineRef.current?.focus()
                          }}
                        >
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button size="sm" variant="ghost" title="Save as a custom command" onClick={() => setEditing(newCustom(h.line, h.line.slice(0, 40)))}>
                          <Save className="size-3.5" />
                        </Button>
                        <Button size="sm" variant="ghost" title="Copy" onClick={() => navigator.clipboard.writeText(h.line)}>
                          <Clipboard className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Delete"
                          onClick={() =>
                            confirmThen(`Delete "${h.line}" from history?`, () =>
                              run('delh', async () => {
                                await runCommand({
                                  type: 'delete_history',
                                  id: h.id,
                                })
                                await refresh()
                              }),
                            )
                          }
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </div>
                    </div>
                  ))}
                  {historyShown.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">{q ? 'Nothing in the history matches.' : 'Commands you run appear here.'}</p>}
                </div>
              )}

              {!projectId && activeTab === 'custom' && <p className="pt-3 text-xs text-muted-foreground">Pick a project to also see its artisan, composer, package.json and manage.py commands.</p>}
              {projectId && loadingSources && sources.length === 0 && (
                <p className="flex items-center gap-2 pt-3 text-xs text-muted-foreground">
                  <Spinner /> Reading the project's commands…
                </p>
              )}
            </div>
          </CardContent>
        </Card>

        <div className="flex min-w-0 flex-col gap-4">
          {selectedDiscovered ? (
            <CommandForm
              key={`${selectedDiscovered.source.id}:${selectedDiscovered.command.name}`}
              source={selectedDiscovered.source}
              command={selectedDiscovered.command}
              busy={busy !== null}
              needsProject={!projectId}
              onRun={(text) => runLine(text)}
              onEdit={(text) => {
                setLine(text)
                lineRef.current?.focus()
              }}
              onSave={(text) =>
                setEditing(
                  newCustom(
                    text,
                    selectedDiscovered.command.name,
                    SOURCE_CATEGORY[selectedDiscovered.source.id] ?? 'custom',
                    framework && selectedDiscovered.source.id !== 'scripts' ? [framework] : [],
                  ),
                )
              }
            />
          ) : selectedQuick ? (
            <QuickDetail
              command={selectedQuick}
              busy={busy !== null}
              needsProject={selectedQuick.applies_to.length > 0 && !projectId}
              onRun={() => runQuick(selectedQuick)}
              onEdit={() => setEditing(selectedQuick)}
              onDelete={() =>
                confirmThen(
                  commands.some((c) => c.id === selectedQuick.id && !c.builtin)
                    ? `Delete "${selectedQuick.name}"? A built-in command with the same id comes back as it was.`
                    : `Delete "${selectedQuick.name}"?`,
                  () =>
                    run('del', async () => {
                      await runCommand({
                        type: 'delete_quick_command',
                        id: selectedQuick.id,
                      })
                      setSelected(null)
                      await refresh()
                    }),
                )
              }
            />
          ) : (
            <Card>
              <CardContent className="flex flex-col items-center gap-2 py-10 text-center">
                <SquareTerminal className="size-8 text-muted-foreground" />
                <p className="text-sm font-medium">Pick a command on the left</p>
                <p className="max-w-sm text-xs text-muted-foreground">Its arguments and options turn into a form here, with the exact command line shown before you run it.</p>
              </CardContent>
            </Card>
          )}

          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="flex min-w-0 items-center gap-2 text-sm">
                <span className="truncate">Output{title && `: ${title}`}</span>
                {processState && <StateBadge state={processState} />}
              </CardTitle>
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
                        await runCommand({
                          type: 'stop_process',
                          id: processId,
                        })
                        await waitForProcessExit(processId)
                      })
                    }
                  >
                    {busy === 'stop' ? <Spinner /> : <StopIcon />} {busy === 'stop' ? 'Stopping…' : 'Stop'}
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent>
              <pre ref={outRef} className="h-72 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-lg bg-muted/40 p-3 font-mono text-xs">
                {output.join('\n') || 'Run something to see its output here.'}
              </pre>
            </CardContent>
          </Card>
        </div>
      </div>

      {editing && (
        <CustomCommandDialog
          key={editing.id || 'new'}
          command={editing}
          existingIds={commands.map((c) => c.id)}
          busy={busy !== null}
          onClose={() => setEditing(null)}
          onSave={(cmd) =>
            run('save', async () => {
              await runCommand({ type: 'save_quick_command', command: cmd })
              setEditing(null)
              await refresh()
              setTab('custom')
              setSelected({ kind: 'quick', id: cmd.id })
            })
          }
        />
      )}
    </div>
  )
}

function StateBadge({ state }: { state: ProcessState }) {
  const label: Record<string, string> = {
    starting: 'starting',
    running: 'running',
    stopping: 'stopping',
    stopped: 'finished',
    crashed: 'failed',
    failed: 'failed',
  }
  const variant = state === 'crashed' || state === 'failed' ? 'destructive' : state === 'stopped' ? 'secondary' : 'default'
  return (
    <Badge variant={variant} className="shrink-0">
      {label[state] ?? state}
    </Badge>
  )
}

/** One source's commands, grouped by namespace ("make", "migrate", ...). */
function SourceList({ source, commands, selected, onSelect }: { source: CommandSource; commands: DiscoveredCommand[]; selected: string | null; onSelect: (name: string) => void }) {
  if (source.error) return <SourceProblem tone="error" title={`Could not read the ${source.label} commands.`} text={source.error} />
  const groups = new Map<string, DiscoveredCommand[]>()
  for (const c of commands) {
    const ns = source.id === 'scripts' ? '' : namespaceOf(c.name)
    groups.set(ns, [...(groups.get(ns) ?? []), c])
  }
  const ordered = [...groups.entries()].sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : a.localeCompare(b)))
  return (
    <div className="flex flex-col gap-3">
      {source.warning && <SourceProblem tone="warning" text={source.warning} />}
      {commands.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">No command matches.</p>}
      {ordered.map(([ns, list]) => (
        <div key={ns || '_'}>
          {ordered.length > 1 && <div className="sticky top-0 z-10 bg-card py-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">{ns || 'general'}</div>}
          <div className="flex flex-col">
            {list.map((c) => (
              <button
                key={c.name}
                onClick={() => onSelect(c.name)}
                className={cn('flex min-w-0 flex-col rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-muted', selected === c.name && 'bg-primary/10 hover:bg-primary/15')}
              >
                <span className="truncate font-mono text-[13px]">{c.name}</span>
                {c.description && <span className={cn('truncate text-xs text-muted-foreground', source.id === 'scripts' && 'font-mono')}>{c.description}</span>}
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

/**
 * Why a source's list is missing or second-best. The backend puts plain-words paragraphs
 * first and the tool's own output last; that last part is tucked away.
 */
function SourceProblem({ tone, title, text }: { tone: 'error' | 'warning'; title?: string; text: string }) {
  const parts = text.split('\n\n')
  const raw = parts.length > 1 ? parts.pop() : null
  return (
    <div className={cn('flex flex-col gap-1.5 rounded-lg border p-3 text-xs', tone === 'error' ? 'border-destructive/40 bg-destructive/5' : 'border-warning/40 bg-warning/10')}>
      {title && <p className="font-medium">{title}</p>}
      {parts.map((p, i) => (
        <p key={i} className={cn(i === 0 && !title && 'font-medium')}>
          {p}
        </p>
      ))}
      {raw && (
        <details className="text-muted-foreground">
          <summary className="cursor-pointer select-none">Details</summary>
          <pre className="mt-1 whitespace-pre-wrap break-all">{raw}</pre>
        </details>
      )}
    </div>
  )
}

function QuickList({ commands, selected, onSelect, onNew }: { commands: QuickCommand[]; selected: string | null; onSelect: (id: string) => void; onNew: () => void }) {
  const categories = Array.from(new Set(commands.map((c) => (c.builtin ? c.category || 'other' : 'custom')))).sort((a, b) => categoryRank(a) - categoryRank(b) || a.localeCompare(b))
  return (
    <div className="flex flex-col gap-3">
      <Button size="sm" variant="secondary" className="self-start" onClick={onNew}>
        <Plus /> New custom command
      </Button>
      {categories.map((cat) => (
        <div key={cat}>
          <div className="sticky top-0 z-10 flex items-center gap-1.5 bg-card py-1 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <TechIcon id={cat} className="size-3" /> {categoryLabel(cat)}
          </div>
          {commands
            .filter((c) => (c.builtin ? c.category || 'other' : 'custom') === cat)
            .map((c) => (
              <button
                key={c.id}
                onClick={() => onSelect(c.id)}
                className={cn('flex w-full min-w-0 flex-col rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-muted', selected === c.id && 'bg-primary/10 hover:bg-primary/15')}
              >
                <span className="truncate text-[13px] font-medium">{c.name}</span>
                <span className="truncate text-xs text-muted-foreground">{c.description || commandLine(c)}</span>
              </button>
            ))}
        </div>
      ))}
      {commands.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">No command matches.</p>}
    </div>
  )
}

function CommandPreview({ text }: { text: string }) {
  return (
    <div className="flex items-start gap-2 rounded-lg bg-muted/60 px-3 py-2">
      <code className="min-w-0 flex-1 whitespace-pre-wrap break-all font-mono text-xs">{text}</code>
      <button title="Copy" className="shrink-0 text-muted-foreground hover:text-foreground" onClick={() => navigator.clipboard.writeText(text)}>
        <Clipboard className="size-3.5" />
      </button>
    </div>
  )
}

/** A form built from a command's definition: arguments, options, anything extra — and the exact line it makes. */
function CommandForm({
  source,
  command,
  busy,
  needsProject,
  onRun,
  onEdit,
  onSave,
}: {
  source: CommandSource
  command: DiscoveredCommand
  busy: boolean
  needsProject: boolean
  onRun: (line: string) => void
  onEdit: (line: string) => void
  onSave: (line: string) => void
}) {
  const [args, setArgs] = useState<Record<string, string>>({})
  const [flags, setFlags] = useState<Record<string, boolean>>({})
  const [values, setValues] = useState<Record<string, string>>({})
  const [extra, setExtra] = useState('')
  const [optionFilter, setOptionFilter] = useState('')

  const words = (v: string) => splitLine(v).filter(Boolean)
  const tokens: string[] = [...source.prefix, command.name]
  for (const a of command.arguments) {
    const v = args[a.name]?.trim() ?? ''
    if (v) tokens.push(...(a.multiple ? words(v) : [v]))
  }
  for (const o of command.options) {
    const v = values[o.name]?.trim() ?? ''
    if (!o.accepts_value) {
      if (flags[o.name]) tokens.push(o.name)
    } else if (v) {
      for (const one of o.multiple ? words(v) : [v]) tokens.push(`${o.name}=${one}`)
    } else if (!o.value_required && flags[o.name]) tokens.push(o.name)
  }
  const extraWords = words(extra)
  if (extraWords.length) tokens.push(...(source.id === 'scripts' && source.prefix[0] === 'npm' ? ['--', ...extraWords] : extraWords))
  const line = tokens.map(quote).join(' ')
  const missing = command.arguments.filter((a) => a.required && !args[a.name]?.trim()).map((a) => a.name)

  const f = optionFilter.trim().toLowerCase()
  const options = command.options.filter((o) => !f || o.name.includes(f) || o.description.toLowerCase().includes(f))
  const flagOptions = options.filter((o) => !o.accepts_value)
  const valueOptions = options.filter((o) => o.accepts_value)

  const submit = () => !busy && missing.length === 0 && !needsProject && onRun(line)

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 font-mono text-base">
          <TechIcon id={SOURCE_ICON[source.id] ?? source.prefix[0]} />
          <span className="truncate">{command.name}</span>
        </CardTitle>
        {command.description && <CardDescription className={cn(source.id === 'scripts' && 'font-mono')}>{command.description}</CardDescription>}
        {command.help && command.help !== command.description && (
          <details className="text-xs text-muted-foreground">
            <summary className="cursor-pointer select-none">More help</summary>
            <pre className="mt-2 max-h-48 overflow-y-auto whitespace-pre-wrap font-sans">{command.help}</pre>
          </details>
        )}
      </CardHeader>
      <CardContent
        className="flex flex-col gap-5"
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'INPUT') submit()
        }}
      >
        {command.arguments.length > 0 && (
          <section className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Arguments</h3>
            <div className="grid gap-3 sm:grid-cols-2">
              {command.arguments.map((a) => (
                <Field key={a.name} label={`${a.name}${a.required ? ' *' : ''}`} hint={[a.description, a.multiple && 'Several values: separate with spaces.'].filter(Boolean).join(' ')}>
                  <Input value={args[a.name] ?? ''} placeholder={a.default ?? ''} onChange={(e) => setArgs({ ...args, [a.name]: e.target.value })} />
                </Field>
              ))}
            </div>
          </section>
        )}

        {command.options.length > 0 && (
          <section className="flex flex-col gap-3">
            <div className="flex items-center justify-between gap-3">
              <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Options</h3>
              {command.options.length > 8 && <Input value={optionFilter} onChange={(e) => setOptionFilter(e.target.value)} placeholder="Filter options" className="h-7 w-44 text-xs" />}
            </div>
            {flagOptions.length > 0 && (
              <div className="grid gap-x-4 gap-y-2.5 sm:grid-cols-2">
                {flagOptions.map((o) => (
                  <Toggle key={o.name} checked={!!flags[o.name]} onChange={(v) => setFlags({ ...flags, [o.name]: v })} label={o.name + (o.shortcut ? `  (${o.shortcut})` : '')} hint={o.description} />
                ))}
              </div>
            )}
            {valueOptions.length > 0 && (
              <div className="grid gap-3 sm:grid-cols-2">
                {valueOptions.map((o) => (
                  <Field
                    key={o.name}
                    label={o.name}
                    hint={[o.description, o.multiple && 'Several values: separate with spaces.', !o.value_required && 'The value is optional.'].filter(Boolean).join(' ')}
                  >
                    <div className="flex items-center gap-2">
                      {!o.value_required && (
                        <input
                          type="checkbox"
                          title={`Pass ${o.name} without a value`}
                          checked={!!flags[o.name]}
                          onChange={(e) => setFlags({ ...flags, [o.name]: e.target.checked })}
                          className="size-4 shrink-0 accent-[var(--primary)]"
                        />
                      )}
                      <Input value={values[o.name] ?? ''} placeholder={o.default ?? ''} onChange={(e) => setValues({ ...values, [o.name]: e.target.value })} />
                    </div>
                  </Field>
                ))}
              </div>
            )}
            {options.length === 0 && <p className="text-xs text-muted-foreground">No option matches.</p>}
          </section>
        )}

        <Field label="Extra arguments" hint={source.id === 'django' || source.id === 'scripts' ? 'Passed through as typed.' : 'Anything the form above does not cover, e.g. -v or --env=testing.'}>
          <Input value={extra} onChange={(e) => setExtra(e.target.value)} className="font-mono" />
        </Field>

        <div className="flex flex-col gap-3">
          <CommandPreview text={line} />
          <div className="flex flex-wrap items-center gap-2">
            <Button disabled={busy || missing.length > 0 || needsProject} onClick={submit}>
              <Play /> Run
            </Button>
            <Button variant="secondary" onClick={() => onSave(line)}>
              <Save /> Save as custom command
            </Button>
            <Button variant="ghost" onClick={() => onEdit(line)}>
              <Pencil /> Edit as text
            </Button>
            {missing.length > 0 && <span className="text-xs text-muted-foreground">Needs: {missing.join(', ')}</span>}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function QuickDetail({
  command,
  busy,
  needsProject,
  onRun,
  onEdit,
  onDelete,
}: {
  command: QuickCommand
  busy: boolean
  needsProject: boolean
  onRun: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const text = commandLine(command)
  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 text-base">
          <TechIcon id={command.builtin ? command.category || 'other' : 'custom'} />
          <span className="truncate">{command.name}</span>
          {!command.builtin && <Badge variant="secondary">yours</Badge>}
        </CardTitle>
        {command.description && <CardDescription>{command.description}</CardDescription>}
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {text ? <CommandPreview text={text} /> : <p className="text-xs text-muted-foreground">Built-in action: {command.action?.replace(/_/g, ' ')}</p>}
        {command.applies_to.length > 0 && <p className="text-xs text-muted-foreground">For {command.applies_to.map((f) => f.replace(/_/g, ' ')).join(', ')} projects.</p>}
        <div className="flex flex-wrap items-center gap-2">
          <Button disabled={busy || needsProject} onClick={onRun}>
            <Play /> Run
          </Button>
          {command.command && (
            <Button variant="secondary" onClick={onEdit}>
              <Pencil /> {command.builtin ? 'Customize' : 'Edit'}
            </Button>
          )}
          {!command.builtin && (
            <Button variant="ghost" onClick={onDelete}>
              <Trash2 /> Delete
            </Button>
          )}
          {needsProject && <span className="text-xs text-muted-foreground">Pick a project first.</span>}
        </div>
      </CardContent>
    </Card>
  )
}

/** Create or edit a custom command. Editing a built-in saves your version under the same id. */
function CustomCommandDialog({
  command,
  existingIds,
  busy,
  onClose,
  onSave,
}: {
  command: QuickCommand
  existingIds: string[]
  busy: boolean
  onClose: () => void
  onSave: (cmd: QuickCommand) => void
}) {
  const [name, setName] = useState(command.name)
  const [text, setText] = useState(commandLine(command))
  const [description, setDescription] = useState(command.description)
  const [appliesTo, setAppliesTo] = useState(command.applies_to[0] ?? '')
  const [inProject, setInProject] = useState(command.working_directory === '{{project_path}}' || command.working_directory === null)

  const tokens = splitLine(text)
  const valid = name.trim() !== '' && tokens.length > 0

  function save() {
    let id = command.id
    if (!id) {
      const base = slug(name)
      id = base
      for (let n = 2; existingIds.includes(id); n++) id = `${base}-${n}`
    }
    onSave({
      ...command,
      id,
      name: name.trim(),
      description: description.trim(),
      category: command.category || 'custom',
      applies_to: appliesTo ? [appliesTo] : [],
      working_directory: inProject ? '{{project_path}}' : null,
      command: { executable: tokens[0], arguments: tokens.slice(1) },
      action: null,
      builtin: false,
    })
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title={command.id ? (command.builtin ? 'Customize command' : 'Edit command') : 'New custom command'}
      description={command.builtin ? 'Your version replaces the built-in one. Deleting it later brings the built-in back.' : undefined}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={busy || !valid} onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Rebuild search index" autoFocus />
        </Field>
        <Field label="Command" hint="Runs with the project's own PHP / Node / Python. {{project_path}}, {{project_name}} and {{domain}} are filled in.">
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="php artisan scout:import App\Models\Post" className="font-mono" />
        </Field>
        <Field label="Description">
          <Input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Optional" />
        </Field>
        <Field label="Show for">
          <Select value={appliesTo} onChange={(e) => setAppliesTo(e.target.value)}>
            <option value="">Every project</option>
            {FRAMEWORKS.map((f) => (
              <option key={f} value={f}>
                {f.replace(/_/g, ' ')} projects only
              </option>
            ))}
          </Select>
        </Field>
        <Toggle checked={inProject} onChange={setInProject} label="Run in the project folder" />
      </div>
    </Dialog>
  )
}
