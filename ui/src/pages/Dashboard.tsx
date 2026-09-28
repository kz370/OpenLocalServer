import { AlertTriangle, CheckCircle2, Home, Play, RefreshCw, Rocket, RotateCw, XCircle } from 'lucide-react'
import { type ReactNode, memo, useMemo, useState } from 'react'

import { DiagnosticsCard } from '@/components/DiagnosticsCard'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { ActionMenu, type MenuItem } from '@/components/ui/menu'
import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DashboardData, type HealthItem, type ServerAvailability, type ServiceStatus, type StartupSettings, type SystemStats, runCommand } from '@/core'
import { formatBytes, useAction, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { waitForService, waitForWebStopped } from '@/lib/wait'
import { confirmAction } from '@/lib/confirm'

/** §173 / §116 / §101: what's running, what's wrong, and one-click ways to act on it. */
export function DashboardPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [data, setData] = useState<DashboardData | null>(null)
  const [diagToken, setDiagToken] = useState(0)
  const [stats, setStats] = useState<SystemStats | null>(null)
  const [accessLines, setAccessLines] = useState<string[]>([])
  const [startup, setStartup] = useState<StartupSettings | null>(null)
  const { busy, error, setError, run } = useAction()

  usePoll(async () => {
    try {
      const res = await runCommand({ type: 'get_dashboard' })
      if (res.type === 'dashboard') setData(res.data)
    } catch {
      /* the core is busy or restarting; the next poll retries */
    }
  }, 3000)

  // Machine load next to sites, not buried on Processes page.
  usePoll(async () => {
    const r = await runCommand({ type: 'get_system_stats' }).catch(() => null)
    if (r?.type === 'system_stats') setStats(r.stats)
  }, 3000)

  // Traffic from web access log; no new backend needed.
  usePoll(async () => {
    const r = await runCommand({ type: 'read_log', source: 'web:access', max_lines: 400 }).catch(() => null)
    if (r?.type === 'log_lines') setAccessLines(r.lines)
  }, 5000)

  // Start/Stop all acts only on auto-startup set (Settings → Startup).
  usePoll(async () => {
    const r = await runCommand({ type: 'get_startup_settings' }).catch(() => null)
    if (r?.type === 'startup') setStartup(r.settings)
  }, 5000)

  const refresh = async () => {
    const res = await runCommand({ type: 'get_dashboard' })
    if (res.type === 'dashboard') setData(res.data)
  }

  const web = data?.web
  const runningServices = data?.services.filter((s) => s.running) ?? []
  const autoIds = startup?.autostart_services ?? []
  const autoRunning = runningServices.filter((s) => autoIds.includes(s.id))
  const autoStopped = (data?.services ?? []).filter((s) => s.installed && !s.running && autoIds.includes(s.id))
  const autoWebRunning = !!startup?.autostart_web && !!web?.running
  const hasAutoRunning = autoWebRunning || autoRunning.length > 0
  const autoConfigured = !!startup?.autostart_web || autoIds.length > 0
  const problems = data?.health.filter((h) => h.status !== 'ok') ?? []

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
          <p className="text-sm text-muted-foreground">Your local environment at a glance.</p>
        </div>
        <div className="flex max-w-full flex-wrap items-center justify-start gap-1 rounded-lg border border-border bg-card/50 p-1 shadow-sm sm:justify-end">
          <Button variant="secondary" size="sm" className="h-8 rounded-md px-3 text-[13px] font-medium [&_svg]:size-3.5" onClick={() => onNavigate('quickapps')}>
            <Rocket /> New from Quick App
          </Button>
          <div aria-hidden className="mx-1 h-5 w-px bg-border" />
          <Button
            variant="secondary"
            size="sm"
            className="h-8 rounded-md px-3 text-[13px] font-medium [&_svg]:size-3.5"
            disabled={busy !== null}
            onClick={() =>
              run('apply', async () => {
                await runCommand({ type: 'apply_web', overwrite: [] })
                await refresh()
                setDiagToken((t) => t + 1)
              })
            }
          >
            {busy === 'apply' ? <Spinner /> : <Play />} {busy === 'apply' ? (web?.running ? 'Applying…' : 'Starting…') : web?.running ? 'Re-apply web config' : 'Start web server'}
          </Button>
          <Button
            variant={hasAutoRunning ? 'secondary' : 'default'}
            size="sm"
            className={`h-8 min-w-28 rounded-md px-3 text-[13px] font-medium [&_svg]:size-3.5 ${hasAutoRunning ? 'bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive dark:bg-destructive/[0.18] dark:text-red-300/90 dark:hover:bg-destructive/25' : ''}`}
            disabled={busy !== null || !autoConfigured}
            title={autoConfigured ? 'Only auto-startup services (Settings → Startup)' : 'No auto-startup services set (Settings → Startup)'}
            onClick={() =>
              hasAutoRunning
                ? run('stop-all', async () => {
                    await Promise.all([
                      ...(autoWebRunning ? [runCommand({ type: 'stop_web' })] : []),
                      ...autoRunning.map((service) => runCommand({ type: 'stop_service', id: service.id })),
                    ])
                    await Promise.all([
                      ...(autoWebRunning ? [waitForWebStopped()] : []),
                      ...autoRunning.map((service) => waitForService(service.id, 'stopped')),
                    ])
                    await refresh()
                    setDiagToken((t) => t + 1)
                  })
                : run('start-all', async () => {
                    if (startup?.autostart_web && !web?.running) await runCommand({ type: 'apply_web', overwrite: [] })
                    await Promise.all(autoStopped.map((service) => runCommand({ type: 'start_service', id: service.id })))
                    await Promise.all(autoStopped.map((service) => waitForService(service.id, 'running')))
                    await refresh()
                    setDiagToken((t) => t + 1)
                  })
            }
          >
            {busy === 'stop-all' || busy === 'start-all' ? (
              <Spinner />
            ) : hasAutoRunning ? (
              <StopIcon />
            ) : (
              <Play />
            )}{' '}
            {busy === 'stop-all' ? 'Stopping all…' : busy === 'start-all' ? 'Starting all…' : hasAutoRunning ? 'Stop all' : 'Start all'}
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {problems.length > 0 && (
        <Card className="border-warning/40">
          <CardHeader className="pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <AlertTriangle className="size-4 text-warning" /> Needs attention
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {problems.map((h) => (
              <HealthRow key={h.id + h.detail} item={h} />
            ))}
          </CardContent>
        </Card>
      )}

      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
            <div>
              <CardTitle className="text-sm">Overview</CardTitle>
              <CardDescription>
                {data ? `${data.domains.length} sites · ${data.project_count} projects` : 'Loading…'}
              </CardDescription>
            </div>
            <Button size="sm" variant="secondary" onClick={() => onNavigate('sites')}>
              Manage Sites
              <Badge variant="secondary" className="ml-1 tabular-nums">
                {data?.domains.length ?? '…'}
              </Badge>
            </Button>
          </CardHeader>
          <CardContent className="grid gap-6 md:grid-cols-2">
            <div className="flex flex-col gap-3">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">Resources</span>
                <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => onNavigate('processes')}>
                  Details
                </Button>
              </div>
              <div className="flex flex-wrap items-start justify-around gap-4">
                <Donut
                  label="CPU"
                  percent={stats?.cpu_percent ?? 0}
                  detail={stats ? `${stats.cpu_cores} cores` : '…'}
                  base="var(--primary)"
                  loading={!stats}
                />
                <Donut
                  label="RAM"
                  percent={stats ? (stats.memory_used / Math.max(stats.memory_total, 1)) * 100 : 0}
                  detail={stats ? `${formatBytes(stats.memory_used)} / ${formatBytes(stats.memory_total)}` : '…'}
                  base="oklch(0.68 0.12 220)"
                  loading={!stats}
                />
                {(stats?.disks ?? []).slice(0, 1).map((d) => (
                  <Donut
                    key={d.mount}
                    label={`Disk ${d.mount.replace(/\\$/, '')}`}
                    percent={(d.used / Math.max(d.total, 1)) * 100}
                    detail={stats ? `${formatBytes(d.total - d.used)} free` : '…'}
                    base="oklch(0.66 0.16 295)"
                    loading={!stats}
                  />
                ))}
              </div>
            </div>
            <div className="flex min-w-0 flex-col gap-3">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">Traffic</span>
                <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => onNavigate('logs')}>
                  Logs
                </Button>
              </div>
              <TrafficGraph lines={accessLines} tall />
            </div>
          </CardContent>
        </Card>

        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm">Web server</CardTitle>
              {web?.running ? <Badge variant="success">● Running</Badge> : <Badge variant="secondary">Stopped</Badge>}
            </CardHeader>
            <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
              <div>
                {web?.servers.find((s) => s.active)?.name ?? '…'} · ports {web?.http_port} / {web?.https_port}
              </div>
              {web?.php_pools.map((p) => (
                <div key={p.version}>
                  PHP {p.version} · {p.ports.length} worker{p.ports.length === 1 ? '' : 's'} {p.running ? '' : '(stopped)'}
                </div>
              ))}
              {web?.dns_running && <div>Wildcard DNS on port {web.dns_port}</div>}
              <div className="pt-1">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={!web?.running}
                  title={web?.running ? 'Open the built-in welcome site' : 'Start the web server first'}
                  onClick={() => runCommand({ type: 'open_url', url: 'https://openlocalserver.test' }).catch(() => undefined)}
                >
                  <Home /> Open home page
                </Button>
              </div>
            </CardContent>
          </Card>

          <ServicesWidget
            services={data?.services ?? null}
            webServers={data?.web?.servers ?? null}
            busy={busy}
            onNavigate={onNavigate}
            onDone={async () => {
              await refresh()
              setDiagToken((t) => t + 1)
            }}
          />
        </div>
      </div>
      <DiagnosticsCard refreshToken={diagToken} />


      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Environment health</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-1.5 md:grid-cols-2">
          {data?.health.map((h) => <HealthRow key={h.id + h.detail} item={h} />)}
        </CardContent>
      </Card>
    </div>
  )
}

