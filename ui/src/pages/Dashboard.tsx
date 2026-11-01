import { Activity, ChevronDown, FileText, Home, Play, Repeat, Rocket, RotateCw } from 'lucide-react'
import { memo, useEffect, useMemo, useState } from 'react'

import { DiagnosticsCard } from '@/components/DiagnosticsCard'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { ServiceMark } from '@/components/TechIcon'
import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DashboardData, type Diagnostic, type ServerAvailability, type ServiceStatus, type SystemStats, runCommand } from '@/core'
import { formatBytes, useAction, usePoll } from '@/lib/hooks'
import { createPersistentCache } from '@/lib/cache'
import { cn } from '@/lib/utils'
import { waitForService } from '@/lib/wait'
import { confirmAction } from '@/lib/confirm'
import { groupServices } from '@/lib/serviceGrouping'

const dashboardCache = createPersistentCache<DashboardData | null>('dashboard_data', null)
const dashboardStatsCache = createPersistentCache<SystemStats | null>('dashboard_stats', null)
const accessLinesCache = createPersistentCache<string[]>('dashboard_access_lines', [])

/**
 * Summary dashboard page: what's running, what's wrong, and one-click ways to act on it.
 * Uses CSS container queries driven by the page content width.
 */
export function DashboardPage({ onNavigate, onOpenLogs }: { onNavigate: (p: Page) => void; onOpenLogs: (source: string) => void }) {
  const [data, setData] = useState<DashboardData | null>(dashboardCache.get)
  const [dataError, setDataError] = useState<Diagnostic | null>(null)
  const [diagToken, setDiagToken] = useState(0)
  const [stats, setStats] = useState<SystemStats | null>(dashboardStatsCache.get)
  const [accessLines, setAccessLines] = useState<string[]>(accessLinesCache.get)
  const action = useAction()
  const { busy, error, setError, run } = action

  usePoll(async () => {
    try {
      const res = await runCommand({ type: 'get_dashboard' })
      if (res.type === 'dashboard') {
        setData(res.data)
        dashboardCache.set(res.data)
        setDataError(null)
      }
    } catch (e) {
      setDataError(asDiagnostic(e))
    }
  }, 3000)

  // Machine load next to sites, not buried on Processes page.
  usePoll(async () => {
    const r = await runCommand({ type: 'get_system_stats' }).catch(() => null)
    if (r?.type === 'system_stats') {
      setStats(r.stats)
      dashboardStatsCache.set(r.stats)
    }
  }, 3000)

  // Traffic from web access log
  usePoll(async () => {
    const r = await runCommand({ type: 'read_log', source: 'web:access', max_lines: 400 }).catch(() => null)
    if (r?.type === 'log_lines') {
      setAccessLines(r.lines)
      accessLinesCache.set(r.lines)
    }
  }, 5000)

  const refresh = async () => {
    const res = await runCommand({ type: 'get_dashboard' })
    if (res.type === 'dashboard') {
      setData(res.data)
      dashboardCache.set(res.data)
    }
  }

  const web = data?.web
  const runningServices = data?.services.filter((s) => s.running) ?? []
  const stoppable = (data?.services ?? []).filter((s) => s.installed && !s.running)
  const hasRunning = runningServices.length > 0 || web?.running === true

  return (
    <div className="@container/dashboard flex flex-col gap-3.5 w-full min-w-0">
      {/* Header: title and actions wrap cleanly without clipping */}
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="secondary"
            size="sm"
            className="h-8 rounded-md px-3 text-[13px] font-medium [&_svg]:size-3.5 shrink-0"
            onClick={() => onNavigate('quickapps')}
          >
            <Rocket /> New from Quick App
          </Button>
          <Button
            variant={hasRunning ? 'secondary' : 'default'}
            size="sm"
            className={cn(
              'h-8 min-w-28 rounded-md px-3 text-[13px] font-medium [&_svg]:size-3.5 shrink-0 transition-colors',
              hasRunning && 'border border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive dark:bg-destructive/[0.18] dark:text-red-300 dark:hover:bg-destructive/25'
            )}
            disabled={busy !== null || (!hasRunning && stoppable.length === 0)}
            title={
              hasRunning
                ? 'Stop every running service, worker, tunnel and the web stack'
                : stoppable.length > 0 || (web && !web.running)
                  ? `Start ${stoppable.length + (web?.running ? 0 : 1)} stopped service${stoppable.length === 0 ? '' : 's'}`
                  : 'Every installed service is already running'
            }
            onClick={() =>
              hasRunning
                ? void (async () => {
                  const ok = await confirmAction('Stop all running services, workers, tunnels, and the web stack?')
                  if (!ok) return
                  await run('stop-all', async () => {
                    const res = await runCommand({ type: 'stop_all' })
                    if (res.type === 'stopped_everything' && res.still_running.length > 0) {
                      throw {
                        problem: 'Something did not stop.',
                        cause: `Still running after Stop all: ${res.still_running.join(', ')}`,
                        fix: 'Close it from Task Manager and press Stop all again.',
                      }
                    }
                    await refresh()
                    setDiagToken((t) => t + 1)
                  })
                })()
                : void run('start-all', async () => {
                  const current = await runCommand({ type: 'get_dashboard' })
                  if (current.type === 'dashboard') setData(current.data)
                  const targets = (current.type === 'dashboard' ? current.data.services : []).filter(
                    (s) => s.installed && !s.running,
                  )
                  const promises: Promise<unknown>[] = targets.map((service) =>
                    runCommand({ type: 'start_service', id: service.id }),
                  )
                  if (current.type === 'dashboard' && !current.data.web?.running) {
                    promises.push(runCommand({ type: 'apply_web', overwrite: [] }))
                  }
                  await Promise.all(promises)
                  await Promise.all(targets.map((service) => waitForService(service.id, 'running')))
                  await refresh()
                  setDiagToken((t) => t + 1)
                })
            }
          >
            {busy === 'stop-all' || busy === 'start-all' ? (
              <Spinner />
            ) : hasRunning ? (
              <StopIcon />
            ) : (
              <Play />
            )}{' '}
            {busy === 'stop-all' ? 'Stopping all…' : busy === 'start-all' ? 'Starting all…' : hasRunning ? 'Stop all' : 'Start all'}
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {dataError && <ErrorCard error={dataError} onDismiss={() => setDataError(null)} />}

      {/* Diagnostics: compact pill when clean, expanded card when issues found */}
      <DiagnosticsCard
        refreshToken={diagToken}
        onNavigate={onNavigate}
        healthProblems={data?.health ?? []}
      />

      {/*
        Responsive content-driven layout:
        - Wide (>=1100px): 2 columns. Left: Resources, Web server, Traffic. Right: Services (full height).
        - Narrow (<1100px): single column in order: Resources, Web server, Services, Traffic.
      */}
      <div className="grid grid-cols-1 @[1100px]/dashboard:grid-cols-2 gap-3.5 items-start">
        {/* 1. Resources card */}
        <Card className="order-1 @[1100px]/dashboard:col-start-1 @[1100px]/dashboard:row-start-1 shadow-xs">
          <CardHeader className="flex-row items-center justify-between space-y-0 px-3.5 py-2.5">
            <div>
              <CardTitle className="text-sm font-semibold">Overview & Resources</CardTitle>
              <CardDescription className="text-xs text-muted-foreground mt-0.5">
                {data ? (
                  <>
                    <button
                      type="button"
                      onClick={() => onNavigate('sites')}
                      className="cursor-pointer font-medium text-foreground/80 hover:text-foreground hover:underline underline-offset-2 transition-colors"
                    >
                      {data.domains.length} sites
                    </button>
                    {' · '}
                    <button
                      type="button"
                      onClick={() => onNavigate('sites')}
                      className="cursor-pointer font-medium text-foreground/80 hover:text-foreground hover:underline underline-offset-2 transition-colors"
                    >
                      {data.project_count} projects
                    </button>
                  </>
                ) : (
                  'Loading…'
                )}
              </CardDescription>
            </div>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
              onClick={() => onNavigate('processes')}
              title="Open the Processes page to view system monitor and process details"
            >
              View processes
            </Button>
          </CardHeader>
          <CardContent className="px-3.5 pb-3 pt-0">
            {/* Flattened items: 3 compact columns in 1 row separated by clean dividers */}
            <div className="grid grid-cols-3 divide-x divide-border/50">
              <CompactResource
                label="CPU"
                percent={stats?.cpu_percent ?? 0}
                detail={stats ? `${stats.cpu_cores} cores` : '…'}
                loading={!stats}
              />
              <CompactResource
                label="RAM"
                percent={stats?.memory_used && stats?.memory_total ? (stats.memory_used / stats.memory_total) * 100 : 0}
                detail={stats ? `${formatBytes(stats.memory_used)} / ${formatBytes(stats.memory_total)}` : '…'}
                loading={!stats}
              />
              {(stats?.disks ?? []).slice(0, 1).map((d) => (
                <CompactResource
                  key={d.mount}
                  label={`Disk ${d.mount.replace(/\\$/, '')}`}
                  percent={d.total > 0 ? (d.used / d.total) * 100 : 0}
                  detail={stats ? `${formatBytes(d.total - d.used)} free` : '…'}
                  loading={!stats}
                />
              ))}
            </div>
          </CardContent>
        </Card>

        {/* 2. Web server card */}
        <Card className="order-2 @[1100px]/dashboard:col-start-1 @[1100px]/dashboard:row-start-2 shadow-xs">
          <CardHeader className="flex-row items-center justify-between space-y-0 px-3.5 py-2.5">
            <CardTitle className="text-sm font-semibold">Web server</CardTitle>
            {web?.running ? (
              <Badge variant="success" className="shrink-0 whitespace-nowrap text-xs">
                ● Running
              </Badge>
            ) : (
              <Badge variant="secondary" className="shrink-0 whitespace-nowrap text-xs">
                Stopped
              </Badge>
            )}
          </CardHeader>
          <CardContent className="flex flex-col gap-1.5 text-xs text-muted-foreground px-3.5 pb-3 pt-0">
            <div className="font-medium text-foreground/90">
              {web?.servers.find((s) => s.active)?.name ?? '…'} · ports {web?.http_port ?? '—'} / {web?.https_port ?? '—'}
            </div>
            {web?.php_pools && web.php_pools.length > 0 && (
              <div>
                {web.php_pools.map((p) => `PHP ${p.version} (${p.ports.length} worker${p.ports.length === 1 ? '' : 's'}${p.running ? '' : ', stopped'})`).join(' · ')}
              </div>
            )}
            {web?.dns_running && <div>Wildcard DNS on port {web.dns_port}</div>}
            <div className="pt-1.5">
              <Button
                size="sm"
                variant="secondary"
                className="h-7 text-xs font-medium gap-1.5"
                disabled={!web?.running}
                title={web?.running ? 'Open the built-in welcome site' : 'Start the web server first'}
                onClick={() => runCommand({ type: 'open_url', url: 'https://openlocalserver.test' }).catch(() => undefined)}
              >
                <Home className="size-3.5" /> Open home page
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* 3. Services card: full height alongside Left column when wide, above Traffic when narrow */}
        <div className="order-3 @[1100px]/dashboard:col-start-2 @[1100px]/dashboard:row-start-1 @[1100px]/dashboard:row-span-3 flex min-h-0 min-w-0 flex-1 flex-col">
          <ServicesWidget
            services={data?.services ?? null}
            webServers={data?.web?.servers ?? null}
            busy={busy}
            run={run}
            onNavigate={onNavigate}
            onOpenLogs={onOpenLogs}
            onDone={async () => {
              await refresh()
              setDiagToken((t) => t + 1)
            }}
          />
        </div>

        {/* 4. Traffic card: bottom of Left column when wide, after Services when narrow */}
        <Card className="order-4 @[1100px]/dashboard:col-start-1 @[1100px]/dashboard:row-start-3 shadow-xs">
          <CardHeader className="flex-row items-center justify-between space-y-0 px-3.5 py-2.5">
            <CardTitle className="text-sm font-semibold">Traffic</CardTitle>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground"
              onClick={() => onNavigate('logs')}
            >
              Logs
            </Button>
          </CardHeader>
          <CardContent className="flex flex-col gap-2.5 px-3.5 pb-3 pt-0">
            <TrafficGraph lines={accessLines} />
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

/** Port column tooltip: web server HTTP port or background service port. */
function servicePortTitle(s: ServiceStatus, sites?: number): string {
  if (s.kind !== 'web') return `Port ${s.port}`
  if (sites === 0) return `HTTP port ${s.port} — no sites assigned to ${s.name} yet`
  return `HTTP port ${s.port} — ${sites} site${sites === 1 ? '' : 's'} on ${s.name}`
}

const COLLAPSED_STORAGE_KEY = 'ols:dashboard:services:collapsed'
const SETTING_KEY = 'dashboard.services.collapsed_groups'

function getInitialCollapsed(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(COLLAPSED_STORAGE_KEY)
    if (raw) return JSON.parse(raw) as Record<string, boolean>
  } catch {
    // ignore
  }
  return {}
}

