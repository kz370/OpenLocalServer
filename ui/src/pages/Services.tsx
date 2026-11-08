import { ExternalLink, FileText, Moon, Play, RefreshCw, RotateCw } from 'lucide-react'
import { useEffect, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { CustomServices } from '@/components/CustomServices'
import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { StopIcon } from '@/components/StopIcon'
import { ServiceMark } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, SwitchRow } from '@/components/ui/form'
import { NumberInput } from '@/components/ui/input'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  type Diagnostic,
  type IdleConfig,
  type IdleSettings,
  type PortStatus,
  type ServiceLifecycleState,
  type ServiceStatus,
  runCommand,
} from '@/core'
import { createPersistentCache } from '@/lib/cache'
import {
  notifyManualStartFailure,
  runSerializedServiceOp,
} from '@/lib/diagnosticsNotify'
import { getServiceLifecycleState, useServiceStatuses } from '@/lib/serviceStatus'
import { waitForService } from '@/lib/wait'

type ServiceAction = 'start' | 'stop' | 'restart' | 'reload'

const servicesListCache = createPersistentCache<ServiceStatus[]>('services_list', [])

export function ServicesPage({ onOpenLogs }: { onOpenLogs: (source: string) => void }) {
  const [services, setServices] = useState<ServiceStatus[]>(servicesListCache.get)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [idleFor, setIdleFor] = useState<ServiceStatus | null>(null)
  const serviceStatuses = useServiceStatuses()

  async function refresh() {
    try {
      const res = await runCommand({ type: 'list_services' })
      if (res.type === 'services') {
        setServices(res.services)
        servicesListCache.set(res.services)
      }
    } catch (err) {
      // An empty list and a failed list look identical on screen, so the failure has
      // to be shown or the page silently claims nothing is registered.
      setError(asDiagnostic(err))
    }
  }

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, 3000)
    return () => clearInterval(interval)
  }, [])

  /** The full per-service suite, same set the Dashboard offers (§ see ServicesWidget).
   * `busy` is keyed `svc:<id>:<action>` so the spinner sits on the control that was
   * pressed and the other four stay usable, rather than the whole row going dead. */
  async function act(service: ServiceStatus, action: ServiceAction) {
    setError(null)
    try {
      await runSerializedServiceOp(service.id, action, async () => {
        if (action === 'reload') {
          // A web server reloads its config in place: no dropped connections.
          await runCommand({ type: 'apply_web', overwrite: [] })
        } else if (action === 'restart') {
          await runCommand({ type: 'restart_service', id: service.id })
          await waitForService(service.id, 'running')
        } else {
          await runCommand({ type: action === 'start' ? 'start_service' : 'stop_service', id: service.id })
          await waitForService(service.id, action === 'start' ? 'running' : 'stopped')
        }
      })
      await refresh()
    } catch (err) {
      const diag = asDiagnostic(err)
      setError(diag)
      if (action === 'start' || action === 'restart') {
        notifyManualStartFailure(service.id, service.name, diag)
      }
      await refresh().catch(() => undefined)
    }
  }

  // The three web servers get their own table: each starts and stops on its own, on
  // the port it binds right now (§3.3).
  const webServers = services.filter((s) => s.kind === 'web')
  const otherServices = services.filter((s) => s.kind !== 'web')

  return (
    <PageLayout
      mode="scroll"
      header={
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Services</h1>
        <p className="text-sm text-muted-foreground">Web servers, mail testing and databases — each one a process you start and stop.</p>
      </div>
      }
    >

      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Web servers</CardTitle>
          <CardDescription>
            They can run at the same time on different ports. The default one owns 80/443 and serves every site that hasn't picked a
            server of its own; the rest serve only the sites pinned to them.
          </CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ServiceTable
            services={webServers}
            serviceStatuses={serviceStatuses}
            onAct={act}
            onOpenLogs={onOpenLogs}
            onError={setError}
            portLabel={(s) => (s.port === 80 ? '80 / 443 (default)' : s.port === null ? '—' : `${s.port} (custom)`)}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Services</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <ServiceTable
            services={otherServices}
            serviceStatuses={serviceStatuses}
            onAct={act}
            onOpenLogs={onOpenLogs}
            onError={setError}
            onIdle={setIdleFor}
          />
        </CardContent>
      </Card>

      <CustomServices onChanged={refresh} />
      <IdleDialog service={idleFor} onClose={() => setIdleFor(null)} onError={setError} />
    </PageLayout>
  )
}