/** What the port column means for this row: a web server's HTTP port, or the service's own. */
function servicePortTitle(s: ServiceStatus, sites?: number): string {
  if (s.kind !== 'web') return `Port ${s.port}`
  if (sites === 0) return `HTTP port ${s.port} — no sites assigned to ${s.name} yet, so it serves nothing`
  return `HTTP port ${s.port} — ${sites} site${sites === 1 ? '' : 's'} on ${s.name}`
}

/** The services the widget shows, in a fixed order, so the list never re-shuffles. */
const WIDGET_ORDER = ['mailpit', 'mariadb', 'postgres', 'mongodb', 'redis', 'nginx', 'apache', 'caddy'] as const

type WidgetAction = 'start' | 'stop' | 'restart' | 'reload'

/**
 * The dashboard's Services card: one compact row per service, each with its own logo,
 * port and state, and icon-only controls. Nothing here scrolls — the eight built-in
 * services always fit.
 */
function ServicesWidget({
  services,
  webServers,
  busy,
  onNavigate,
  onDone,
}: {
  services: ServiceStatus[] | null
  webServers: ServerAvailability[] | null
  busy: string | null
  onNavigate: (p: Page) => void
  onDone: () => Promise<void>
}) {
  const { run } = useAction()
  const byId = new Map((services ?? []).map((s) => [s.id, s]))
  // Always the same eight rows, installed or not, so the card keeps a stable height.
  const rows = WIDGET_ORDER.map((id) => byId.get(id)).filter((s): s is ServiceStatus => !!s)
  const running = rows.filter((s) => s.running).length
  const installed = rows.filter((s) => s.installed).length
  // How many sites each web server renders — a running server with none opens no
  // listener, which is different from one that has stopped answering.
  const webSites = new Map((webServers ?? []).map((s) => [s.id, s.sites]))

  async function act(s: ServiceStatus, action: WidgetAction) {
    if (action === 'stop' && !(await confirmAction(`Stop ${s.name}? Anything connected to it will be disconnected.`))) return
    if (action === 'restart' && s.running && !(await confirmAction(`Restart ${s.name}? Anything connected to it will be disconnected.`))) return
    void run(`svc:${s.id}:${action}`, async () => {
      if (action === 'reload') {
        // A web server reloads its config in place: no dropped connections.
        await runCommand({ type: 'apply_web', overwrite: [] })
      } else if (action === 'restart') {
        await runCommand({ type: 'restart_service', id: s.id })
        await waitForService(s.id, 'running')
      } else {
        await runCommand({ type: action === 'start' ? 'start_service' : 'stop_service', id: s.id })
        await waitForService(s.id, action === 'start' ? 'running' : 'stopped')
      }
      await onDone()
    })
  }

  return (
    <Card className="overflow-hidden py-0">
      <CardHeader className="flex-row items-center justify-between space-y-0 px-3 py-2.5">
        <div className="flex min-w-0 items-baseline gap-2">
          <CardTitle className="text-sm">Services</CardTitle>
          <CardDescription className="truncate text-xs tabular-nums">
            {services === null ? 'Loading…' : `${running} running · ${installed} installed`}
          </CardDescription>
        </div>
        <Button size="sm" variant="ghost" className="h-7 shrink-0 rounded-md px-2 text-xs" onClick={() => onNavigate('services')}>
          Manage
        </Button>
      </CardHeader>
      <CardContent className="p-0">
        {rows.map((s, i) => {
          const working = busy === `svc:${s.id}:start` || busy === `svc:${s.id}:stop` || busy === `svc:${s.id}:restart` || busy === `svc:${s.id}:reload`
          const state = !s.installed ? 'missing' : s.running ? (s.healthy === false ? 'unhealthy' : 'running') : 'stopped'
          return (
            <div
              key={s.id}
              className={`group flex items-center gap-2 px-3 py-1.5 transition-colors hover:bg-muted/40 ${i > 0 ? 'border-t border-border/60' : ''}`}
            >
              <span className={cn('shrink-0', !s.installed && 'opacity-40 grayscale')}>
                <TechIcon id={s.id} className="size-4" />
              </span>
              <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{s.name}</span>
              <span
                className="shrink-0 font-mono text-[11px] tabular-nums text-muted-foreground"
                title={servicePortTitle(s, webSites.get(s.id))}
              >
                {s.port === null ? '—' : s.port}
              </span>
              <StateBadge state={state} />
              <span className="flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                <IconAction
                  title={s.running ? `Stop ${s.name}` : `Start ${s.name}`}
                  disabled={busy !== null || !s.installed}
                  spinning={working}
                  onClick={() => act(s, s.running ? 'stop' : 'start')}
                >
                  {s.running ? <StopIcon className="size-3.5" /> : <Play className="size-3.5" />}
                </IconAction>
                <IconAction
                  title="Restart"
                  disabled={busy !== null || !s.installed}
                  onClick={() => act(s, 'restart')}
                >
                  <RotateCw className="size-3.5" />
                </IconAction>
                {s.kind === 'web' && (
                  <IconAction title="Reload config" disabled={busy !== null || !s.installed} onClick={() => act(s, 'reload')}>
                    <RefreshCw className="size-3.5" />
                  </IconAction>
                )}
              </span>
              <ActionMenu
                label={`More actions for ${s.name}`}
                items={menuFor(s, act, onNavigate)}
              />
            </div>
          )
        })}
      </CardContent>
    </Card>
  )
}

