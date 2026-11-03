import { ChevronDown, FileText, Globe, Home, Play, Repeat, Rocket, RotateCw } from 'lucide-react'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { DiagnosticsCard } from '@/components/DiagnosticsCard'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { ServiceMark } from '@/components/TechIcon'
import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  type DashboardData,
  type Diagnostic,
  type ServiceLifecycleState,
  type ServerAvailability,
  type ServiceStatus,
  type StartupSettings,
  type SystemStats,
  runCommand,
} from '@/core'
import { formatBytes, useAction, usePoll } from '@/lib/hooks'
import { createPersistentCache } from '@/lib/cache'
import {
  notifyManualStartFailure,
  runSerializedServiceOp,
} from '@/lib/diagnosticsNotify'
import { getServiceLifecycleState, useServiceStatuses } from '@/lib/serviceStatus'
import { notify, resolveNotificationsForService } from '@/lib/notify'
import { cn } from '@/lib/utils'
import { waitForService } from '@/lib/wait'
import { confirmAction } from '@/lib/confirm'
import { groupServices } from '@/lib/serviceGrouping'

const dashboardCache = createPersistentCache<DashboardData | null>('dashboard_data', null)
const dashboardStatsCache = createPersistentCache<SystemStats | null>('dashboard_stats', null)
const accessLinesCache = createPersistentCache<string[]>('dashboard_access_lines', [])

export type WidgetAction = 'start' | 'stop' | 'restart' | 'reload'

/**
 * Summary dashboard page: what's running, what's wrong, and one-click ways to act on it.
 * Responsive container layout driven by content area:
 * - Content >= 1500px: 3 columns (Resources+Web, Services max 540px, Traffic).
 * - Content 1100-1499px: 2 columns (Resources+Web+Traffic, Services max 540px).
 * - Content < 1100px: 1 column (Diagnostics, Resources, Services, Web server, Traffic).
 */