type WidgetAction = 'start' | 'stop' | 'restart' | 'reload'

/**
 * Standard fixed action button for each service row slot.
 * Ensures identical dimensions (28px x 28px), fixed slots, and clean visual affordance.
 */
function RowActionButton({
  title,
  disabled,
  spinning,
  onClick,
  children,
  variant = 'neutral',
}: {
  title: string
  disabled: boolean
  spinning?: boolean
  onClick: (e: React.MouseEvent) => void
  children: React.ReactNode
  variant?: 'start' | 'stop' | 'neutral'
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'size-7 min-w-7 min-h-7 shrink-0 rounded-md border flex items-center justify-center transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
        disabled
          ? 'opacity-30 cursor-not-allowed border-border/20 bg-muted/10 text-muted-foreground pointer-events-none'
          : variant === 'start'
            ? 'cursor-pointer border-success/30 bg-success/5 text-success hover:bg-success/15 hover:border-success/50'
            : variant === 'stop'
              ? 'cursor-pointer border-destructive/30 bg-destructive/5 text-destructive hover:bg-destructive/15 hover:border-destructive/50'
              : 'cursor-pointer border-border/40 bg-muted/20 text-muted-foreground hover:bg-muted/60 hover:text-foreground hover:border-border/80'
      )}
    >
      {spinning ? <Spinner className="size-3.5" /> : children}
    </button>
  )
}