type WidgetState = 'running' | 'stopped' | 'unhealthy' | 'missing'

function menuFor(
  s: ServiceStatus,
  act: (s: ServiceStatus, action: WidgetAction) => void,
  onNavigate: (p: Page) => void
): MenuItem[] {
  const items: MenuItem[] = [
    { label: s.running ? 'Stop' : 'Start', onSelect: () => act(s, s.running ? 'stop' : 'start'), disabled: !s.installed },
    { label: 'Restart', onSelect: () => act(s, 'restart'), disabled: !s.installed },
  ]
  if (s.kind === 'web') {
    items.push({ label: 'Reload config', onSelect: () => act(s, 'reload'), disabled: !s.installed })
  }
  items.push('separator', { label: 'Open in Services', onSelect: () => onNavigate('services') })
  items.push({ label: 'View logs', onSelect: () => onNavigate('logs') })
  if (!s.installed) {
    items.push({ label: 'Install from Runtimes', onSelect: () => onNavigate('runtimes') })
  }
  return items
}

function StateBadge({ state }: { state: WidgetState }) {
  const label = state === 'running' ? 'Running' : state === 'unhealthy' ? 'Not responding' : state === 'missing' ? 'Not installed' : 'Stopped'
  return (
    <span
      className={cn(
        'w-[5.75rem] shrink-0 truncate rounded-full px-1.5 py-0.5 text-center text-[10px] font-medium leading-4',
        state === 'running' && 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
        state === 'unhealthy' && 'bg-amber-500/15 text-amber-600 dark:text-amber-400',
        state === 'stopped' && 'bg-muted text-muted-foreground',
        state === 'missing' && 'bg-muted/50 text-muted-foreground/70'
      )}
    >
      {label}
    </span>
  )
}

