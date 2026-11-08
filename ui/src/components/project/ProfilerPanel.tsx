import { Gauge, RefreshCw, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { type CallNode, type Diagnostic, type DomainSummary, type ProjectDetail, type XdebugProfile, type XdebugProfileFile, type XdebugReport, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { asDiagnostic } from '@/lib/diagnostic'
import { formatBytes, timeAgo } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const ms = (ns: number) => (ns / 1_000_000 >= 100 ? (ns / 1_000_000).toFixed(0) : (ns / 1_000_000).toFixed(1))

type SortKey = 'inclusive_ns' | 'self_ns' | 'calls'

/**
 * §174 Profiler: turn on Xdebug's profile mode for the site's PHP version, load a page with
 * `?XDEBUG_TRIGGER=1`, then see where the time went: top functions and an approximate call
 * chart. Everything is drawn with plain elements; there is no chart library.
 */
export function ProfilerPanel({ detail }: { detail: ProjectDetail }) {
  const php = detail.resolved.find((r) => r.id === 'php')?.installed_version ?? null
  const projectId = detail.project.id
  const [sites, setSites] = useState<DomainSummary[]>([])
  const [report, setReport] = useState<XdebugReport | null>(null)
  const [files, setFiles] = useState<XdebugProfileFile[]>([])
  const [open, setOpen] = useState<{ file: string; profile: XdebugProfile } | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const site = sites.find((s) => s.kind === 'php') ?? null
  const profiling = report?.settings.modes.includes('profile') ?? false

  const load = useCallback(async () => {
    try {
      const d = await runCommand({ type: 'list_domains' })
      if (d.type === 'domains') setSites(d.domains.filter((x) => x.project_id === projectId))
      if (php) {
        const x = await runCommand({ type: 'get_xdebug', version: php })
        if (x.type === 'xdebug') setReport(x.report)
      }
      const f = await runCommand({ type: 'list_xdebug_profiles' })
      if (f.type === 'xdebug_profiles') setFiles(f.files)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }, [projectId, php])
  useEffect(() => {
    void load()
  }, [load])

  const mine = useMemo(() => {
    const hosts = new Set(sites.map((s) => s.hostname.toLowerCase()))
    return files.filter((f) => f.host && hosts.has(f.host.toLowerCase()))
  }, [files, sites])

  async function toggle(on: boolean) {
    if (!site) return
    setBusy('toggle')
    setError(null)
    try {
      const r = await runCommand({ type: 'set_site_profiling', hostname: site.hostname, on })
      if (r.type === 'xdebug') setReport(r.report)
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(null)
    }
  }

  async function openFile(file: string) {
    setBusy(file)
    setError(null)
    try {
      const r = await runCommand({ type: 'get_xdebug_profile', file })
      if (r.type === 'xdebug_profile') setOpen({ file, profile: r.profile })
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(null)
    }
  }

  async function remove(file: string) {
    if (!(await confirmAction(`Delete this profile (${file})?`))) return
    try {
      await runCommand({ type: 'delete_xdebug_profiles', files: [file] })
      if (open?.file === file) setOpen(null)
      await load()
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  if (!php) return <p className="text-sm text-muted-foreground">This project has no PHP version resolved. Install PHP from the Runtimes page.</p>
  if (!site) return <p className="text-sm text-muted-foreground">This project has no PHP site yet. Add a site for it first.</p>

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border px-3 py-2 text-sm">
        <label className="flex items-center gap-2">
          <Switch checked={profiling} onChange={(v) => void toggle(v)} disabled={busy !== null} label="Profile requests that ask for it" />
          <span>Profile requests that ask for it</span>
          {report && !report.enabled && <Badge variant="outline">Xdebug is off for PHP {php}</Badge>}
        </label>
        <Button
          size="sm"
          disabled={!profiling}
          className="cursor-pointer"
          onClick={() => void runCommand({ type: 'open_url', url: `${site.url.replace(/\/$/, '')}/?XDEBUG_TRIGGER=1` }).catch((e) => setError(asDiagnostic(e)))}
        >
          <Gauge /> Profile a request
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Only requests that carry <code>XDEBUG_TRIGGER</code> are profiled, so everything else stays as fast as before. The button opens {site.hostname} with <code>?XDEBUG_TRIGGER=1</code>; that query string can confuse some routes. For any page, use the Xdebug helper browser extension or a cookie <code>XDEBUG_TRIGGER=1</code>. Xdebug is set per PHP version, so this applies to every site on PHP {php}.
      </p>

      <section className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium">Profiles · {mine.length}</h3>
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => void load()}>
            <RefreshCw /> Refresh
          </Button>
        </div>
        {mine.length === 0 && <p className="text-sm text-muted-foreground">None yet. Turn profiling on, then press Profile a request.</p>}
        {mine.map((f) => (
          <div key={f.file} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2">
            <span className="min-w-0 text-sm">
              {f.host} <span className="text-xs text-muted-foreground">· {timeAgo(f.created_ms)} · {formatBytes(f.size)} · PHP {f.php_version}</span>
            </span>
            <span className="flex gap-1">
              <Button size="sm" variant="secondary" className="cursor-pointer" disabled={busy !== null} onClick={() => void openFile(f.file)}>
                {busy === f.file ? <Spinner /> : null} Open
              </Button>
              <Button size="icon" variant="ghost" className="size-8 cursor-pointer" title="Delete" onClick={() => void remove(f.file)}>
                <Trash2 />
              </Button>
            </span>
          </div>
        ))}
      </section>

      {open && <Viewer profile={open.profile} />}
    </div>
  )
}

function Viewer({ profile }: { profile: XdebugProfile }) {
  const [sort, setSort] = useState<SortKey>('inclusive_ns')
  const [hover, setHover] = useState<CallNode | null>(null)
  const rows = useMemo(() => [...profile.functions].sort((a, b) => b[sort] - a[sort]).slice(0, 50), [profile, sort])
  const total = profile.total_time_ns || 1
  const head = (key: SortKey, label: string) => (
    <th className="px-2 py-1 text-right font-medium">
      <button type="button" className={cn('cursor-pointer hover:underline', sort === key && 'text-primary')} onClick={() => setSort(key)}>
        {label}
      </button>
    </th>
  )
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border p-3">
      <p className="text-sm">
        Total time: <strong>{ms(profile.total_time_ns)} ms</strong>
      </p>
      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="border-b text-muted-foreground">
              <th className="px-2 py-1 text-left font-medium">Top functions</th>
              {head('self_ns', 'Self ms')}
              {head('inclusive_ns', 'Total ms')}
              {head('calls', 'Calls')}
            </tr>
          </thead>
          <tbody>
            {rows.map((f, i) => (
              <tr
                key={`${f.name}:${i}`}
                className={cn('border-b last:border-0', f.file && !f.file.startsWith('php:') && 'cursor-pointer hover:bg-muted/40')}
                title={f.file}
                onClick={() => f.file && !f.file.startsWith('php:') && void runCommand({ type: 'open_in_editor', path: f.file }).catch(() => undefined)}
              >
                <td className="max-w-80 truncate px-2 py-1 font-mono">{f.name}</td>
                <td className="px-2 py-1 text-right tabular-nums">{ms(f.self_ns)}</td>
                <td className="px-2 py-1 text-right tabular-nums">{ms(f.inclusive_ns)}</td>
                <td className="px-2 py-1 text-right tabular-nums">{f.calls}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex flex-col gap-1">
        <p className="text-xs text-muted-foreground">Call chart (approximate: Xdebug records totals per function, not every path). Width is the share of the parent.</p>
        <div className="overflow-x-auto">
          <div className="min-w-[32rem]">
            <Icicle node={profile.tree} depth={0} onHover={setHover} />
          </div>
        </div>
        <p className="h-4 truncate font-mono text-xs text-muted-foreground">
          {hover ? `${hover.name} · ${ms(hover.inclusive_ns)} ms · ${((hover.inclusive_ns / total) * 100).toFixed(1)}% · ${hover.calls} calls` : ''}
        </p>
      </div>
    </section>
  )
}

const HUES = [210, 160, 30, 280, 340, 90]

/** One row per depth; a child's width is its share of its parent. Plain divs only. */
function Icicle({ node, depth, onHover }: { node: CallNode; depth: number; onHover: (n: CallNode | null) => void }) {
  return (
    <div className="flex w-full flex-col">
      <div
        className="h-5 truncate rounded-[2px] border border-background px-1 text-[10px] leading-5 text-white"
        style={{ background: `hsl(${HUES[depth % HUES.length]} 55% 38%)` }}
        title={`${node.name} · ${ms(node.inclusive_ns)} ms`}
        onMouseEnter={() => onHover(node)}
        onMouseLeave={() => onHover(null)}
      >
        {node.name}
      </div>
      {node.children.length > 0 && (
        <div className="flex w-full items-start">
          {node.children.map((c, i) => (
            <div key={`${c.name}:${i}`} style={{ width: `${Math.max(0.5, (c.inclusive_ns / Math.max(1, node.inclusive_ns)) * 100)}%` }} className="min-w-0">
              <Icicle node={c} depth={depth + 1} onHover={onHover} />
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