/**
 * Services card: services grouped by type with collapsible section headers and fixed 4-slot rows.
 */
function ServicesWidget({
  services,
  webServers,
  busy,
  run,
  onNavigate,
  onOpenLogs,
  onDone,
}: {
  services: ServiceStatus[] | null
  webServers: ServerAvailability[] | null
  busy: string | null
  run: <T, >(key: string, fn: () => Promise<T>) => Promise<T | undefined>
  onNavigate: (p: Page) => void
  onOpenLogs: (source: string) => void
  onDone: () => Promise<void>
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(getInitialCollapsed)

  useEffect(() => {
    void runCommand({ type: 'get_setting', key: SETTING_KEY })
      .then((res) => {
        if (res.type === 'setting' && res.value && typeof res.value === 'object') {
          const remote = res.value as Record<string, boolean>
          setCollapsed((prev) => ({ ...remote, ...prev }))
        }
      })
      .catch(() => undefined)
  }, [])

  function toggleGroup(groupId: string) {
    setCollapsed((prev) => {
      const next = { ...prev, [groupId]: !prev[groupId] }
      try {
        localStorage.setItem(COLLAPSED_STORAGE_KEY, JSON.stringify(next))
      } catch {
        // ignore
      }
      void runCommand({ type: 'set_setting', key: SETTING_KEY, value: next }).catch(() => undefined)
      return next
    })
  }

  const groups = groupServices(services)
  const allServices = groups.flatMap((g) => g.services)
  const running = allServices.filter((s) => s.running).length
  const installed = allServices.filter((s) => s.installed).length
  const webSites = new Map((webServers ?? []).map((s) => [s.id, s.sites]))

  async function act(s: ServiceStatus, action: WidgetAction) {
    if (action === 'stop' && !(await confirmAction(`Stop ${s.name}? Anything connected to it will be disconnected.`))) return
    if (action === 'restart' && s.running && !(await confirmAction(`Restart ${s.name}? Anything connected to it will be disconnected.`))) return
    void run(`svc:${s.id}:${action}`, async () => {
      if (action === 'reload') {
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

  async function actGroup(group: (typeof groups)[number], action: 'start' | 'stop') {
    if (action === 'stop') {
      if (!(await confirmAction(`Stop all running services in ${group.label}? Anything connected to them will be disconnected.`))) return
      const targets = group.services.filter((s) => s.running)
      void run(`group:${group.id}:stop`, async () => {
        await Promise.all(targets.map((s) => runCommand({ type: 'stop_service', id: s.id })))
        await Promise.all(targets.map((s) => waitForService(s.id, 'stopped')))
        await onDone()
      })
    } else {
      const targets = group.services.filter((s) => s.installed && !s.running)
      void run(`group:${group.id}:start`, async () => {
        await Promise.all(targets.map((s) => runCommand({ type: 'start_service', id: s.id })))
        await Promise.all(targets.map((s) => waitForService(s.id, 'running')))
        await onDone()
      })
    }
  }

  return (
    <Card className="flex w-full min-w-0 flex-1 flex-col overflow-hidden py-0 shadow-xs">
      <CardHeader className="flex-row items-center justify-between space-y-0 px-3 py-2.5">
        <div className="flex min-w-0 items-baseline gap-2">
          <CardTitle className="text-sm font-semibold">Services</CardTitle>
          <CardDescription className="truncate text-xs tabular-nums">
            {services === null ? 'Loading…' : `${running} running · ${installed} installed`}
          </CardDescription>
        </div>
        <Button size="sm" variant="ghost" className="h-7 shrink-0 rounded-md px-2 text-xs text-muted-foreground hover:text-foreground" onClick={() => onNavigate('services')}>
          Manage
        </Button>
      </CardHeader>
      <CardContent className="p-0">
        {groups.map((group, groupIndex) => {
          const runningInGroup = group.services.filter((s) => s.running).length
          const stoppedInGroup = group.services.filter((s) => s.installed && !s.running).length

          // Default: collapsed when all services are stopped (e.g. Cache & Queues 0/2, Mail 0/1)
          const isCollapsed = collapsed[group.id] !== undefined
            ? !!collapsed[group.id]
            : runningInGroup === 0

          const groupBusy = busy === `group:${group.id}:start` || busy === `group:${group.id}:stop`

          const groupToggleTooltip = runningInGroup > 0
            ? `Stop ${runningInGroup} running`
            : `Start ${stoppedInGroup} stopped`

          return (
            <div key={group.id} className="min-w-0">
              <div
                className={cn(
                  'flex w-full min-w-0 h-8 items-center justify-between gap-2 px-3 transition-colors hover:bg-muted/30 select-none',
                  groupIndex > 0 && 'border-t border-border/60'
                )}
              >
                <button
                  type="button"
                  onClick={() => toggleGroup(group.id)}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left cursor-pointer"
                  aria-expanded={!isCollapsed}
                >
                  <ChevronDown
                    className={cn(
                      'size-3.5 shrink-0 text-muted-foreground transition-transform duration-200',
                      isCollapsed && '-rotate-90'
                    )}
                  />
                  <span className="min-w-0 truncate text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/90 dark:text-muted-foreground">
                    {group.label}
                  </span>
                  {/* Clean n/m count without duplicate status dot */}
                  <span className="tabular-nums text-xs text-muted-foreground/80">
                    {runningInGroup}/{group.services.length}
                  </span>
                </button>

                {/* Group toggle aligned in Slot 1 column */}
                <div className="flex shrink-0 items-center gap-1 w-[7.5rem] justify-end">
                  <RowActionButton
                    title={groupToggleTooltip}
                    disabled={groupBusy || group.services.every((s) => !s.installed)}
                    spinning={groupBusy}
                    variant={runningInGroup > 0 ? 'stop' : 'start'}
                    onClick={(e) => {
                      e.stopPropagation()
                      void actGroup(group, runningInGroup > 0 ? 'stop' : 'start')
                    }}
                  >
                    {runningInGroup > 0 ? <StopIcon className="size-3.5" /> : <Play className="size-3.5" />}
                  </RowActionButton>
                  {/* Spacers for slots 2, 3, 4 preserve exact column alignment with rows */}
                  <span className="size-7 min-w-7 shrink-0" aria-hidden="true" />
                  <span className="size-7 min-w-7 shrink-0" aria-hidden="true" />
                  <span className="size-7 min-w-7 shrink-0" aria-hidden="true" />
                </div>
              </div>

              {!isCollapsed && (
                <div>
                  {group.services.map((s) => {
                    const working =
                      busy === `svc:${s.id}:start` ||
                      busy === `svc:${s.id}:stop` ||
                      busy === `svc:${s.id}:restart` ||
                      busy === `svc:${s.id}:reload`
                    const state = !s.installed
                      ? 'missing'
                      : s.running
                        ? s.healthy === false
                          ? 'unhealthy'
                          : 'running'
                        : 'stopped'

                    return (
                      <div
                        key={s.id}
                        className="flex items-center justify-between gap-2 px-3 h-8.5 transition-colors hover:bg-muted/25 border-t border-border/60 min-w-0"
                      >
                        {/* Service mark with single status badge, name, and port (port hides on narrow) */}
                        <div className="flex items-center gap-2.5 min-w-0 flex-1">
                          <span className="shrink-0 flex items-center justify-center" title={`${s.name} (${state})`}>
                            <ServiceMark id={s.id} state={state} className="size-4 [&>svg]:size-3" />
                          </span>
                          <span className="min-w-0 truncate text-[13px] font-medium text-foreground" title={`${s.name} (${state})`}>
                            {s.name}
                          </span>
                          <span
                            className="hidden @[480px]/dashboard:inline-block font-mono text-xs tabular-nums text-muted-foreground shrink-0 ml-auto mr-1"
                            title={servicePortTitle(s, webSites.get(s.id))}
                          >
                            {s.port === null ? '—' : s.port}
                          </span>
                        </div>

                        {/* Fixed 4 action slots in exact order */}
                        <div className="flex shrink-0 items-center gap-1 w-[7.5rem] justify-end">
                          {/* Slot 1: Start/Stop toggle button */}
                          <RowActionButton
                            title={s.running ? `Stop ${s.name}` : `Start ${s.name}`}
                            disabled={busy !== null || !s.installed}
                            spinning={working && (busy === `svc:${s.id}:start` || busy === `svc:${s.id}:stop`)}
                            variant={s.running ? 'stop' : 'start'}
                            onClick={() => void act(s, s.running ? 'stop' : 'start')}
                          >
                            {s.running ? <StopIcon className="size-3.5" /> : <Play className="size-3.5" />}
                          </RowActionButton>

                          {/* Slot 2: Restart */}
                          <RowActionButton
                            title="Restart"
                            disabled={busy !== null || !s.installed || !s.running}
                            spinning={working && busy === `svc:${s.id}:restart`}
                            variant="neutral"
                            onClick={() => void act(s, 'restart')}
                          >
                            <RotateCw className="size-3.5" />
                          </RowActionButton>

                          {/* Slot 3: Reload config */}
                          <RowActionButton
                            title="Reload config"
                            disabled={busy !== null || !s.installed || !s.running || s.kind !== 'web'}
                            spinning={working && busy === `svc:${s.id}:reload`}
                            variant="neutral"
                            onClick={() => void act(s, 'reload')}
                          >
                            <Repeat className="size-3.5" />
                          </RowActionButton>

                          {/* Slot 4: View logs */}
                          <RowActionButton
                            title="View logs"
                            disabled={busy !== null || !s.installed}
                            variant="neutral"
                            onClick={() => onOpenLogs(s.log_source || `service:${s.id}`)}
                          >
                            <FileText className="size-3.5" />
                          </RowActionButton>
                        </div>
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </CardContent>
    </Card>
  )
}

/** Semantic color thresholds: normal (<75%), warning (75-89%), critical (>=90%) */
function getGaugeColor(percent: number): string {
  if (percent >= 90) return 'var(--destructive)'
  if (percent >= 75) return 'var(--warning)'
  return 'var(--success)'
}

/**
 * Compact resource gauge with no nested bordered box.
 * Adapts size smoothly when space is tight.
 */
function CompactResource({
  label,
  percent,
  detail,
  loading,
}: {
  label: string
  percent: number
  detail: string
  loading?: boolean
}) {
  const r = 16
  const c = 2 * Math.PI * r
  const p = Math.min(100, Math.max(0, percent))
  const color = getGaugeColor(p)

  return (
    <div className="flex flex-col sm:flex-row items-center justify-center gap-2 px-2 text-center sm:text-left min-w-0">
      <div className="relative size-10 @[500px]/dashboard:size-11 shrink-0">
        <svg viewBox="0 0 40 40" className="size-full -rotate-90">
          <circle cx="20" cy="20" r={r} fill="none" stroke="var(--muted)" strokeWidth="4" />
          <circle
            cx="20"
            cy="20"
            r={r}
            fill="none"
            stroke={color}
            strokeWidth="4"
            strokeLinecap="round"
            strokeDasharray={c}
            strokeDashoffset={c * (1 - p / 100)}
            className="transition-[stroke-dashoffset,stroke] duration-500"
          />
        </svg>
        <span
          className="absolute inset-0 flex items-center justify-center text-[11px] font-semibold tabular-nums"
          style={{ color }}
        >
          {loading ? '…' : `${p.toFixed(0)}%`}
        </span>
      </div>
      <div className="flex flex-col min-w-0 leading-tight">
        <span className="text-xs font-medium text-foreground">{label}</span>
        <span className="text-[11px] text-muted-foreground/80 tabular-nums truncate max-w-full" title={detail}>
          {detail}
        </span>
      </div>
    </div>
  )
}

const MONTHS: Record<string, number> = {
  Jan: 0, Feb: 1, Mar: 2, Apr: 3, May: 4, Jun: 5,
  Jul: 6, Aug: 7, Sep: 8, Oct: 9, Nov: 10, Dec: 11,
}

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

const REQ_COLOR = 'var(--success)'
const DATA_COLOR = 'var(--warning)'

/** Monotone cubic spline (Fritsch-Carlson) interpolation clamped at the baseline */
function monotonePath(pts: { x: number; y: number }[], baseline: number): string {
  const n = pts.length
  if (n === 0) return ''
  if (n === 1) return `M ${pts[0].x},${pts[0].y}`

  const m = new Array(n).fill(0)
  const deltas: number[] = []
  for (let i = 0; i < n - 1; i++) {
    const dx = pts[i + 1].x - pts[i].x
    deltas.push(dx !== 0 ? (pts[i + 1].y - pts[i].y) / dx : 0)
  }

  m[0] = deltas[0]
  m[n - 1] = deltas[n - 2]
  for (let i = 1; i < n - 1; i++) {
    if (deltas[i - 1] * deltas[i] <= 0) {
      m[i] = 0
    } else {
      m[i] = (deltas[i - 1] + deltas[i]) / 2
    }
  }

  for (let i = 0; i < n - 1; i++) {
    if (deltas[i] === 0) {
      m[i] = 0
      m[i + 1] = 0
    } else {
      const alpha = m[i] / deltas[i]
      const beta = m[i + 1] / deltas[i]
      const dist = alpha * alpha + beta * beta
      if (dist > 9) {
        const tau = 3 / Math.sqrt(dist)
        m[i] = tau * alpha * deltas[i]
        m[i + 1] = tau * beta * deltas[i]
      }
    }
  }

  let path = `M ${pts[0].x.toFixed(2)},${pts[0].y.toFixed(2)}`
  for (let i = 0; i < n - 1; i++) {
    const dx = pts[i + 1].x - pts[i].x
    const p1x = pts[i].x + dx / 3
    const p1y = Math.min(baseline, pts[i].y + (m[i] * dx) / 3)
    const p2x = pts[i + 1].x - dx / 3
    const p2y = Math.min(baseline, pts[i + 1].y - (m[i + 1] * dx) / 3)
    path += ` C ${p1x.toFixed(2)},${p1y.toFixed(2)} ${p2x.toFixed(2)},${p2y.toFixed(2)} ${pts[i + 1].x.toFixed(2)},${pts[i + 1].y.toFixed(2)}`
  }
  return path
}

const TrafficGraph = memo(function TrafficGraph({ lines }: { lines: string[] }) {
  const { req, kb, end, totalLogReq, totalLogKb } = useMemo(() => {
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
    let allBytes = 0

    for (const p of parsed) {
      allBytes += p.bytes
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
    return {
      req: r,
      kb: k,
      end: endMs,
      totalLogReq: parsed.length,
      totalLogKb: allBytes / 1024,
    }
  }, [lines])

  const N = req.length
  const WIN_MS = 5 * 60 * 1000
  const [hover, setHover] = useState<number | null>(null)

  // Nice round Y-axis maximum (multiples of 5, minimum 10)
  const maxV = Math.max(10, ...req, ...kb)
  const niceMax = Math.ceil(maxV / 5) * 5

  const W = 300
  const H = 85
  const PAD_L = 20
  const BASELINE = H - 16

  const pts = (series: number[]) =>
    series.map((v, i) => ({
      x: PAD_L + (i / (N - 1)) * (W - PAD_L - 4),
      y: Math.min(BASELINE, 6 + (1 - v / niceMax) * (BASELINE - 6)),
    }))

  const upPts = pts(req)
  const downPts = pts(kb)
  const upLine = monotonePath(upPts, BASELINE)
  const downLine = monotonePath(downPts, BASELINE)
  const area = (line: string) => `${line} L ${W - 4},${BASELINE} L ${PAD_L},${BASELINE} Z`

  const windowReq = req.reduce((a, b) => a + b, 0)
  const windowKb = kb.reduce((a, b) => a + b, 0)

  const fmtT = (ms: number) => new Date(ms).toTimeString().slice(0, 8)
  const bucketT = (i: number) => end - ((N - 1 - i) / N) * WIN_MS

  // Max 3 round ticks: 0, niceMax/2, niceMax
  const ticks = [0, Math.round(niceMax / 2), niceMax]

  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    const x = ((e.clientX - rect.left) / rect.width) * W
    const frac = (x - PAD_L) / (W - PAD_L - 4)
    setHover(Math.max(0, Math.min(N - 1, Math.round(frac * (N - 1)))))
  }

  const hov = hover !== null ? { i: hover, t: bucketT(hover), r: req[hover], k: kb[hover], x: upPts[hover].x } : null

  return (
    <div className="flex min-w-0 flex-col gap-2">
      {/* Flattened inline metrics: one compact row with clean divider */}
      <div className="flex items-center justify-between rounded-md bg-muted/30 px-3 py-1.5 text-xs">
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="size-1.5 rounded-full shrink-0" style={{ background: REQ_COLOR }} />
          <span className="text-muted-foreground">Requests:</span>
          <span className="font-semibold tabular-nums text-foreground">{windowReq}</span>
          <span className="text-[11px] text-muted-foreground/80 tabular-nums">({totalLogReq} total)</span>
        </div>
        <div className="h-3 w-px bg-border/60 mx-2 shrink-0" />
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="size-1.5 rounded-full shrink-0" style={{ background: DATA_COLOR }} />
          <span className="text-muted-foreground">Served:</span>
          <span className="font-semibold tabular-nums text-foreground truncate">
            {windowKb >= 1024 ? `${(windowKb / 1024).toFixed(1)} MB` : `${windowKb.toFixed(0)} KB`}
          </span>
          <span className="text-[11px] text-muted-foreground/80 tabular-nums shrink-0">
            ({totalLogKb >= 1024 ? `${(totalLogKb / 1024).toFixed(1)} MB` : `${totalLogKb.toFixed(0)} KB`})
          </span>
        </div>
      </div>

      {/* Slim empty state when traffic is basically zero */}
      {totalLogReq === 0 && windowReq === 0 ? (
        <div className="flex h-14 items-center justify-center gap-2 rounded-md border border-dashed border-border/50 px-3 text-xs text-muted-foreground/80">
          <Activity className="size-3.5 opacity-50 shrink-0" />
          <span>No traffic recorded in the last 5 minutes</span>
        </div>
      ) : (
        <div className="relative" onMouseLeave={() => setHover(null)}>
          <svg
            viewBox={`0 0 ${W} ${H}`}
            className="w-full h-24"
            preserveAspectRatio="none"
            onMouseMove={onMove}
          >
            <defs>
              <linearGradient id="trafUp" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={REQ_COLOR} stopOpacity="0.45" />
                <stop offset="100%" stopColor={REQ_COLOR} stopOpacity="0.02" />
              </linearGradient>
              <linearGradient id="trafDown" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={DATA_COLOR} stopOpacity="0.45" />
                <stop offset="100%" stopColor={DATA_COLOR} stopOpacity="0.02" />
              </linearGradient>
            </defs>
            {ticks.map((t) => {
              const y = 6 + (1 - t / niceMax) * (BASELINE - 6)
              return (
                <g key={t}>
                  <line x1={PAD_L} x2={W - 4} y1={y} y2={y} stroke="var(--border)" strokeWidth="0.5" strokeDasharray="3 3" opacity="0.8" />
                  <text x="2" y={y + 3} fontSize="7" fill="var(--muted-foreground)" className="hidden @[450px]/dashboard:inline">
                    {t}
                  </text>
                </g>
              )
            })}
            <path d={area(downLine)} fill="url(#trafDown)" />
            <path d={area(upLine)} fill="url(#trafUp)" />
            <path d={downLine} fill="none" stroke={DATA_COLOR} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
            <path d={upLine} fill="none" stroke={REQ_COLOR} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
            {hov !== null && (
              <g>
                <line x1={hov.x} x2={hov.x} y1="6" y2={BASELINE} stroke="var(--muted-foreground)" strokeWidth="0.75" strokeDasharray="2 2" opacity="0.9" />
                <circle cx={upPts[hov.i].x} cy={upPts[hov.i].y} r="2.4" fill={REQ_COLOR} stroke="var(--card)" strokeWidth="1" />
                <circle cx={downPts[hov.i].x} cy={downPts[hov.i].y} r="2.4" fill={DATA_COLOR} stroke="var(--card)" strokeWidth="1" />
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
                <span className="size-1.5 rounded-full" style={{ background: REQ_COLOR }} /> {hov.r} req
              </div>
              <div className="flex items-center gap-1.5 tabular-nums">
                <span className="size-1.5 rounded-full" style={{ background: DATA_COLOR }} /> {hov.k.toFixed(2)} KB
              </div>
            </div>
          )}
        </div>
      )}

      <div className="flex justify-between text-[11px] tabular-nums text-muted-foreground/80">
        <span>Last 5 min ({fmtT(end - WIN_MS)})</span>
        <span>Now ({fmtT(end)})</span>
      </div>
    </div>
  )
})
