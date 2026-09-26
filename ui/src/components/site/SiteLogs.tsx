import { CheckCircle2, Clipboard, Pause, Play, RefreshCw } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Select, Tabs, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CommandSource, type LogSource, runCommand } from '@/core'
import { SourceProblem, sourceCache } from '@/pages/Commands'
import { SEVERITY_COLOR, severityOf } from '@/pages/Logs'

/**
 * A site's problems and logs in one place: issues found while reading the project's tools
 * (a framework that can't boot, a missing PHP extension) and the logs that concern the site.
 */
export function SiteLogs({ hostname, projectId, projectName }: { hostname: string | null; projectId: string | null; projectName: string | null }) {
  const [view, setView] = useState<'issues' | 'logs'>('issues')
  const [sources, setSources] = useState<CommandSource[] | null>(projectId ? (sourceCache.get(projectId) ?? null) : [])
  const [checking, setChecking] = useState(false)

  async function check(force = false) {
    if (!projectId) return
    const cached = sourceCache.get(projectId)
    if (cached && !force) return setSources(cached)
    setChecking(true)
    try {
      const r = await runCommand({ type: 'discover_commands', project_id: projectId })
      if (r.type === 'command_sources') {
        sourceCache.set(projectId, r.sources)
        setSources(r.sources)
      }
    } finally {
      setChecking(false)
    }
  }

  useEffect(() => {
    check().catch(() => setSources([]))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId])

  const issues = (sources ?? []).filter((s) => s.error || s.warning)

  return (
    <div className="flex flex-col gap-4">
      <Tabs
        tabs={[
          { id: 'issues', label: 'Issues', badge: sources === null ? undefined : issues.length },
          { id: 'logs', label: 'Logs' },
        ]}
        value={view}
        onChange={setView}
      />

      {view === 'issues' && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-muted-foreground">Problems found while reading the project's tools (artisan, composer, npm, manage.py). The Repair section can fix many of them.</p>
            {projectId && (
              <Button size="sm" variant="ghost" disabled={checking} onClick={() => void check(true)}>
                {checking ? <Spinner /> : <RefreshCw className="size-3.5" />} Check again
              </Button>
            )}
          </div>
          {!projectId && <p className="text-sm text-muted-foreground">This site has no project, so there is nothing to check.</p>}
          {projectId && sources === null && <p className="text-sm text-muted-foreground">Checking the project…</p>}
          {sources !== null && projectId && issues.length === 0 && (
            <p className="flex items-center gap-2 text-sm text-muted-foreground">
              <CheckCircle2 className="size-4 text-success" /> No issues found.
            </p>
          )}
          {issues.map((s) =>
            s.error ? (
              <SourceProblem key={s.id} tone="error" title={`${s.label}: could not read its commands.`} text={s.error} />
            ) : (
              <SourceProblem key={s.id} tone="warning" title={s.label} text={s.warning!} />
            ),
          )}
        </div>
      )}

      {view === 'logs' && <LogViewer hostname={hostname} projectName={projectName} />}
    </div>
  )
}

/** The web server's logs (narrowed to this site where the log says which site a line is for) and the site's processes. */
function LogViewer({ hostname, projectName }: { hostname: string | null; projectName: string | null }) {
  const [all, setAll] = useState<LogSource[]>([])
  const [source, setSource] = useState('web:error')
  const [lines, setLines] = useState<string[]>([])
  const [onlySite, setOnlySite] = useState(true)
  const [query, setQuery] = useState('')
  const [live, setLive] = useState(true)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    runCommand({ type: 'list_log_sources' })
      .then((r) => r.type === 'log_sources' && setAll(r.sources))
      .catch(() => undefined)
  }, [])

  // The web server's logs, plus processes that belong to this site (its app, the project's workers).
  const names = [hostname, projectName].filter(Boolean).map((n) => n!.toLowerCase())
  const mine = all.filter((s) => s.kind === 'web' || (s.kind === 'process' && names.some((n) => s.name.toLowerCase().includes(n))))

  useEffect(() => {
    let alive = true
    const tick = () =>
      runCommand({ type: 'read_log', source, max_lines: 2000 })
        .then((r) => alive && r.type === 'log_lines' && setLines(r.lines))
        .catch(() => alive && setLines([]))
    void tick()
    if (!live) return () => void (alive = false)
    const t = setInterval(tick, 2000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [source, live])

  // The error log names the site on each line ("server: shop.test"); the access log does not.
  const canNarrow = source === 'web:error' && !!hostname
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return lines
      .filter((l) => !(canNarrow && onlySite) || l.toLowerCase().includes(hostname!.toLowerCase()))
      .filter((l) => !q || l.toLowerCase().includes(q))
      .map((text) => ({ text, sev: severityOf(text) }))
  }, [lines, query, canNarrow, onlySite, hostname])

  useEffect(() => {
    box.current?.scrollTo({ top: box.current.scrollHeight })
  }, [shown.length])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <Select value={source} onChange={(e) => setSource(e.target.value)} className="w-64">
          {mine.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </Select>
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" className="w-48" />
        {canNarrow && <Toggle checked={onlySite} onChange={setOnlySite} label={`Only ${hostname}`} />}
        <div className="ml-auto flex gap-1">
          <Button size="sm" variant="ghost" className="h-8 w-8 px-0" title={live ? 'Pause' : 'Resume'} onClick={() => setLive(!live)}>
            {live ? <Pause className="size-3.5" /> : <Play className="size-3.5" />}
          </Button>
          <Button size="sm" variant="ghost" className="h-8 w-8 px-0" title="Copy visible lines" onClick={() => navigator.clipboard.writeText(shown.map((l) => l.text).join('\n'))}>
            <Clipboard className="size-3.5" />
          </Button>
        </div>
      </div>
      {source === 'web:access' && <p className="text-xs text-muted-foreground">The access log is shared by every site and doesn't say which site a request was for.</p>}
      <div ref={box} className="h-80 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-lg border border-border bg-muted/30 p-3 font-mono text-xs leading-relaxed">
        {shown.length === 0 && <p className="text-muted-foreground">No log lines{query || (canNarrow && onlySite) ? ' match' : ' yet'}.</p>}
        {shown.map((l, i) => (
          <div key={i} className={SEVERITY_COLOR[l.sev]}>
            {l.text}
          </div>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">
        {shown.length} of {lines.length} lines. Every log is on the Logs page too.
      </p>
    </div>
  )
}