function IconAction({
  title,
  disabled,
  spinning,
  onClick,
  children,
}: {
  title: string
  disabled: boolean
  spinning?: boolean
  onClick: () => void
  children: ReactNode
}) {
  return (
    <Button
      size="sm"
      variant="ghost"
      className="size-6 shrink-0 cursor-pointer rounded-md p-0 text-muted-foreground hover:text-foreground [&_svg]:size-3.5"
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
    >
      {spinning ? <Spinner className="size-3.5" /> : children}
    </Button>
  )
}

function Donut({ label, percent, detail, base, loading }: { label: string; percent: number; detail: string; base: string; loading?: boolean }) {  const r = 34
  const c = 2 * Math.PI * r
  const p = Math.min(100, Math.max(0, percent))
  const color = p >= 90 ? 'var(--destructive)' : base
  return (
    <div className="flex w-28 flex-col items-center gap-1.5 text-center">
      <div className="relative size-24">
        <svg viewBox="0 0 80 80" className="size-full -rotate-90">
          <circle cx="40" cy="40" r={r} fill="none" stroke="var(--muted)" strokeWidth="8" />
          <circle cx="40" cy="40" r={r} fill="none" stroke={color} strokeWidth="8" strokeLinecap="round" strokeDasharray={c} strokeDashoffset={c * (1 - p / 100)} className="transition-[stroke-dashoffset,stroke] duration-500" />
        </svg>
        <span className="absolute inset-0 flex items-center justify-center text-lg font-semibold tabular-nums" style={{ color }}>
          {loading ? '…' : `${p.toFixed(0)}%`}
        </span>
      </div>
      <span className="text-sm font-medium">{label}</span>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  )
}