export function DashboardPage({
  onNavigate,
  onOpenLogs,
}: {
  onNavigate: (p: Page) => void
  onOpenLogs: (source: string) => void
}) {
  const [data, setData] = useState<DashboardData | null>(dashboardCache.get)
  const [dataError, setDataError] = useState<Diagnostic | null>(null)
  const [diagToken, setDiagToken] = useState(0)
  const [stats, setStats] = useState<SystemStats | null>(dashboardStatsCache.get)
  const [accessLines, setAccessLines] = useState<string[]>(accessLinesCache.get)
  const [startup, setStartup] = useState<StartupSettings | null>(null)
  const serviceStatuses = useServiceStatuses()

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

  // Machine load
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

  // Autostart settings: Stop all / Start all specifically targets autostart services
  usePoll(async () => {
    const r = await runCommand({ type: 'get_startup_settings' }).catch(() => null)
    if (r?.type === 'startup') {
      setStartup(r.settings)
    }
  }, 5000)

  const refresh = async () => {
    const res = await runCommand({ type: 'get_dashboard' })
    if (res.type === 'dashboard') {
      setData(res.data)
      dashboardCache.set(res.data)
      return res.data
    }
    return null
  }

  const onServiceAction = useCallback(
    async (s: ServiceStatus, action: WidgetAction) => {
      try {
        await runSerializedServiceOp(s.id, action, async () => {
          if (action === 'reload') {
            await runCommand({ type: 'apply_web', overwrite: [] })
          } else if (action === 'restart') {
            await runCommand({ type: 'restart_service', id: s.id })
            await waitForService(s.id, 'running')
          } else {
            await runCommand({ type: action === 'start' ? 'start_service' : 'stop_service', id: s.id })
            await waitForService(s.id, action === 'start' ? 'running' : 'stopped')
          }
        })
        await refresh()
        setDiagToken((t) => t + 1)
      } catch (e) {
        const diag = asDiagnostic(e)
        if (action === 'start' || action === 'restart' || action === 'reload') {
          notifyManualStartFailure(s.id, s.name, diag)
        } else {
          notify.error(diag.problem || `Failed to ${action} ${s.name}`, {
            details: diag.cause,
          })
        }
      }
    },
    [],
  )

  const web = data?.web
  const webStatus = web
    ? serviceStatuses.get(web.default_server) ??
      data?.services.find((service) => service.id === web.default_server)?.state
    : undefined
  const autostartIds = startup?.autostart_services ?? []
  const isWebInAutostart =
    autostartIds.some((id) => ['nginx', 'apache', 'caddy'].includes(id)) || autostartIds.includes('web')

  // Stop all targets EVERYTHING running (services + web stack + workers + tunnels)
  const runningServices = (data?.services ?? []).filter((s) => s.running)
  const hasRunning = runningServices.length > 0 || web?.running === true
  const totalRunningCount = runningServices.length + (web?.running ? 1 : 0)

  // Start all specifically targets AUTOSTART apps/services only
  const autostartServices = (data?.services ?? []).filter((s) => autostartIds.includes(s.id))
  const targetsToStart =
    autostartIds.length > 0
      ? autostartServices.filter((s) => s.installed && !s.running)
      : (data?.services ?? []).filter((s) => s.installed && !s.running)
  const shouldStartWeb = autostartIds.length > 0 ? (isWebInAutostart && !web?.running) : (web && !web.running)

  const canStart = targetsToStart.length > 0 || shouldStartWeb
  const autostartStartCount = targetsToStart.length + (shouldStartWeb ? 1 : 0)

  const phpDisplay = useMemo(() => {
    if (!web?.php_pools || web.php_pools.length === 0) return 'None'
    return web.php_pools
      .map((p) => `${p.version} · ${p.ports.length} worker${p.ports.length === 1 ? '' : 's'}${p.running ? '' : ' (stopped)'}`)
      .join(', ')
  }, [web?.php_pools])

  return (
    <div className="@container/dashboard dashboard-page-container">
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
              hasRunning &&
                'border border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive dark:bg-destructive/[0.18] dark:text-red-300 dark:hover:bg-destructive/25'
            )}
            disabled={busy !== null || (!hasRunning && !canStart)}
            title={
              hasRunning
                ? `Stop all ${totalRunningCount} running service${totalRunningCount === 1 ? '' : 's'}, workers, tunnels, and the web stack`
                : canStart
                  ? `Start ${autostartStartCount} autostart service${autostartStartCount === 1 ? '' : 's'}`
                  : 'All autostart services are already running'
            }
            onClick={() =>
              hasRunning
                ? void (async () => {
                    const ok = await confirmAction(
                      'Stop all running services, workers, tunnels, and the web stack?',
                      'Stop all services',
                      'Stop all services',
                    )
                    if (!ok) return
                    await run('stop-all', async () => {
                      const res = await runCommand({ type: 'stop_all' })
                      if (res.type === 'stopped_everything') {
                        const failed = res.still_running.length
                        const total = totalRunningCount
                        if (failed > 0) {
                          notify.batch({
                            operation: 'services stopped',
                            total: Math.max(total, 1),
                            succeeded: Math.max(0, total - failed),
                            failed,
                            failedItems: res.still_running,
                          })
                          throw {
                            problem: 'Something did not stop.',
                            cause: `Still running after Stop all: ${res.still_running.join(', ')}`,
                            fix: 'Close it from Task Manager and press Stop all again.',
                          }
                        }
                      }
                      await refresh()
                      setDiagToken((t) => t + 1)
                    })
                  })()
                : void (async () => {
                    await run('start-all', async () => {
                      // Build deduplicated start targets so we never dispatch both start_service(apache)
                      // and apply_web for the same web server simultaneously.
                      const effectiveTargets = [...targetsToStart]
                      const hasWebTarget = effectiveTargets.some((s) =>
                        ['nginx', 'apache', 'caddy'].includes(s.id),
                      )
                      if (shouldStartWeb && !hasWebTarget && web?.default_server) {
                        const defaultWebSvc = (data?.services ?? []).find(
                          (s) => s.id === web.default_server && s.installed && !s.running,
                        )
                        if (defaultWebSvc) {
                          effectiveTargets.push(defaultWebSvc)
                        }
                      }

                      const promises: Promise<unknown>[] = effectiveTargets.map((service) =>
                        runSerializedServiceOp(service.id, 'start', async () => {
                          await runCommand({ type: 'start_service', id: service.id })
                          await waitForService(service.id, 'running')
                        }),
                      )
                      const results = await Promise.allSettled(promises)
                      const latestData = await refresh()
                      setDiagToken((t) => t + 1)

                      const liveServices = latestData?.services ?? []
                      const failedEntries: { svc: ServiceStatus; diag: Diagnostic }[] = []

                      results.forEach((r, i) => {
                        const svc = effectiveTargets[i]
                        if (!svc) return
                        const liveSvc = liveServices.find((x) => x.id === svc.id)
                        const isReadyNow = Boolean(liveSvc && liveSvc.running && liveSvc.healthy !== false)
                        if (isReadyNow) {
                          resolveNotificationsForService(svc.id)
                          return
                        }
                        if (r.status === 'rejected') {
                          failedEntries.push({ svc, diag: asDiagnostic(r.reason) })
                        }
                      })

                      if (failedEntries.length === 1) {
                        const { svc, diag } = failedEntries[0]
                        notifyManualStartFailure(svc.id, svc.name, diag)
                      } else if (failedEntries.length > 1) {
                        for (const { svc, diag } of failedEntries) {
                          notifyManualStartFailure(svc.id, svc.name, diag)
                        }
                      }
                    })
                  })()
            }
          >
            {busy === 'stop-all' || busy === 'start-all' ? (
              <Spinner />
            ) : hasRunning ? (
              <StopIcon />
            ) : (
              <Play />
            )}{' '}
            {busy === 'stop-all'
              ? 'Stopping all…'
              : busy === 'start-all'
                ? 'Starting autostart…'
                : hasRunning
                  ? 'Stop all'
                  : 'Start all'}
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {dataError && <ErrorCard error={dataError} onDismiss={() => setDataError(null)} />}

      {/* Diagnostics: strictly one slim row (<= 44px) in all states so layout never jumps */}
      <DiagnosticsCard
        refreshToken={diagToken}
        onNavigate={onNavigate}
        onRefresh={refresh}
        healthProblems={data?.health ?? []}
      />

      {/* Web server strip: one full-width card (~52-58px), single row vertically centered */}
      <Card className="shadow-xs min-h-[52px] px-3.5 py-2 flex flex-wrap items-center justify-between gap-3">
        {/* Left: Title + Status pill */}
        <div className="flex items-center gap-2.5 shrink-0">
          <span className="text-sm font-semibold text-foreground">Web server</span>
          {webStatus === 'starting' || webStatus === 'restarting' ? (
            <Badge variant="warning" className="shrink-0 whitespace-nowrap text-xs py-0.5">
              {webStatus === 'restarting' ? 'Restarting…' : 'Starting…'}
            </Badge>
          ) : webStatus === 'stopping' ? (
            <Badge variant="warning" className="shrink-0 whitespace-nowrap text-xs py-0.5">
              Stopping…
            </Badge>
          ) : web?.running || webStatus === 'running' ? (
            <Badge variant="success" className="shrink-0 whitespace-nowrap text-xs py-0.5">
              ● Running
            </Badge>
          ) : webStatus === 'error' ? (
            <Badge variant="destructive" className="shrink-0 whitespace-nowrap text-xs py-0.5">
              Error
            </Badge>
          ) : (
            <Badge variant="secondary" className="shrink-0 whitespace-nowrap text-xs py-0.5">
              Stopped
            </Badge>
          )}
        </div>

        {/* Middle: Inline label/value pairs with thin vertical dividers */}
        <div
          className={cn(
            'flex flex-wrap items-center gap-x-3.5 gap-y-1 text-xs transition-opacity',
            !web?.running && 'opacity-65'
          )}
        >
          {/* Server */}
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="text-[11px] font-medium text-muted-foreground">Server</span>
            <span className="font-semibold text-foreground truncate">
              {web?.servers.find((s) => s.active)?.name ?? (web?.servers[0]?.name ?? '—')}
            </span>
          </div>

          <div className="h-3.5 w-px bg-border/60 shrink-0 hidden sm:block" />

          {/* Ports */}
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="text-[11px] font-medium text-muted-foreground">Ports</span>
            <span className="font-mono font-semibold text-foreground truncate">
              {web?.http_port ?? 80} / {web?.https_port ?? 443}
            </span>
          </div>

          <div className="h-3.5 w-px bg-border/60 shrink-0 hidden sm:block" />

          {/* PHP */}
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="text-[11px] font-medium text-muted-foreground">PHP</span>
            <span className="font-semibold text-foreground truncate" title={phpDisplay}>
              {phpDisplay !== 'None' ? phpDisplay : '8.4 · 3 workers'}
            </span>
          </div>

          <div className="h-3.5 w-px bg-border/60 shrink-0 hidden sm:block" />

          {/* Wildcard DNS */}
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="text-[11px] font-medium text-muted-foreground">Wildcard DNS</span>
            <span className="font-semibold text-foreground truncate">
              {web?.dns_running
                ? `Port ${web.dns_port}`
                : web?.dns_port
                  ? `Port ${web.dns_port}`
                  : 'Port 53'}
            </span>
          </div>
        </div>

        {/* Right: Open home page button */}
        <Button
          size="sm"
          variant="secondary"
          className="h-7 text-xs font-medium gap-1.5 shrink-0 ml-auto"
          disabled={!web?.running}
          title={web?.running ? 'Open the built-in welcome site' : 'Start the web server first'}
          onClick={() =>
            runCommand({ type: 'open_url', url: 'https://openlocalserver.test' }).catch(() => undefined)
          }
        >
          <Home className="size-3.5" /> Open home page
        </Button>
      </Card>

      {/* Two independent vertical stacks with uniform 16px gap */}
      <div className="dashboard-columns-container">
        {/* Left stack: Resources, then Traffic (flexible width) */}
        <div className="dashboard-left-stack">
          {/* 1. Resources card */}
          <Card className="dashboard-card-resources shadow-xs">
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
            <CardContent className="px-3.5 pb-3.5 pt-0">
              {/* 3 equal-width tiles with ring on top, label below, detail under label */}
              <div className="grid grid-cols-3 divide-x divide-border/40 py-1">
                <ResourceDonut
                  label="CPU"
                  percent={stats?.cpu_percent ?? 0}
                  detail={stats ? `${stats.cpu_cores} cores` : '…'}
                  variant="cpu"
                  loading={!stats}
                />
                <ResourceDonut
                  label="RAM"
                  percent={stats?.memory_used && stats?.memory_total ? (stats.memory_used / stats.memory_total) * 100 : 0}
                  detail={stats ? `${formatBytes(stats.memory_used)} / ${formatBytes(stats.memory_total)}` : '…'}
                  variant="ram"
                  loading={!stats}
                />
                {(stats?.disks ?? []).slice(0, 1).map((d) => (
                  <ResourceDonut
                    key={d.mount}
                    label={`Disk ${d.mount.replace(/\\$/, '')}`}
                    percent={d.total > 0 ? (d.used / d.total) * 100 : 0}
                    detail={stats ? `${formatBytes(d.total - d.used)} free` : '…'}
                    variant="disk"
                    loading={!stats}
                  />
                ))}
              </div>
            </CardContent>
          </Card>

          {/* 2. Traffic card: natural height, fixed 180px chart, real pixel rendering without distortion */}
          <Card className="dashboard-card-traffic shadow-xs">
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
            <CardContent className="flex flex-col gap-2.5 px-3.5 pb-3.5 pt-0">
              <TrafficGraph lines={accessLines} webRunning={web?.running ?? false} />
            </CardContent>
          </Card>
        </div>

        {/* Right stack: Services card (fixed ~520px at >= 1100px) */}
        <div className="dashboard-right-stack min-w-0">
          <ServicesWidget
            services={data?.services ?? null}
            webServers={data?.web?.servers ?? null}
            serviceStatuses={serviceStatuses}
            onAction={onServiceAction}
            onNavigate={onNavigate}
            onOpenLogs={onOpenLogs}
          />
        </div>
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