/** One action table. Shared by the web servers and the other services. */
function ServiceTable({
  services,
  serviceStatuses,
  onAct,
  onOpenLogs,
  onError,
  onIdle,
  portLabel,
}: {
  services: ServiceStatus[]
  serviceStatuses: ReadonlyMap<string, ServiceLifecycleState>
  onAct: (s: ServiceStatus, action: ServiceAction) => void
  onOpenLogs: (source: string) => void
  onError: (e: Diagnostic) => void
  /** Opens "Sleep when unused" for a service that supports it. */
  onIdle?: (s: ServiceStatus) => void
  portLabel?: (s: ServiceStatus) => string
}) {
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Name</TableHead>
          <TableHead>Port</TableHead>
          <TableHead>Status</TableHead>
          <TableHead className="text-right">Action</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {services.map((s) => {
          const state = getServiceLifecycleState(s, serviceStatuses)
          const isStarting = state === 'starting' || state === 'restarting'
          const isStopping = state === 'stopping'
          const isPending = isStarting || isStopping
          const isRunning = state === 'running' || (isStopping && s.running)
          return (
          <TableRow key={s.id}>
            <TableCell>
              <span className="flex items-center gap-2">
                <ServiceMark
                  id={s.id}
                  state={
                    !s.installed
                      ? 'missing'
                      : isStarting
                        ? 'starting'
                        : isStopping
                          ? 'stopping'
                          : state === 'error'
                            ? 'unhealthy'
                            : isRunning
                              ? s.healthy === false
                                ? 'unhealthy'
                                : 'running'
                              : 'stopped'
                  }
                />
                <span className="font-medium">{s.name}</span>
              </span>
            </TableCell>
            <TableCell className="font-mono text-xs tabular-nums text-foreground/75">
              {portLabel ? portLabel(s) : s.installed ? (s.port ?? '—') : '—'}
            </TableCell>
            <TableCell>
              {!s.installed ? (
                <Badge variant="outline">not installed</Badge>
              ) : isStarting ? (
                <Badge variant="secondary">{state === 'restarting' ? 'Restarting…' : 'Starting…'}</Badge>
              ) : isStopping ? (
                <Badge variant="secondary">Stopping…</Badge>
              ) : isRunning ? (
                <span className="flex flex-wrap items-center gap-1.5">
                  <Badge variant="success">● Running</Badge>
                  {s.healthy === false && <Badge variant="warning">not answering</Badge>}
                  {s.port_status === 'in_use' && <Badge variant="warning">port in use</Badge>}
                  {s.kind === 'web' && (s.sites ?? 0) === 0 && <Badge variant="outline">no sites</Badge>}
                </span>
              ) : state === 'error' ? (
                <Badge variant="destructive">Error</Badge>
              ) : (
                <span className="flex flex-col items-start gap-1">
                  <span className="flex flex-wrap items-center gap-1.5">
                    {s.sleeping ? (
                      <Badge variant="secondary">
                        <Moon className="mr-1 size-3" /> Sleeping
                      </Badge>
                    ) : (
                      <Badge variant="secondary">Stopped</Badge>
                    )}
                    {s.port_status === 'in_use' && <Badge variant="warning">port in use</Badge>}
                  </span>
                  {s.installed && s.port !== null && s.port_status === 'in_use' && <PortHolder port={s.port} />}
                </span>
              )}
            </TableCell>
            <TableCell className="text-right">
              <div className="flex items-center justify-end gap-2">
                {s.id === 'mailpit' && s.running && s.port !== null && (
                  <button
                    type="button"
                    className="inline-flex cursor-pointer items-center gap-1 text-xs text-primary hover:underline"
                    title={`Open Mailpit at http://127.0.0.1:${s.port}`}
                    onClick={() => runCommand({ type: 'open_url', url: `http://127.0.0.1:${s.port}` }).catch((e) => onError(e as Diagnostic))}
                  >
                    Open <ExternalLink className="size-3" />
                  </button>
                )}
                {s.installed && (
                  <span className="flex items-center justify-end gap-0.5">
                    <ServiceIconAction
                      title={`${isRunning ? 'Stop' : 'Start'} ${s.name}`}
                      disabled={isPending}
                      spinning={isPending}
                      iconColor={isRunning ? 'text-destructive/70' : 'text-emerald-400'}
                      onClick={() => onAct(s, isRunning ? 'stop' : 'start')}
                    >
                      {isRunning ? <StopIcon /> : <Play className="size-3.5" />}
                    </ServiceIconAction>
                    <ServiceIconAction
                      title={`Restart ${s.name}`}
                      // Restart is a stop followed by a start, so it is available whether or
                      // not the service is up right now — the same rule the Dashboard uses.
                      disabled={isPending}
                      spinning={state === 'restarting'}
                      iconColor="text-amber-400"
                      onClick={() => onAct(s, 'restart')}
                    >
                      <RotateCw className="size-3.5" />
                    </ServiceIconAction>
                    <ServiceIconAction
                      title={`Reload ${s.name} config`}
                      disabled={s.kind !== 'web' || isPending}
                      spinning={state === 'restarting'}
                      iconColor="text-sky-400"
                      onClick={() => onAct(s, 'reload')}
                    >
                      <RefreshCw className="size-3.5" />
                    </ServiceIconAction>
                    {onIdle && IDLE_SERVICES.includes(s.id) && (
                      <ServiceIconAction title={`Sleep ${s.name} when unused`} disabled={isPending} iconColor="text-indigo-400" onClick={() => onIdle(s)}>
                        <Moon className="size-3.5" />
                      </ServiceIconAction>
                    )}
                    <ServiceIconAction
                      title={`View ${s.name} logs`}
                      disabled={!s.log_source}
                      iconColor="text-violet-400"
                      onClick={() => s.log_source && onOpenLogs(s.log_source)}
                    >
                      <FileText className="size-3.5" />
                    </ServiceIconAction>
                  </span>
                )}
              </div>
            </TableCell>
          </TableRow>
          )
        })}
        {services.length === 0 && (
          <TableRow>
            <TableCell colSpan={4} className="text-center text-sm text-muted-foreground">
              Nothing registered.
            </TableCell>
          </TableRow>
        )}
      </TableBody>
    </Table>
  )
}

