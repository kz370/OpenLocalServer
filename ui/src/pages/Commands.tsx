import { listen } from '@tauri-apps/api/event'
import { Clipboard, Pencil, Play, Save, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type HistoryEntry, type ProcessEvent, type Project, type QuickCommand, runCommand } from '@/core'
import { timeAgo, useAction } from '@/lib/hooks'

/** §89–93: reusable developer commands, a free-form runner, and what was run recently. */
export function CommandsPage() {
  const [projects, setProjects] = useState<Project[]>([])
  const [projectId, setProjectId] = useState<string>('')
  const [framework, setFramework] = useState<string>('')
  const [commands, setCommands] = useState<QuickCommand[]>([])
  const [history, setHistory] = useState<HistoryEntry[]>([])
  const [line, setLine] = useState('')
  const [output, setOutput] = useState<string[]>([])
  const [title, setTitle] = useState<string>('')
  const [processId, setProcessId] = useState<number | null>(null)
  const [saveFor, setSaveFor] = useState<HistoryEntry | null>(null)
  const [saveId, setSaveId] = useState('')
  const [saveName, setSaveName] = useState('')
  const { busy, error, setError, run } = useAction()
  const outRef = useRef<HTMLPreElement>(null)

  async function refresh() {
    const [c, h] = await Promise.all([runCommand({ type: 'list_quick_commands' }), runCommand({ type: 'list_history' })])
    if (c.type === 'quick_commands') setCommands(c.commands)
    if (h.type === 'history') setHistory(h.entries)
  }

  useEffect(() => {
    runCommand({ type: 'list_projects' }).then((r) => {
      if (r.type === 'projects') {
        setProjects(r.projects)
        if (r.projects[0]) setProjectId(r.projects[0].id)
      }
    })
    refresh().catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!projectId) return setFramework('')
    runCommand({ type: 'get_project_detail', id: projectId }).then((r) => r.type === 'project_detail' && setFramework(r.detail.detection.framework))
  }, [projectId])

  useEffect(() => {
    const un = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (processId !== null && e.id === processId && e.kind === 'output') setOutput((prev) => [...prev.slice(-499), e.line])
    })
    return () => {
      un.then((f) => f())
    }
  }, [processId])

  useEffect(() => {
    outRef.current?.scrollTo({ top: outRef.current.scrollHeight })
  }, [output.length])

  const visible = commands.filter((c) => c.applies_to.length === 0 || c.applies_to.includes(framework))
  const categories = Array.from(new Set(visible.map((c) => c.category)))

  async function startProcess(res: Awaited<ReturnType<typeof runCommand>>, label: string) {
    if (res.type === 'process_started' || (res.type === 'maybe_process' && res.id !== null)) {
      setOutput([])
      setTitle(label)
      setProcessId(res.type === 'process_started' ? res.id : (res as { id: number }).id)
    }
    await refresh()
  }

  const runQuick = (c: QuickCommand) =>
    run(c.id, async () => startProcess(await runCommand({ type: 'run_quick_command', id: c.id, project_id: projectId || null }), c.name))

  const runLine = (text: string) =>
    run('line', async () => startProcess(await runCommand({ type: 'run_command_line', line: text, cwd: null, project_id: projectId || null }), text))

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Commands</h1>
        <p className="text-sm text-muted-foreground">
          Run common tasks with the project's own PHP / Node / Python, without opening a terminal.
        </p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <Card>
        <CardContent className="flex flex-wrap items-end gap-3 pt-4">
          <Field label="Project">
            <Select value={projectId} onChange={(e) => setProjectId(e.target.value)} className="w-64">
              <option value="">— none (global) —</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </Select>
          </Field>
          <div className="flex min-w-72 flex-1 flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Run a command</label>
            <div className="flex gap-2">
              <Input value={line} onChange={(e) => setLine(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && line.trim() && runLine(line)} placeholder="php artisan tinker   ·   npm run build   ·   composer require x/y" />
              <Button disabled={!line.trim() || busy !== null} onClick={() => runLine(line)}>
                <Play /> Run
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Quick Commands</CardTitle>
            <CardDescription>{framework ? `Showing commands for ${framework.replace('_', ' ')} projects and general tools.` : 'Pick a project to see its commands.'}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {categories.map((cat) => (
              <div key={cat}>
                <div className="mb-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">{cat}</div>
                <div className="flex flex-col gap-1.5">
                  {visible.filter((c) => c.category === cat).map((c) => (
                    <div key={c.id} className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium">
                          {c.name}
                          {!c.builtin && <Badge variant="secondary" className="ml-2">yours</Badge>}
                        </div>
                        <div className="truncate text-xs text-muted-foreground">{c.description}</div>
                      </div>
                      <div className="flex shrink-0 gap-1">
                        {!c.builtin && (
                          <Button size="sm" variant="ghost" title="Delete" onClick={() => run('del', async () => { await runCommand({ type: 'delete_quick_command', id: c.id }); await refresh() })}>
                            <Trash2 className="size-3.5" />
                          </Button>
                        )}
                        <Button size="sm" variant="secondary" disabled={busy !== null || (c.applies_to.length > 0 && !projectId)} onClick={() => runQuick(c)}>
                          <Play /> Run
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </CardContent>
        </Card>

        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm">Output{title && `: ${title}`}</CardTitle>
              {processId !== null && (
                <Button size="sm" variant="ghost" onClick={() => runCommand({ type: 'stop_process', id: processId })}>Stop</Button>
              )}
            </CardHeader>
            <CardContent>
              <pre ref={outRef} className="h-56 overflow-auto rounded-lg bg-muted/40 p-3 font-mono text-xs">{output.join('\n') || 'Run something to see its output here.'}</pre>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm">Recent commands</CardTitle>
              {history.length > 0 && (
                <Button size="sm" variant="ghost" onClick={() => run('clear', async () => { await runCommand({ type: 'clear_history' }); await refresh() })}>Clear</Button>
              )}
            </CardHeader>
            <CardContent className="flex flex-col gap-1.5">
              {history.slice(0, 30).map((h) => (
                <div key={h.id} className="flex items-center justify-between gap-2 rounded-lg border border-border px-3 py-1.5">
                  <div className="min-w-0">
                    <code className="block truncate text-xs">{h.line}</code>
                    <span className="text-[11px] text-muted-foreground">
                      {timeAgo(h.timestamp_ms)}{h.project_id && ` · ${projects.find((p) => p.id === h.project_id)?.name ?? ''}`}
                    </span>
                  </div>
                  <div className="flex shrink-0">
                    <Button size="sm" variant="ghost" title="Run again" onClick={() => { setProjectId(h.project_id ?? ''); void runLine(h.line) }}>
                      <Play className="size-3.5" />
                    </Button>
                    <Button size="sm" variant="ghost" title="Edit" onClick={() => setLine(h.line)}>
                      <Pencil className="size-3.5" />
                    </Button>
                    <Button size="sm" variant="ghost" title="Save as Quick Command" onClick={() => { setSaveFor(h); setSaveId(''); setSaveName(h.line.slice(0, 40)) }}>
                      <Save className="size-3.5" />
                    </Button>
                    <Button size="sm" variant="ghost" title="Copy" onClick={() => navigator.clipboard.writeText(h.line)}>
                      <Clipboard className="size-3.5" />
                    </Button>
                    <Button size="sm" variant="ghost" title="Delete" onClick={() => run('delh', async () => { await runCommand({ type: 'delete_history', id: h.id }); await refresh() })}>
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>
              ))}
              {history.length === 0 && <p className="text-sm text-muted-foreground">Commands you run appear here.</p>}
            </CardContent>
          </Card>
        </div>
      </div>

      <Dialog
        open={!!saveFor}
        onClose={() => setSaveFor(null)}
        title="Save as Quick Command"
        footer={
          <>
            <Button variant="ghost" onClick={() => setSaveFor(null)}>Cancel</Button>
            <Button disabled={busy !== null || !saveId || !saveName} onClick={() => run('save', async () => { await runCommand({ type: 'save_history_as_quick_command', id: saveFor!.id, command_id: saveId, name: saveName }); setSaveFor(null); await refresh() })}>Save</Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <code className="rounded bg-muted px-2 py-1 text-xs">{saveFor?.line}</code>
          <Field label="Name">
            <Input value={saveName} onChange={(e) => setSaveName(e.target.value)} />
          </Field>
          <Field label="Id" hint="Lowercase letters, digits and dashes">
            <Input value={saveId} onChange={(e) => setSaveId(e.target.value)} placeholder="my-command" />
          </Field>
        </div>
      </Dialog>
    </div>
  )
}