/**
 * Services card: services grouped by type with collapsible section headers and fixed 4-slot rows.
 * OVERRIDE: Group headers do NOT have group stop/start buttons.
 */
function ServicesWidget({
  services,
  webServers,
  serviceStatuses,
  onAction,
  onNavigate,
  onOpenLogs,
}: {
  services: ServiceStatus[] | null
  webServers: ServerAvailability[] | null
  serviceStatuses: ReadonlyMap<string, ServiceLifecycleState>
  onAction: (s: ServiceStatus, action: WidgetAction) => Promise<void>
  onNavigate: (p: Page) => void
  onOpenLogs: (source: string) => void
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

  function handleAction(s: ServiceStatus, action: WidgetAction) {
    void onAction(s, action)
  }

  return (
    <Card className="shadow-xs flex flex-col">
      <CardHeader className="flex-row items-center justify-between space-y-0 px-3.5 py-2.5">
        <div className="flex items-baseline gap-2">
          <CardTitle className="text-sm font-semibold">Services</CardTitle>
          <span className="text-xs text-muted-foreground tabular-nums">
            {running} running · {installed} installed
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="h-7 shrink-0 rounded-md px-2 text-xs text-muted-foreground hover:text-foreground"
          onClick={() => onNavigate('services')}
        >
          Manage
        </Button>
      </CardHeader>
      <CardContent className="p-0">
        {groups.map((group, groupIndex) => {
          const runningInGroup = group.services.filter((s) => s.running).length
          const activeOrStartingInGroup = group.services.filter((s) => {
            const status = getServiceLifecycleState(s, serviceStatuses)
            return s.running || status === 'starting' || status === 'stopping' || status === 'restarting'
          }).length
          // Collapsed by default when all services in group are stopped (and none transitioning)
          const isCollapsed =
            collapsed[group.id] !== undefined ? !!collapsed[group.id] : activeOrStartingInGroup === 0

          return (
            <div key={group.id} className="min-w-0">
              {/* Group header: chevron + group name + n/m count only (no redundant action buttons) */}
              <div
                className={cn(
                  'flex w-full min-w-0 h-9 items-center justify-between px-3 transition-colors hover:bg-muted/30 select-none cursor-pointer',
                  groupIndex > 0 && 'border-t border-border/60'
                )}
                onClick={() => toggleGroup(group.id)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    toggleGroup(group.id)
                  }
                }}
                aria-expanded={!isCollapsed}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <ChevronDown
                    className={cn(
                      'size-3.5 shrink-0 text-muted-foreground transition-transform duration-200',
                      isCollapsed && '-rotate-90'
                    )}
                  />
                  <span className="min-w-0 truncate text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/90 dark:text-muted-foreground">
                    {group.label}
                  </span>
                  <span className="tabular-nums text-xs text-muted-foreground/80 font-medium">
                    {runningInGroup}/{group.services.length}
                  </span>
                </div>
              </div>

              {!isCollapsed && (
                <div>
                  {group.services.map((s) => {
                    const serviceStatus = getServiceLifecycleState(s, serviceStatuses)
                    const isStarting =
                      serviceStatus === 'starting' || serviceStatus === 'restarting'
                    const isStopping = serviceStatus === 'stopping'
                    const isPending = isStarting || isStopping
                    const isRunning = serviceStatus === 'running' || (isStopping && s.running)
                    const state = !s.installed
                      ? 'missing'
                      : isStarting
                        ? 'starting'
                        : isStopping
                          ? 'stopping'
                          : serviceStatus === 'error'
                            ? 'unhealthy'
                            : isRunning
                            ? s.healthy === false
                              ? 'unhealthy'
                              : 'running'
                            : 'stopped'

                    return (
                      <div
                        key={s.id}
                        className="grid grid-cols-[1fr_64px_138px] items-center gap-2 px-3 h-10 border-t border-border/60 transition-colors hover:bg-muted/25 min-w-0 select-none"
                      >
                        {/* Col 1: Icon + Name */}
                        <div className="flex items-center gap-2.5 min-w-0">
                          <span
                            className="shrink-0 flex items-center justify-center"
                            title={`${s.name} (${state})`}
                          >
                            <ServiceMark id={s.id} state={state} className="size-4 [&>svg]:size-3" />
                          </span>
                          <span
                            className="min-w-0 truncate text-[13px] font-medium text-foreground"
                            title={`${s.name} (${state})`}
                          >
                            {s.name}
                          </span>
                        </div>

                        {/* Col 2: Port: fixed ~64px, right-aligned, monospace */}
                        <span
                          className="font-mono text-xs tabular-nums text-muted-foreground/80 text-right truncate"
                          title={servicePortTitle(s, webSites.get(s.id))}
                        >
                          {s.port === null ? '—' : s.port}
                        </span>

                        {/* Col 3: 4 action slots (fixed 30px each, 6px gap = 138px) */}
                        <div className="flex items-center gap-1.5 justify-end shrink-0 w-[138px]">
                          {/* Slot 1: Start/Stop toggle button */}
                          <button
                            type="button"
                            title={
                              isPending
                                  ? `${isStarting ? serviceStatus === 'restarting' ? 'Restarting' : 'Starting' : 'Stopping'} ${s.name}…`
                                  : isRunning
                                  ? `Stop ${s.name}`
                                  : `Start ${s.name}`
                            }
                            aria-label={
                              isPending
                                  ? `${isStarting ? serviceStatus === 'restarting' ? 'Restart' : 'Start' : 'Stop'}ing ${s.name}`
                                  : isRunning
                                  ? `Stop ${s.name}`
                                  : `Start ${s.name}`
                            }
                            disabled={isPending || !s.installed}
                            onClick={() => void handleAction(s, s.running ? 'stop' : 'start')}
                            className={cn(
                              'action-slot-btn',
                              isPending
                                ? 'action-slot-pending'
                                : isRunning
                                  ? 'action-slot-stop'
                                  : 'action-slot-start'
                            )}
                          >
                            {isPending ? (
                              <Spinner className="size-3.5 shrink-0" />
                            ) : isRunning ? (
                              <StopIcon />
                            ) : (
                              <Play />
                            )}
                          </button>

                          {/* Slot 2: Restart */}
                          <button
                            type="button"
                            title="Restart"
                            aria-label="Restart"
                            disabled={isPending || !s.installed || !isRunning}
                            onClick={() => void handleAction(s, 'restart')}
                            className="action-slot-btn action-slot-restart"
                          >
                            <RotateCw />
                          </button>

                          {/* Slot 3: Reload config */}
                          <button
                            type="button"
                            title="Reload config"
                            aria-label="Reload config"
                            disabled={isPending || !s.installed || !isRunning || s.kind !== 'web'}
                            onClick={() => void handleAction(s, 'reload')}
                            className="action-slot-btn action-slot-reload"
                          >
                            <Repeat />
                          </button>

                          {/* Slot 4: View logs */}
                          <button
                            type="button"
                            title="View logs"
                            aria-label="View logs"
                            disabled={!s.installed}
                            onClick={() => onOpenLogs(s.log_source || `service:${s.id}`)}
                            className="action-slot-btn action-slot-logs"
                          >
                            <FileText />
                          </button>
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

/**
 * Three centered tiles for CPU, RAM, Disk.
 * Diameter 80px (viewBox 0 0 80 80, radius 33, stroke 7px).
 * Scale font to ~22% of diameter (~16px bold) so 100% fits with >= 10px clearance.
 */
function ResourceDonut({
  label,
  percent,
  detail,
  variant,
  loading,
}: {
  label: string
  percent: number
  detail: string
  variant: 'cpu' | 'ram' | 'disk'
  loading?: boolean
}) {
  const p = Math.min(100, Math.max(0, Math.round(percent)))
  const r = 33
  const c = 2 * Math.PI * r

  // Semantic threshold colors
  let color = 'var(--action-start)'
  if (variant === 'cpu') {
    color = p >= 90 ? 'var(--action-stop)' : p >= 75 ? 'var(--action-restart)' : 'var(--action-start)'
  } else if (variant === 'ram') {
    color = p >= 90 ? 'var(--action-stop)' : p >= 80 ? 'var(--action-restart)' : 'var(--action-reload)'
  } else {
    color = p >= 95 ? 'var(--action-stop)' : p >= 85 ? 'var(--action-restart)' : 'var(--action-logs)'
  }

  return (
    <div className="flex flex-col items-center justify-center text-center p-2 min-w-0">
      <div className="relative size-20 shrink-0">
        <svg viewBox="0 0 80 80" className="size-full -rotate-90">
          <circle cx="40" cy="40" r={r} fill="none" stroke="var(--muted)" strokeWidth="7" />
          <circle
            cx="40"
            cy="40"
            r={r}
            fill="none"
            stroke={color}
            strokeWidth="7"
            strokeLinecap="round"
            strokeDasharray={c}
            strokeDashoffset={c * (1 - p / 100)}
            className="transition-[stroke-dashoffset,stroke] duration-500"
          />
        </svg>
        <span
          className="absolute inset-0 flex items-center justify-center text-base font-bold tabular-nums"
          style={{ color }}
        >
          {loading ? '…' : `${p}%`}
        </span>
      </div>
      <span className="text-xs font-semibold text-foreground mt-2">{label}</span>
      <span className="text-[11px] text-muted-foreground tabular-nums truncate max-w-full" title={detail}>
        {detail}
      </span>
    </div>
  )
}

const MONTHS: Record<string, number> = {
  Jan: 0,
  Feb: 1,
  Mar: 2,
  Apr: 3,
  May: 4,
  Jun: 5,
  Jul: 6,
  Aug: 7,
  Sep: 8,
  Oct: 9,
  Nov: 10,
  Dec: 11,
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

const REQ_COLOR = 'var(--action-start)'
const DATA_COLOR = 'var(--action-restart)'

/**
 * Monotone Hermite spline clamped to never dip below the zero baseline.
 */
function monotonePath(pts: { x: number; y: number }[], baseline: number): string {
  const n = pts.length
  if (n === 0) return ''
  if (n === 1) return `M ${pts[0].x},${pts[0].y}`

  const deltas: number[] = new Array(n - 1)
  const m: number[] = new Array(n)

  for (let i = 0; i < n - 1; i++) {
    const dx = pts[i + 1].x - pts[i].x
    deltas[i] = dx === 0 ? 0 : (pts[i + 1].y - pts[i].y) / dx
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

const TrafficGraph = memo(function TrafficGraph({
  lines,
  webRunning = true,
}: {
  lines: string[]
  webRunning?: boolean
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(320)

  useEffect(() => {
    if (!containerRef.current) return
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const w = Math.floor(entry.contentRect.width)
        if (w > 0) setWidth(w)
      }
    })
    ro.observe(containerRef.current)
    return () => ro.disconnect()
  }, [])

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

  const H = 180
  const PAD_L = 28
  const BASELINE = H - 24
  const TOP_PAD = 12

  const pts = (series: number[]) =>
    series.map((v, i) => ({
      x: PAD_L + (i / (N - 1)) * (width - PAD_L - 8),
      y: Math.min(BASELINE, TOP_PAD + (1 - v / niceMax) * (BASELINE - TOP_PAD)),
    }))

  const upPts = pts(req)
  const downPts = pts(kb)
  const upLine = monotonePath(upPts, BASELINE)
  const downLine = monotonePath(downPts, BASELINE)
  const area = (line: string) => `${line} L ${width - 8},${BASELINE} L ${PAD_L},${BASELINE} Z`

  const windowReq = req.reduce((a, b) => a + b, 0)
  const windowKb = kb.reduce((a, b) => a + b, 0)

  const fmtT = (ms: number) => new Date(ms).toTimeString().slice(0, 8)
  const bucketT = (i: number) => end - ((N - 1 - i) / N) * WIN_MS

  // Max 3 round ticks: 0, niceMax/2, niceMax
  const ticks = [0, Math.round(niceMax / 2), niceMax]

  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    const x = e.clientX - rect.left
    const frac = (x - PAD_L) / (width - PAD_L - 8)
    setHover(Math.max(0, Math.min(N - 1, Math.round(frac * (N - 1)))))
  }

  const hov = hover !== null ? { i: hover, t: bucketT(hover), r: req[hover], k: kb[hover], x: upPts[hover]?.x ?? 0 } : null

  return (
    <div className="flex min-w-0 flex-col gap-2.5">
      {/* Requests and Served sit next to each other as compact stat blocks */}
      <div className="flex flex-wrap items-center gap-3 rounded-md bg-muted/20 px-3 py-1.5 text-xs w-fit">
        <div className="flex items-center gap-1.5">
          <span className="size-2 rounded-full shrink-0" style={{ background: REQ_COLOR }} />
          <span className="text-muted-foreground">Requests:</span>
          <span className="text-sm font-bold tabular-nums text-foreground">{windowReq}</span>
          <span className="text-[11px] text-muted-foreground/80 tabular-nums">({totalLogReq} total)</span>
        </div>
        <div className="h-3 w-px bg-border/60 shrink-0" />
        <div className="flex items-center gap-1.5">
          <span className="size-2 rounded-full shrink-0" style={{ background: DATA_COLOR }} />
          <span className="text-muted-foreground">Served:</span>
          <span className="text-sm font-bold tabular-nums text-foreground truncate">
            {windowKb >= 1024 ? `${(windowKb / 1024).toFixed(1)} MB` : `${windowKb.toFixed(0)} KB`}
          </span>
          <span className="text-[11px] text-muted-foreground/80 tabular-nums shrink-0">
            ({totalLogKb >= 1024 ? `${(totalLogKb / 1024).toFixed(1)} MB` : `${totalLogKb.toFixed(0)} KB`})
          </span>
        </div>
      </div>

      {/* Chart container with fixed height and ResizeObserver to prevent distortion */}
      <div ref={containerRef} className="w-full h-[180px] relative">
        {!webRunning ? (
          <div className="flex h-full items-center justify-center gap-2 rounded-md border border-dashed border-border/50 px-3 text-xs text-muted-foreground/80">
            <Globe className="size-4 opacity-50 shrink-0" />
            <span>Web server is stopped. Traffic will appear here.</span>
          </div>
        ) : totalLogReq === 0 && windowReq === 0 ? (
          <div className="flex h-full items-center justify-center gap-2 rounded-md border border-dashed border-border/50 px-3 text-xs text-muted-foreground/80">
            <Globe className="size-4 opacity-50 shrink-0" />
            <span>No traffic recorded in the last 5 minutes</span>
          </div>
        ) : (
          <div className="w-full h-full relative" onMouseLeave={() => setHover(null)}>
            <svg
              viewBox={`0 0 ${width} ${H}`}
              width={width}
              height={H}
              className="w-full h-[180px] overflow-visible"
              onMouseMove={onMove}
            >
              <defs>
                <linearGradient id="trafUp" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={REQ_COLOR} stopOpacity="0.38" />
                  <stop offset="100%" stopColor={REQ_COLOR} stopOpacity="0.02" />
                </linearGradient>
                <linearGradient id="trafDown" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={DATA_COLOR} stopOpacity="0.38" />
                  <stop offset="100%" stopColor={DATA_COLOR} stopOpacity="0.02" />
                </linearGradient>
              </defs>

              {/* Gridlines and undistorted round tick labels */}
              {ticks.map((t) => {
                const y = Math.round(TOP_PAD + (1 - t / niceMax) * (BASELINE - TOP_PAD))
                return (
                  <g key={t}>
                    <line
                      x1={PAD_L}
                      x2={width - 8}
                      y1={y}
                      y2={y}
                      stroke="var(--border)"
                      strokeWidth="0.75"
                      strokeDasharray="3 3"
                      opacity="0.6"
                    />
                    <text
                      x={PAD_L - 6}
                      y={y + 3.5}
                      textAnchor="end"
                      fontSize="11"
                      fill="var(--muted-foreground)"
                      className="font-mono tabular-nums select-none"
                    >
                      {t}
                    </text>
                  </g>
                )
              })}

              <path d={area(downLine)} fill="url(#trafDown)" />
              <path d={area(upLine)} fill="url(#trafUp)" />
              <path d={downLine} fill="none" stroke={DATA_COLOR} strokeWidth="2" vectorEffect="non-scaling-stroke" />
              <path d={upLine} fill="none" stroke={REQ_COLOR} strokeWidth="2" vectorEffect="non-scaling-stroke" />

              {hov !== null && (
                <g>
                  <line
                    x1={hov.x}
                    x2={hov.x}
                    y1={TOP_PAD}
                    y2={BASELINE}
                    stroke="var(--muted-foreground)"
                    strokeWidth="1"
                    strokeDasharray="2 2"
                    opacity="0.9"
                  />
                  <circle cx={upPts[hov.i].x} cy={upPts[hov.i].y} r="3" fill={REQ_COLOR} stroke="var(--card)" strokeWidth="1.5" />
                  <circle cx={downPts[hov.i].x} cy={downPts[hov.i].y} r="3" fill={DATA_COLOR} stroke="var(--card)" strokeWidth="1.5" />
                </g>
              )}
            </svg>

            {hov !== null && (
              <div
                className="pointer-events-none absolute z-10 -translate-x-1/2 rounded-md border border-border bg-card px-2.5 py-1.5 text-xs shadow-lg"
                style={{ left: `${Math.min(85, Math.max(15, (hov.x / width) * 100))}%`, top: '4px' }}
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
      </div>

      <div className="flex justify-between text-[11px] tabular-nums text-muted-foreground/80 shrink-0">
        <span>Last 5 min ({fmtT(end - WIN_MS)})</span>
        <span>Now ({fmtT(end)})</span>
      </div>
    </div>
  )
})