/** Who holds this port (best-effort via netstat). Propose, never kill blindly. */
function PortHolder({ port }: { port: number }) {
  const [holder, setHolder] = useState<PortStatus | null>(null)
  useEffect(() => {
    let alive = true
    const tick = () => {
      runCommand({ type: 'check_port', port })
        .then((r) => alive && r.type === 'port' && setHolder(r.status))
        .catch(() => undefined)
    }
    void tick()
    const t = setInterval(tick, 5000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [port])
  if (!holder || holder.status === 'free') return null
  // Stale PID race (holder exited between netstat and tasklist): port frees
  // on next check, so say retry instead of naming a ghost.
  if (holder.process_name === null) {
    return (
      <span className="max-w-64 text-xs text-muted-foreground" title="The holder exited already or can't be identified. Wait a few seconds, then press Start again.">
        holder gone — retry Start shortly
      </span>
    )
  }
  const pid = holder.pid !== null ? ` (PID ${holder.pid})` : ''
  return (
    <span className="max-w-64 text-xs text-muted-foreground" title="Stop that program safely (its own stop command or Windows Services), then Start here. Killing it by force can corrupt its data.">
      held by {holder.process_name}
      {pid} — stop it first, then Start
    </span>
  )
}

/** The services that can sleep when unused (§179). */
const IDLE_SERVICES = ['mariadb', 'postgres', 'redis', 'mongodb', 'memcached']

/**
 * §179 Sleep when unused: stops a service after N minutes without connections and starts it
 * again when an app connects. Off by default. Rendered outside the table's card.
 */
function IdleDialog({ service, onClose, onError }: { service: ServiceStatus | null; onClose: () => void; onError: (e: Diagnostic) => void }) {
  const [all, setAll] = useState<IdleSettings>({})
  const [cfg, setCfg] = useState<IdleConfig>({ enabled: false, minutes: 30, wake_on_connect: true })

  useEffect(() => {
    if (!service) return
    void runCommand({ type: 'get_idle_settings' })
      .then((r) => {
        if (r.type !== 'idle_settings') return
        setAll(r.settings)
        setCfg(r.settings[service.id] ?? { enabled: false, minutes: 30, wake_on_connect: true })
      })
      .catch((e) => onError(asDiagnostic(e)))
  }, [service, onError])

  async function save() {
    if (!service) return
    try {
      await runCommand({ type: 'set_idle_settings', settings: { ...all, [service.id]: cfg } })
      onClose()
    } catch (e) {
      onError(asDiagnostic(e))
    }
  }

  return (
    <Dialog
      open={service !== null}
      onClose={onClose}
      title={service ? `Sleep ${service.name} when unused` : 'Sleep when unused'}
      description="Saves memory and battery on a laptop."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <SwitchRow checked={cfg.enabled} onChange={(v) => setCfg({ ...cfg, enabled: v })} title="Sleep when unused" hint="Stops the service after a while without connections." />
        <Field label="Minutes without connections (5 to 240)">
          <NumberInput label="Minutes" value={cfg.minutes} min={5} max={240} disabled={!cfg.enabled} onChange={(v) => setCfg({ ...cfg, minutes: v ?? 30 })} className="h-8 w-28" />
        </Field>
        <SwitchRow checked={cfg.wake_on_connect} disabled={!cfg.enabled} onChange={(v) => setCfg({ ...cfg, wake_on_connect: v })} title="Start again when an app connects" hint="OLS listens on 127.0.0.1 and starts the service for the first connection." />
        <p className="text-xs text-muted-foreground">The first connection waits a few seconds while the service starts. A second new connection in that same moment may fail once. Apps that connect to the network address or ::1 do not wake it.</p>
      </div>
    </Dialog>
  )
}