const MONTHS: Record<string, number> = { Jan: 0, Feb: 1, Mar: 2, Apr: 3, May: 4, Jun: 5, Jul: 6, Aug: 7, Sep: 8, Oct: 9, Nov: 10, Dec: 11 }

function parseAccessTime(line: string): number | null {
  const m = line.match(/\[(\d{2})\/(\w{3})\/(\d{4}):(\d{2}):(\d{2}):(\d{2})/)
  if (!m) return null
  const month = MONTHS[m[2]]
  if (month === undefined) return null
  return Date.UTC(Number(m[3]), month, Number(m[1]), Number(m[4]), Number(m[5]), Number(m[6]))
}

function parseAccessLine(line: string): { t: number | null; bytes: number } {
  const t = parseAccessTime(line)
  const m = line.match(/"\s(\d{3})\s(\d+|-)/)
  const bytes = m && m[2] !== '-' ? Number(m[2]) || 0 : 0
  return { t, bytes }
}

const UP_COLOR = '#22c55e'
const DOWN_COLOR = '#f59e0b'

function smoothPath(pts: { x: number; y: number }[]): string {
  if (pts.length === 0) return ''
  if (pts.length === 1) return `M ${pts[0].x},${pts[0].y}`
  let d = `M ${pts[0].x},${pts[0].y}`
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(0, i - 1)]
    const p1 = pts[i]
    const p2 = pts[i + 1]
    const p3 = pts[Math.min(pts.length - 1, i + 2)]
    const c1x = p1.x + (p2.x - p0.x) / 6
    const c1y = p1.y + (p2.y - p0.y) / 6
    const c2x = p2.x - (p3.x - p1.x) / 6
    const c2y = p2.y - (p3.y - p1.y) / 6
    d += ` C ${c1x},${c1y} ${c2x},${c2y} ${p2.x},${p2.y}`
  }
  return d
}

