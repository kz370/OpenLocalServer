import { save } from '@tauri-apps/plugin-dialog'
import { Clipboard, Download, Eraser, Pause, Play, Sparkles } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type LogSource, runCommand } from '@/core'
import { askAi, useAiReady } from '@/lib/ai'
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
  const [pickerOpen, setPickerOpen] = useState(false)
  const [pickTab, setPickTab] = useState<'error' | 'warn' | 'both'>('both')
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const aiReady = useAiReady()
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

  /** Unique error/warn lines for AI picker (deduped ignoring timestamps, newest last, capped). */
  const candidates = useMemo(() => {
    const seen = new Map<string, { text: string; sev: Severity; count: number }>()
    for (const text of lines.slice(-2000)) {
      const sev = severityOf(text)
      if (sev !== 'error' && sev !== 'warn') continue
      const key = dedupeKey(text)
      const hit = seen.get(key)
      if (hit) hit.count += 1
      else seen.set(key, { text, sev, count: 1 })
    }
    return [...seen.values()].slice(-200)
  }, [lines])
  const errorCount = useMemo(() => candidates.filter((c) => c.sev === 'error').length, [candidates])
  const warnCount = candidates.length - errorCount
  const visibleCandidates = candidates.filter((c) => pickTab === 'both' || c.sev === pickTab)

  function openPicker() {
    setPickTab('both')
    setPicked(new Set(candidates.filter((c) => c.sev === 'error').map((c) => c.text)))
    setPickerOpen(true)
  }

  const EXCERPT_FILE_AT = 12000
  const pickedChars = useMemo(
    () => candidates.filter((c) => picked.has(c.text)).reduce((n, c) => n + c.text.length + 1, 0),
    [candidates, picked],
  )
  const pickedOverLimit = pickedChars > EXCERPT_FILE_AT

  function sendPickedToAi(withExcerpt: boolean) {
    const sourceName = sources.find((s) => s.id === source)?.name ?? source
    const excerpt = withExcerpt
      ? candidates.filter((c) => picked.has(c.text)).map((c) => (c.count > 1 ? `${c.text} (×${c.count})` : c.text)).join('\n')
      : undefined
    setPickerOpen(false)
    askAi({
      title: 'Ask the logs',
      description:
        excerpt && excerpt.length > 0
          ? excerpt.length > EXCERPT_FILE_AT
            ? `Sends ${picked.size} picked line${picked.size === 1 ? '' : 's'} from ${sourceName} as a .log text file (too long for inline) plus tail, secrets hidden.`
            : `Sends ${picked.size} picked line${picked.size === 1 ? '' : 's'} from ${sourceName} plus tail, secrets hidden.`
          : `Reads the last lines of ${sourceName}, secrets hidden.`,
      request: {
        feature: 'logs',
        log_sources: [source],
        ...(excerpt && excerpt.length > 0 ? { text: excerpt } : {}),
      },
      question: 'required',
      placeholder: 'e.g. Why does shop.test return a 502?',
    })
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
            {aiReady && (
              <Button size="sm" variant="secondary" title="Pick error/warn lines to send to the AI" onClick={openPicker}>
                <Sparkles /> Ask AI
              </Button>
            )}
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

      <Dialog
        open={pickerOpen}
        onClose={() => setPickerOpen(false)}
        title="Ask AI about log lines"
        description={`${candidates.length} unique error/warn lines (${errorCount} errors, ${warnCount} warns). Duplicates collapsed, repeat count shown. Pick lines to send, or send full tail.`}
        size="wide"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPickerOpen(false)}>Cancel</Button>
            <Button variant="secondary" onClick={() => sendPickedToAi(false)}>Send full tail</Button>
            <Button disabled={picked.size === 0} onClick={() => sendPickedToAi(true)}>
              <Sparkles /> {pickedOverLimit ? `Send as .log file (${picked.size})` : `Send ${picked.size} line${picked.size === 1 ? '' : 's'} to AI`}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <Select value={pickTab} onChange={(e) => setPickTab(e.target.value as typeof pickTab)} className="w-60 shrink-0">
              <option value="both">Errors + warns ({candidates.length})</option>
              <option value="error">Errors only ({errorCount})</option>
              <option value="warn">Warns only ({warnCount})</option>
            </Select>
            <Button size="sm" variant="ghost" onClick={() => setPicked(new Set(visibleCandidates.map((c) => c.text)))}>Select visible</Button>
            <Button size="sm" variant="ghost" onClick={() => setPicked(new Set<string>())}>Clear</Button>
            {picked.size > 0 && (
              <span className="text-xs text-muted-foreground">
                {pickedChars.toLocaleString()} chars{pickedOverLimit ? ' — over inline limit, sends as .log file' : ''}
              </span>
            )}
          </div>
          {visibleCandidates.length === 0 && (
            <p className="text-sm text-muted-foreground">No error/warn lines in this tab. Use “Send full tail” instead.</p>
          )}
          <div className="flex max-h-96 flex-col gap-1 overflow-y-auto rounded-lg border border-border p-2">
            {visibleCandidates.map((c) => (
              <label key={c.text} className="flex cursor-pointer items-start gap-2 rounded p-1 hover:bg-accent">
                <Checkbox
                  label={c.text.slice(0, 120)}
                  checked={picked.has(c.text)}
                  onChange={(on) =>
                    setPicked((prev) => {
                      const next = new Set(prev)
                      if (on) next.add(c.text)
                      else next.delete(c.text)
                      return next
                    })
                  }
                />
                <span className={`min-w-0 flex-1 break-all font-mono text-xs ${SEVERITY_COLOR[c.sev]}`}>
                  {c.text}
                  {c.count > 1 && (
                    <span className="ml-2 inline-block rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground">×{c.count}</span>
                  )}
                </span>
              </label>
            ))}
          </div>
        </div>
      </Dialog>
    </div>
  )
}
