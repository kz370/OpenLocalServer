import { save } from '@tauri-apps/plugin-dialog'
import { Clipboard, Download, Eraser, Pause, Play } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type LogSource, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'

export type Severity = 'error' | 'warn' | 'info' | 'debug'

/** Best-effort severity from JSON logs (`"level":"WARN"`) and plain text (`[error]`, `ERROR:`). */
export function severityOf(line: string): Severity {
  const l = line.toLowerCase()
  if (/"level":"(error|fatal)"|\[(emerg|alert|crit|error)\]|\berror\b|✗|failed|fatal/.test(l)) return 'error'
  if (/"level":"warn|\[warn\]|\bwarn(ing)?\b/.test(l)) return 'warn'
  if (/"level":"debug|"level":"trace|\[debug\]/.test(l)) return 'debug'
  return 'info'
}

export const SEVERITY_COLOR: Record<Severity, string> = {
  error: 'text-destructive',
  warn: 'text-warning',
  info: '',
  debug: 'text-muted-foreground',
}

/** §117: every log source in one place, live tail, search, filter, copy, export. */
export function LogsPage() {
  const [sources, setSources] = useState<LogSource[]>([])
  const [source, setSource] = useState('app')
  const [lines, setLines] = useState<string[]>([])
  const [query, setQuery] = useState('')
  const [level, setLevel] = useState<'all' | Severity>('all')
  const [live, setLive] = useState(true)
  const [follow, setFollow] = useState(true)
  const { error, setError, run } = useAction()
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const load = () => runCommand({ type: 'list_log_sources' }).then((r) => r.type === 'log_sources' && setSources(r.sources)).catch(() => undefined)
    void load()
    const t = setInterval(load, 5000)
    return () => clearInterval(t)
  }, [])

  useEffect(() => {
    let alive = true
    const tick = () =>
      runCommand({ type: 'read_log', source, max_lines: 2000 })
        .then((r) => alive && r.type === 'log_lines' && setLines(r.lines))
        .catch((e) => alive && setError(e))
    void tick()
    if (!live) return () => { alive = false }
    const t = setInterval(tick, 1500)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [source, live, setError])

  const shown = useMemo(() => {
    const q = query.toLowerCase()
    return lines
      .map((text) => ({ text, sev: severityOf(text) }))
      .filter((l) => (level === 'all' || l.sev === level) && (!q || l.text.toLowerCase().includes(q)))
  }, [lines, query, level])

  useEffect(() => {
    if (follow) box.current?.scrollTo({ top: box.current.scrollHeight })
  }, [shown.length, follow])

  async function clearLog() {
    const name = sources.find((s) => s.id === source)?.name ?? 'this log'
    if (!(await confirmAction(`Clear ${name}? Its lines are deleted for good; export first if you need them.`))) return
    await runCommand({ type: 'clear_log', source })
    setLines([])
  }

  async function exportLog() {
    const dest = await save({ title: 'Export log', defaultPath: `${source.replace(/[:]/g, '-')}.log` })
    if (dest) await runCommand({ type: 'export_log', source, dest })
  }

  return (
    <div className="flex h-full flex-col gap-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Logs</h1>
        <p className="text-sm text-muted-foreground">Application, web server, processes and Quick App runs. Secrets are redacted before they are written.</p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <Card>
        <CardContent className="flex flex-wrap items-center gap-3 pt-4">
          <Select value={source} onChange={(e) => setSource(e.target.value)} className="w-72">
            {sources.map((s) => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </Select>
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" className="w-56" />
          <Select value={level} onChange={(e) => setLevel(e.target.value as typeof level)} className="w-32">
            <option value="all">All levels</option>
            <option value="error">Errors</option>
            <option value="warn">Warnings</option>
            <option value="info">Info</option>
            <option value="debug">Debug</option>
          </Select>
          <Toggle checked={follow} onChange={setFollow} label="Follow" />
          <div className="ml-auto flex gap-1">
            <Button size="sm" variant="secondary" onClick={() => setLive(!live)}>
              {live ? <Pause /> : <Play />} {live ? 'Live' : 'Paused'}
            </Button>
            <Button size="sm" variant="ghost" title="Copy visible lines" onClick={() => navigator.clipboard.writeText(shown.map((l) => l.text).join('\n'))}>
              <Clipboard />
            </Button>
            <Button size="sm" variant="ghost" title="Export" onClick={() => run('export', exportLog)}>
              <Download />
            </Button>
            <Button size="sm" variant="ghost" title="Clear this log" onClick={() => run('clear', clearLog)}>
              <Eraser />
            </Button>
          </div>
        </CardContent>
      </Card>

      <div ref={box} className="min-h-72 flex-1 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-xl border border-border bg-card p-3 font-mono text-xs leading-relaxed">
        {shown.length === 0 && <p className="text-muted-foreground">No log lines{query || level !== 'all' ? ' match the filter' : ' yet'}.</p>}
        {shown.map((l, i) => (
          <div key={i} className={`whitespace-pre-wrap break-all ${SEVERITY_COLOR[l.sev]}`}>
            {l.text}
          </div>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">{shown.length} of {lines.length} lines</p>
    </div>
  )
}