const TrafficGraph = memo(function TrafficGraph({ lines, tall }: { lines: string[]; tall?: boolean }) {
  // Stable while lines unchanged: window anchors to last log timestamp,
  // not Date.now() per render (that slid buckets every 3s poll → jitter).
  const { req, kb, end } = useMemo(() => {
    const N = 30
    const WIN_MS = 5 * 60 * 1000
    const r = new Array(N).fill(0) as number[]
    const k = new Array(N).fill(0) as number[]
    let latest = 0
    const parsed = lines.map(parseAccessLine)
    for (const p of parsed) if (p.t !== null && p.t > latest) latest = p.t
    const endMs = latest > 0 ? latest : Date.now()
    let unparsed = 0
    let unparsedBytes = 0
    for (const p of parsed) {
      if (p.t === null) {
        unparsed += 1
        unparsedBytes += p.bytes
        continue
      }
      const age = endMs - p.t
      if (age < 0 || age >= WIN_MS) continue
      const idx = N - 1 - Math.floor((age / WIN_MS) * N)
      r[idx] += 1
      k[idx] += p.bytes / 1024
    }
    if (unparsed > 0) {
      r[N - 1] += unparsed
      k[N - 1] += unparsedBytes / 1024
    }
    return { req: r, kb: k, end: endMs }
  }, [lines])
  const N = req.length
  const WIN_MS = 5 * 60 * 1000
  const [hover, setHover] = useState<number | null>(null)
  const maxV = Math.max(1, ...req, ...kb)
  const niceMax = Math.ceil(maxV)
  const W = 300
  const H = 120
  const PAD_L = 22
  const pts = (series: number[]) =>
    series.map((v, i) => ({ x: PAD_L + (i / (N - 1)) * (W - PAD_L - 4), y: 6 + (1 - v / niceMax) * (H - 26) }))
  const upPts = pts(req)
  const downPts = pts(kb)
  const upLine = smoothPath(upPts)
  const downLine = smoothPath(downPts)
  const area = (line: string) => `${line} L ${W - 4},${H - 18} L ${PAD_L},${H - 18} Z`
  const totalReq = req.reduce((a, b) => a + b, 0)
  const totalKb = kb.reduce((a, b) => a + b, 0)
  const fmtT = (ms: number) => new Date(ms).toTimeString().slice(0, 8)
  const bucketT = (i: number) => end - ((N - 1 - i) / N) * WIN_MS
  const ticks = [...new Set([0, 1, 2, 3, 4].map((i) => Math.round((niceMax / 4) * i)))]
  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    const x = ((e.clientX - rect.left) / rect.width) * W
    const frac = (x - PAD_L) / (W - PAD_L - 4)
    setHover(Math.max(0, Math.min(N - 1, Math.round(frac * (N - 1)))))
  }
  const hov = hover !== null ? { i: hover, t: bucketT(hover), r: req[hover], k: kb[hover], x: upPts[hover].x } : null
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="grid grid-cols-2 gap-x-2 gap-y-2.5 rounded-lg bg-muted/40 px-3 py-2.5 text-center">
        <div className="leading-tight">
          <div className="flex items-center justify-center gap-1.5 text-xs text-muted-foreground">
            <span className="size-1.5 shrink-0 rounded-full" style={{ background: UP_COLOR }} /> Upstream
          </div>
          <div className="mt-0.5 whitespace-nowrap text-sm font-semibold tabular-nums">{req[N - 1] === 0 && totalReq === 0 ? '—' : `${req[N - 1]} req`}</div>
        </div>
        <div className="leading-tight">
          <div className="flex items-center justify-center gap-1.5 text-xs text-muted-foreground">
            <span className="size-1.5 shrink-0 rounded-full" style={{ background: DOWN_COLOR }} /> Downstream
          </div>
          <div className="mt-0.5 whitespace-nowrap text-sm font-semibold tabular-nums">{kb[N - 1].toFixed(2)} KB</div>
        </div>
        <div className="leading-tight">
          <div className="text-xs text-muted-foreground">Total req</div>
          <div className="mt-0.5 whitespace-nowrap text-sm font-semibold tabular-nums">{totalReq}</div>
        </div>
        <div className="leading-tight">
          <div className="text-xs text-muted-foreground">Total served</div>
          <div className="mt-0.5 whitespace-nowrap text-sm font-semibold tabular-nums">{totalKb >= 1024 ? `${(totalKb / 1024).toFixed(2)} MB` : `${totalKb.toFixed(2)} KB`}</div>
        </div>
      </div>
      <div className="relative" onMouseLeave={() => setHover(null)}>
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className={`w-full ${tall ? 'h-44' : 'h-28'}`}
          preserveAspectRatio="none"
          onMouseMove={onMove}
        >
          <defs>
            <linearGradient id="trafUp" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={UP_COLOR} stopOpacity="0.45" />
              <stop offset="100%" stopColor={UP_COLOR} stopOpacity="0.02" />
            </linearGradient>
            <linearGradient id="trafDown" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={DOWN_COLOR} stopOpacity="0.45" />
              <stop offset="100%" stopColor={DOWN_COLOR} stopOpacity="0.02" />
            </linearGradient>
          </defs>
          {ticks.map((t) => {
            const y = 6 + (1 - t / niceMax) * (H - 26)
            return (
              <g key={t}>
                <line x1={PAD_L} x2={W - 4} y1={y} y2={y} stroke="var(--border)" strokeWidth="0.5" strokeDasharray="3 3" opacity="0.8" />
                <text x="2" y={y + 3} fontSize="7" fill="var(--muted-foreground)">
                  {t}
                </text>
              </g>
            )
          })}
          <path d={area(downLine)} fill="url(#trafDown)" />
          <path d={area(upLine)} fill="url(#trafUp)" />
          <path d={downLine} fill="none" stroke={DOWN_COLOR} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
          <path d={upLine} fill="none" stroke={UP_COLOR} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
          {hov !== null && (
            <g>
              <line x1={hov.x} x2={hov.x} y1="6" y2={H - 18} stroke="var(--muted-foreground)" strokeWidth="0.75" strokeDasharray="2 2" opacity="0.9" />
              <circle cx={upPts[hov.i].x} cy={upPts[hov.i].y} r="2.4" fill={UP_COLOR} stroke="var(--card)" strokeWidth="1" />
              <circle cx={downPts[hov.i].x} cy={downPts[hov.i].y} r="2.4" fill={DOWN_COLOR} stroke="var(--card)" strokeWidth="1" />
            </g>
          )}
        </svg>
        {hov !== null && (
          <div
            className="pointer-events-none absolute z-10 -translate-x-1/2 rounded-md border border-border bg-card px-2.5 py-1.5 text-xs shadow-lg"
            style={{ left: `${Math.min(85, Math.max(15, (hov.x / W) * 100))}%`, top: '4px' }}
          >
            <div className="font-medium tabular-nums">{fmtT(hov.t)}</div>
            <div className="flex items-center gap-1.5 tabular-nums">
              <span className="size-1.5 rounded-full" style={{ background: UP_COLOR }} /> {hov.r} req
            </div>
            <div className="flex items-center gap-1.5 tabular-nums">
              <span className="size-1.5 rounded-full" style={{ background: DOWN_COLOR }} /> {hov.k.toFixed(2)} KB
            </div>
          </div>
        )}
      </div>
      <div className="flex justify-between text-[11px] tabular-nums text-muted-foreground">
        <span>{fmtT(end - WIN_MS)}</span>
        <span>{fmtT(end)}</span>
      </div>
    </div>
  )
})

function HealthRow({ item }: { item: HealthItem }) {
  const Icon = item.status === 'ok' ? CheckCircle2 : item.status === 'warn' ? AlertTriangle : XCircle
  const color = item.status === 'ok' ? 'text-success' : item.status === 'warn' ? 'text-warning' : 'text-destructive'
  return (
    <div className="flex items-start gap-2 text-sm">
      <Icon className={`mt-0.5 size-4 shrink-0 ${color}`} />
      <div>
        <span className="font-medium">{item.label}</span>
        <span className="text-muted-foreground"> · {item.detail}</span>
        {item.fix && item.status !== 'ok' && <div className="text-xs text-muted-foreground">{item.fix}</div>}
      </div>
    </div>
  )
}
