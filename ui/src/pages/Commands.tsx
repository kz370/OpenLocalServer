import { listen } from '@tauri-apps/api/event'
import { Clipboard, FileText, History as HistoryIcon, Pencil, Play, Plus, RefreshCw, Save, Search, SquareTerminal, Trash2 } from 'lucide-react'
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
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
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
export const SOURCE_ICON: Record<string, string> = {
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
export const sourceCache = new Map<string, CommandSource[]>()

/** Quotes one argument the way the backend's command-line splitter reads it back. */
export function quote(arg: string): string {
  return arg === '' || /[\s"']/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg
}

/** The inverse of `quote` for a whole line: whitespace splits, "..." and '...' group. */
export function splitLine(line: string): string[] {
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

export const commandLine = (c: QuickCommand) => (c.command?.executable ? [c.command.executable, ...c.command.arguments].map(quote).join(' ') : '')

type DialogTab = 'run' | 'details' | 'history'

interface CmdRow {
  key: string
  name: string
  source: string
  icon: string
  description: string
  mono: boolean
  lastRun: number | null
  selection: NonNullable<Selection>
}

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
  const [dialogOpen, setDialogOpen] = useState(false)
  const [dialogTab, setDialogTab] = useState<DialogTab>('run')
  const outputCard = useRef<HTMLDivElement>(null)
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
    setDialogOpen(false)
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
      setTimeout(() => outputCard.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' }), 50)
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

  // The newest run of every command line, so each row can say when it last ran.
  const lastRunByLine = useMemo(() => {
    const m = new Map<string, number>()
    for (const h of history) m.set(h.line, Math.max(m.get(h.line) ?? 0, h.timestamp_ms))
    return m
  }, [history])
  const lastRunOf = (prefix: string): number | null => {
    if (!prefix) return null
    let best: number | null = null
    for (const [l, at] of lastRunByLine) if ((l === prefix || l.startsWith(prefix + ' ')) && (best === null || at > best)) best = at
    return best
  }

  const rows: CmdRow[] = useMemo(() => {
    if (activeSource) {
      return sourceShown(activeSource).map((c) => ({
        key: `${activeSource.id}:${c.name}`,
        name: c.name,
        source: activeSource.label,
        icon: SOURCE_ICON[activeSource.id] ?? activeSource.prefix[0],
        description: c.description,
        mono: activeSource.id === 'scripts',
        lastRun: lastRunOf([...activeSource.prefix, c.name].join(' ')),
        selection: { kind: 'discovered', source: activeSource.id, name: c.name },
      }))
    }
    if (activeTab === 'custom') {
      return [...quickShown]
        .sort((a, b) => categoryRank(a.builtin ? a.category : 'custom') - categoryRank(b.builtin ? b.category : 'custom') || a.name.localeCompare(b.name))
        .map((c) => ({
          key: `quick:${c.id}`,
          name: c.name,
          source: c.builtin ? categoryLabel(c.category || 'other') : 'Yours',
          icon: c.builtin ? c.category || 'other' : 'custom',
          description: c.description || commandLine(c),
          mono: false,
          lastRun: lastRunOf(commandLine(c)),
          selection: { kind: 'quick', id: c.id },
        }))
    }
    return []
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSource, activeTab, sources, commands, history, query, framework])

  const openRow = (sel: NonNullable<Selection>) => {
    setSelected(sel)
    setDialogTab('run')
    setDialogOpen(true)
  }
  const dialogPrefix = selectedDiscovered ? [...selectedDiscovered.source.prefix, selectedDiscovered.command.name].join(' ') : selectedQuick ? commandLine(selectedQuick) : ''
  const dialogHistory = dialogPrefix ? history.filter((h) => h.line === dialogPrefix || h.line.startsWith(dialogPrefix + ' ')) : []

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
          <Button variant="secondary" onClick={() => setEditing(newCustom('', '', 'custom', []))}>
            <Plus /> New custom command
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

      <Card className="min-w-0">
        <CardContent className="flex flex-col gap-3 pt-4">
          <Tabs tabs={tabs} value={activeTab} onChange={(t) => setTab(t)} />
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input ref={searchRef} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search commands   ( / )" className="pl-9" />
          </div>

          {activeSource?.error && <SourceProblem tone="error" title={`Could not read the ${activeSource.label} commands.`} text={activeSource.error} />}
          {activeSource?.warning && <SourceProblem tone="warning" text={activeSource.warning} />}

          {activeTab === 'recent' ? (
            <div className="flex flex-col gap-2">
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
              {historyShown.length > 0 ? (
                <div className="max-h-[52vh] overflow-y-auto rounded-lg">
                  <Table>
                    <TableHeader className="sticky top-0 z-10 bg-muted">
                      <TableRow className="hover:bg-transparent">
                        <TableHead>Command</TableHead>
                        <TableHead className="w-40">Project</TableHead>
                        <TableHead className="w-28">When</TableHead>
                        <TableHead className="w-52 text-right">Actions</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {historyShown.slice(0, 100).map((h) => (
                        <TableRow key={h.id}>
                          <TableCell className="py-1.5">
                            <code className="block truncate text-xs">{h.line}</code>
                          </TableCell>
                          <TableCell className="py-1.5 text-xs text-muted-foreground">{projects.find((p) => p.id === h.project_id)?.name ?? '—'}</TableCell>
                          <TableCell className="py-1.5 text-xs text-muted-foreground">{timeAgo(h.timestamp_ms)}</TableCell>
                          <TableCell className="py-1.5 text-right">
                            <span className="inline-flex justify-end">
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
                                      await runCommand({ type: 'delete_history', id: h.id })
                                      await refresh()
                                    }),
                                  )
                                }
                              >
                                <Trash2 className="size-3.5" />
                              </Button>
                            </span>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              ) : (
                <p className="py-6 text-center text-sm text-muted-foreground">{q ? 'Nothing in the history matches.' : 'Commands you run appear here.'}</p>
              )}
            </div>
          ) : (
            <>
              {rows.length > 0 ? (
                <div className="max-h-[52vh] overflow-y-auto rounded-lg">
                  <Table>
                    <TableHeader className="sticky top-0 z-10 bg-muted">
                      <TableRow className="hover:bg-transparent">
                        <TableHead className="w-[26%]">Command</TableHead>
                        <TableHead className="w-32">Source</TableHead>
                        <TableHead>Description</TableHead>
                        <TableHead className="w-28">Last run</TableHead>
                        <TableHead className="w-24 text-right">Run</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {rows.map((r) => (
                        <TableRow key={r.key} className="cursor-pointer" onClick={() => openRow(r.selection)}>
                          <TableCell className="py-1.5 font-mono text-[13px]">{r.name}</TableCell>
                          <TableCell className="py-1.5">
                            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                              <TechIcon id={r.icon} className="size-3.5" /> {r.source}
                            </span>
                          </TableCell>
                          <TableCell className={cn('max-w-0 truncate py-1.5 text-xs text-muted-foreground', r.mono && 'font-mono')}>{r.description}</TableCell>
                          <TableCell className="py-1.5 text-xs text-muted-foreground">{r.lastRun ? timeAgo(r.lastRun) : '—'}</TableCell>
                          <TableCell className="py-1.5 text-right">
                            <Button
                              size="sm"
                              variant="secondary"
                              className="h-7"
                              onClick={(e) => {
                                e.stopPropagation()
                                openRow(r.selection)
                              }}
                            >
                              <Play className="size-3.5" /> Run
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              ) : (
                !loadingSources && <p className="py-6 text-center text-sm text-muted-foreground">No command matches.</p>
              )}
              {!projectId && activeTab === 'custom' && <p className="text-xs text-muted-foreground">Pick a project to also see its artisan, composer, package.json and manage.py commands.</p>}
              {projectId && loadingSources && sources.length === 0 && (
                <p className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Spinner /> Reading the project's commands…
                </p>
              )}
            </>
          )}
        </CardContent>
      </Card>

      <div ref={outputCard}>
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
                      await runCommand({ type: 'stop_process', id: processId })
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

      <Dialog
        open={dialogOpen}
        wide
        onClose={() => setDialogOpen(false)}
        title={selectedDiscovered?.command.name ?? selectedQuick?.name ?? 'Command'}
        description={selectedDiscovered ? `${selectedDiscovered.source.label} command` : selectedQuick ? (selectedQuick.builtin ? 'Built-in command' : 'Your command') : undefined}
      >
        <div className="flex min-h-[26rem] flex-col gap-4 md:flex-row">
          <nav className="flex shrink-0 gap-0.5 md:w-36 md:flex-col" aria-label="Command sections">
            {(
              [
                { id: 'run', label: 'Run', icon: <Play className="size-4" /> },
                { id: 'details', label: 'Details', icon: <FileText className="size-4" /> },
                { id: 'history', label: 'History', icon: <HistoryIcon className="size-4" /> },
              ] as { id: DialogTab; label: string; icon: React.ReactNode }[]
            ).map((i) => (
              <button
                key={i.id}
                onClick={() => setDialogTab(i.id)}
                className={cn(
                  'flex items-center gap-2 rounded-md px-3 py-2 text-left text-sm font-medium transition-colors',
                  dialogTab === i.id ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
                )}
              >
                {i.icon}
                {i.label}
              </button>
            ))}
          </nav>
          <div className="min-w-0 flex-1">
            {dialogTab === 'run' && selectedDiscovered && (
              <CommandForm
                key={`${selectedDiscovered.source.id}:${selectedDiscovered.command.name}`}
                className="border-0 shadow-none"
                source={selectedDiscovered.source}
                command={selectedDiscovered.command}
                busy={busy !== null}
                needsProject={!projectId}
                onRun={(text) => {
                  setDialogOpen(false)
                  void runLine(text)
                }}
                onEdit={(text) => {
                  setDialogOpen(false)
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
            )}
            {dialogTab === 'run' && selectedQuick && (
              <QuickDetail
                className="border-0 shadow-none"
                command={selectedQuick}
                busy={busy !== null}
                needsProject={selectedQuick.applies_to.length > 0 && !projectId}
                onRun={() => {
                  setDialogOpen(false)
                  void runQuick(selectedQuick)
                }}
                onEdit={() => setEditing(selectedQuick)}
                onDelete={() =>
                  confirmThen(
                    commands.some((c) => c.id === selectedQuick.id && !c.builtin)
                      ? `Delete "${selectedQuick.name}"? A built-in command with the same id comes back as it was.`
                      : `Delete "${selectedQuick.name}"?`,
                    () =>
                      run('del', async () => {
                        await runCommand({ type: 'delete_quick_command', id: selectedQuick.id })
                        setDialogOpen(false)
                        setSelected(null)
                        await refresh()
                      }),
                  )
                }
              />
            )}
            {dialogTab === 'details' && (selectedDiscovered || selectedQuick) && (
              <div className="flex flex-col gap-4 text-sm">
                {selectedDiscovered && (
                  <>
                    <CommandPreview text={[...selectedDiscovered.source.prefix, selectedDiscovered.command.name].map(quote).join(' ')} />
                    {selectedDiscovered.command.description && <p>{selectedDiscovered.command.description}</p>}
                    {selectedDiscovered.command.help && selectedDiscovered.command.help !== selectedDiscovered.command.description && (
                      <pre className="max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg bg-muted/40 p-3 font-sans text-xs">{selectedDiscovered.command.help}</pre>
                    )}
                    {selectedDiscovered.command.arguments.length > 0 && (
                      <div>
                        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Arguments</h3>
                        <ul className="flex flex-col gap-1 text-xs">
                          {selectedDiscovered.command.arguments.map((a) => (
                            <li key={a.name}>
                              <code>{a.name}</code>
                              {a.required && ' (required)'} <span className="text-muted-foreground">{a.description}</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                    {selectedDiscovered.command.options.length > 0 && (
                      <div>
                        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Options</h3>
                        <ul className="flex max-h-56 flex-col gap-1 overflow-y-auto text-xs">
                          {selectedDiscovered.command.options.map((o) => (
                            <li key={o.name}>
                              <code>{o.name}</code> <span className="text-muted-foreground">{o.description}</span>
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </>
                )}
                {selectedQuick && (
                  <>
                    {commandLine(selectedQuick) ? <CommandPreview text={commandLine(selectedQuick)} /> : <p className="text-xs text-muted-foreground">Built-in action: {selectedQuick.action?.replace(/_/g, ' ')}</p>}
                    {selectedQuick.description && <p>{selectedQuick.description}</p>}
                    <p className="text-xs text-muted-foreground">
                      Category: {categoryLabel(selectedQuick.builtin ? selectedQuick.category || 'other' : 'custom')}. Runs in{' '}
                      {selectedQuick.working_directory === '{{project_path}}' || selectedQuick.working_directory === null ? "the project's folder" : selectedQuick.working_directory}.
                      {selectedQuick.applies_to.length > 0 && ` For ${selectedQuick.applies_to.map((f) => f.replace(/_/g, ' ')).join(', ')} projects.`}
                    </p>
                  </>
                )}
              </div>
            )}
            {dialogTab === 'history' && (
              <div className="flex flex-col gap-1.5">
                {dialogHistory.length === 0 && <p className="py-6 text-center text-sm text-muted-foreground">This command has not been run yet.</p>}
                {dialogHistory.slice(0, 30).map((h) => (
                  <div key={h.id} className="flex items-center justify-between gap-2 rounded-lg border border-border px-3 py-1.5">
                    <div className="min-w-0">
                      <code className="block truncate text-xs">{h.line}</code>
                      <span className="text-[11px] text-muted-foreground">{timeAgo(h.timestamp_ms)}</span>
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      title="Run again"
                      onClick={() => {
                        setDialogOpen(false)
                        void runLine(h.line, h.project_id ?? projectId)
                      }}
                    >
                      <Play className="size-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </Dialog>

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

export function StateBadge({ state }: { state: ProcessState }) {
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

/**
 * Why a source's list is missing or second-best. The backend puts plain-words paragraphs
 * first and the tool's own output last; that last part is tucked away.
 */
export function SourceProblem({ tone, title, text }: { tone: 'error' | 'warning'; title?: string; text: string }) {
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
export function CommandForm({
  source,
  command,
  busy,
  needsProject,
  onRun,
  onEdit,
  onSave,
  hidePrefix,
  className,
}: {
  source: CommandSource
  command: DiscoveredCommand
  busy: boolean
  needsProject: boolean
  onRun: (line: string) => void
  /** Gets the line as shown: without the tool's prefix when `hidePrefix` is set. */
  onEdit: (line: string) => void
  onSave: (line: string) => void
  /** Show the line without "php artisan" / "composer" / "npm run"; it is still run with it. */
  hidePrefix?: boolean
  className?: string
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
  const shownLine = hidePrefix ? tokens.slice(source.prefix.length).map(quote).join(' ') : line
  const missing = command.arguments.filter((a) => a.required && !args[a.name]?.trim()).map((a) => a.name)

  const f = optionFilter.trim().toLowerCase()
  const options = command.options.filter((o) => !f || o.name.includes(f) || o.description.toLowerCase().includes(f))
  const flagOptions = options.filter((o) => !o.accepts_value)
  const valueOptions = options.filter((o) => o.accepts_value)

  const submit = () => !busy && missing.length === 0 && !needsProject && onRun(line)

  return (
    <Card className={className}>
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
                        <Switch
                          size="sm"
                          label={`Pass ${o.name} without a value`}
                          checked={!!flags[o.name]}
                          onChange={(on) => setFlags({ ...flags, [o.name]: on })}
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
          <CommandPreview text={shownLine} />
          <div className="flex flex-wrap items-center gap-2">
            <Button disabled={busy || missing.length > 0 || needsProject} onClick={submit}>
              <Play /> Run
            </Button>
            <Button variant="secondary" onClick={() => onSave(line)}>
              <Save /> Save as custom command
            </Button>
            <Button variant="ghost" onClick={() => onEdit(shownLine)}>
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
  className,
}: {
  command: QuickCommand
  busy: boolean
  needsProject: boolean
  onRun: () => void
  onEdit: () => void
  onDelete: () => void
  className?: string
}) {
  const text = commandLine(command)
  return (
    <Card className={className}>
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
