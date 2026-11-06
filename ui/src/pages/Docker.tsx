import { listen } from '@tauri-apps/api/event'
import { ArrowLeft, Box, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, Layers, Search, ExternalLink, FileText, Globe, Pause, Play, RotateCw, Settings2, SquareTerminal } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { ScrollableList } from '@/components/ScrollableList'
import { ComposeBuilderDialog } from '@/components/docker/ComposeBuilderDialog'
import { ComposeEditorDialog } from '@/components/docker/ComposeEditorDialog'
import { ComposeTab } from '@/components/docker/ComposeTab'
import { ContainerSettingsDialog } from '@/components/docker/ContainerSettingsDialog'
import { ImagesTab } from '@/components/docker/ImagesTab'
import { VolumesTab } from '@/components/docker/VolumesTab'
import { Pagination } from '@/components/ui/pagination'
import { CreateSiteDialog } from '@/components/docker/CreateSiteDialog'
import { DockerGate, type DockerView } from '@/components/docker/DockerGate'
import { CopyId, Row, RowList, Truncated } from '@/components/docker/DockerRows'
import { LogsDialog } from '@/components/docker/LogsDialog'
import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { createPersistentCache } from '@/lib/cache'
import { asDiagnostic } from '@/lib/diagnostic'
import { ProjectTerminal } from '@/components/Terminal'
import { Dialog } from '@/components/ui/dialog'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { CardDescription, CardTitle } from '@/components/ui/card'
import {
  type Diagnostic,
  type DockerContainer,
  type DockerContainerAction,
  type DockerStatus,
  type TaskEvent,
  runCommand,
} from '@/core'
import { cn } from '@/lib/utils'

/**
 * §166 Docker page (under Sites). Performance rules from the Docker plan §2.5:
 * - Loaded lazily by App, so it adds nothing to the startup bundle.
 * - Data loads when the page opens, after each action, and on Refresh.
 * - The only timer is ONE status poll, and only while Docker is not running: every 1.5 s
 *   (up to 60 s) after "Start Docker", every 7 s otherwise. It pauses while the window is hidden.
 * - The only live listeners are `task-event` while "Start Docker" or a compose action is
 *   being followed.
 */

const START_POLL_MS = 1500
const IDLE_POLL_MS = 7000
const START_TIMEOUT_MS = 60_000

/** Last status seen, so reopening the page shows the right card at once while a fresh check runs. */
let lastStatus: DockerStatus | null = null
/** Containers survive page changes and restarts; a background refresh only re-renders when the data differs. */
const containersCache = createPersistentCache<DockerContainer[] | null>('docker_containers', null)
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

type Tab = 'containers' | 'images' | 'volumes' | 'compose'

type Filter = 'all' | 'running' | 'sites' | 'managed'

const FILTERS: { id: Filter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'running', label: 'Running' },
  { id: 'sites', label: 'Linked to sites' },
  { id: 'managed', label: 'Created by OLS' },
]

const STATE_BADGE: Record<string, { label: string; variant: 'success' | 'secondary' | 'warning' | 'destructive' | 'outline' }> = {
  running: { label: '● Running', variant: 'success' },
  paused: { label: 'Paused', variant: 'warning' },
  restarting: { label: 'Restarting…', variant: 'secondary' },
  exited: { label: 'Stopped', variant: 'secondary' },
  created: { label: 'Created', variant: 'outline' },
  dead: { label: 'Dead', variant: 'destructive' },
}

export default function DockerPage() {
  const [status, setStatus] = useState<DockerStatus | null>(lastStatus)
  const [containers, setContainers] = useState<DockerContainer[]>(containersCache.get() ?? [])
  const [error, setError] = useState<Diagnostic | null>(null)
  const [filter, setFilter] = useState<Filter>('all')
  const [busy, setBusy] = useState<string | null>(null)
  const [startTask, setStartTask] = useState<string | null>(null)
  const [starting, setStarting] = useState(false)
  const [composeOnly, setComposeOnly] = useState(false)
  const [query, setQuery] = useState('')
  // Groups the user toggled by hand (true = open, false = closed). Others open when something runs.
  const [groupToggles, setGroupToggles] = useState<Map<string, boolean>>(new Map())
  const [checking, setChecking] = useState(false)
  const [containersLoaded, setContainersLoaded] = useState(containersCache.get() !== null)
  const [startError, setStartError] = useState<string | null>(null)
  const startedAt = useRef(0)
  const [tab, setTab] = useState<Tab>('containers')
  // Dialogs shared by both tabs.
  const [logsFor, setLogsFor] = useState<string | null>(null)
  const [siteFor, setSiteFor] = useState<DockerContainer | null>(null)
  const [editPath, setEditPath] = useState<string | null>(null)
  const [builderOpen, setBuilderOpen] = useState(false)
  const [taskId, setTaskId] = useState<string | null>(null)
  const [settingsFor, setSettingsFor] = useState<string | null>(null)
  const [shellFor, setShellFor] = useState<string | null>(null)
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)
  const [composeReload, setComposeReload] = useState(0)

  // Callers that refresh on demand show the spinner first; the first load starts with it on.
  const refresh = useCallback(async () => {
    try {
      const s = await runCommand({ type: 'get_docker_status' })
      if (s.type !== 'docker_status') return
      lastStatus = s.status
      setStatus((prev) => (same(prev, s.status) ? prev : s.status))
      if (s.status.running) {
        const r = await runCommand({ type: 'list_docker_containers', all: true })
        if (r.type === 'docker_containers') {
          containersCache.set(r.containers)
          setContainers((prev) => (same(prev, r.containers) ? prev : r.containers))
        }
        setContainersLoaded(true)
      } else {
      }
    } catch (err) {
      setError(asDiagnostic(err))
      setContainersLoaded(true)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // Follow the "Start Docker" task only while it runs, then drop the listener.
  useEffect(() => {
    if (!startTask) return
    let alive = true
    let unlisten: (() => void) | undefined
    void listen<TaskEvent>('task-event', (event) => {
      const task = event.payload.task
      if (!alive || task.id !== startTask || event.payload.kind !== 'finished') return
      setStartTask(null)
      if (task.error) {
        setStarting(false)
        setStartError(task.error.cause || task.error.problem)
      }
      void refresh()
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    return () => {
      alive = false
      unlisten?.()
    }
  }, [startTask, refresh])

  async function startDocker() {
    setError(null)
    setStartError(null)
    startedAt.current = Date.now()
    setStarting(true)
    try {
      const r = await runCommand({ type: 'start_docker_desktop' })
      if (r.type === 'task_started') setStartTask(r.id)
      else void refresh()
    } catch (err) {
      setStarting(false)
      const d = asDiagnostic(err)
      setStartError(d.cause || d.problem)
    }
  }

  /** Manual re-check (Retry / Refresh): keeps the spinner up for at least 600 ms so it is visible. */
  async function recheck() {
    setStartError(null)
    setStarting(false)
    setChecking(true)
    await Promise.all([refresh(), new Promise((r) => setTimeout(r, 600))])
    setChecking(false)
  }

  const engine = status?.state ?? 'stopped'
  const dockerRunning = engine === 'running'
  const view: DockerView = !status
    ? 'checking'
    : dockerRunning
      ? 'running'
      : starting
        ? 'starting'
        : startError
          ? 'error'
          : engine

  // Docker came up: the start is over, and the tabs load their own data when they mount.
  useEffect(() => {
    if (!dockerRunning) return
    setStarting(false)
    setStartError(null)
    setComposeOnly(false)
  }, [dockerRunning])

  // One status poll while Docker is not running. Fast while starting (with a 60 s limit),
  // slow otherwise. Paused while hidden. The effect owns the only interval and clears it.
  const pollMode = !status || dockerRunning ? null : starting ? 'start' : 'idle'
  useEffect(() => {
    if (!pollMode) return
    let inFlight = false
    const tick = () => {
      if (document.hidden || inFlight) return
      if (pollMode === 'start' && Date.now() - startedAt.current > START_TIMEOUT_MS) {
        setStarting(false)
        setStartError('Docker did not start within 60 seconds. It may still be starting, or it needs attention in Docker Desktop (for example WSL 2).')
        return
      }
      inFlight = true
      void refresh().finally(() => {
        inFlight = false
      })
    }
    const id = window.setInterval(tick, pollMode === 'start' ? START_POLL_MS : IDLE_POLL_MS)
    document.addEventListener('visibilitychange', tick)
    return () => {
      window.clearInterval(id)
      document.removeEventListener('visibilitychange', tick)
    }
  }, [pollMode, refresh])

  function showDetails() {
    setError({
      problem: 'Docker is not reachable',
      cause: [startError, status?.message, status?.endpoint ? `Endpoint: ${status.endpoint}` : null].filter(Boolean).join(' · '),
      fix: 'Open Docker Desktop and check for a message (WSL 2, permissions, or a stuck engine), then press Retry.',
    })
  }

  async function act(c: DockerContainer, action: DockerContainerAction) {
    setError(null)
    setBusy(`${c.id}:${action}`)
    try {
      await runCommand({ type: 'docker_container_action', id: c.id, action })
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(null)
      void refresh()
    }
  }

  /** A compose service names its container; the site dialog needs the full container. */
  async function createSiteFor(name: string) {
    try {
      const r = await runCommand({ type: 'list_docker_containers', all: true })
      const c = r.type === 'docker_containers' ? r.containers.find((x) => x.name === name) : undefined
      if (c) setSiteFor(c)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  async function composeCreated(file: string, start: boolean) {
    setComposeReload((n) => n + 1)
    setTab('compose')
    if (!start) return
    try {
      const r = await runCommand({ type: 'docker_compose_action', path: file, action: 'up' })
      if (r.type === 'task_started') setTaskId(r.id)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    return containers.filter((c) => {
      if (filter === 'running' && c.state !== 'running') return false
      if (filter === 'sites' && !c.ols_site) return false
      if (filter === 'managed' && !c.managed) return false
      if (!q) return true
      return [c.name, c.image, c.id, c.compose_project, c.compose_service, c.ols_site, c.status, ...c.ports.map((p) => String(p.host_port ?? ''))]
        .some((v) => v?.toLowerCase().includes(q))
    })
  }, [containers, filter, query])

  // Docker Desktop style: one row per Compose project (collapsed by default), then loose containers.
  // Pages count these rows, so a project is never split across pages.
  const units = useMemo(() => {
    const projects = new Map<string, DockerContainer[]>()
    const loose: DockerContainer[] = []
    for (const c of visible) {
      if (c.compose_project) projects.set(c.compose_project, [...(projects.get(c.compose_project) ?? []), c])
      else loose.push(c)
    }
    return [
      ...[...projects.entries()].map(([project, rows]) => ({ project, rows })),
      ...loose.map((c) => ({ project: '', rows: [c] })),
    ]
  }, [visible])
  const pageCount = Math.max(1, Math.ceil(units.length / perPage))
  const currentPage = Math.min(page, pageCount)
  const pageUnits = useMemo(() => units.slice((currentPage - 1) * perPage, currentPage * perPage), [units, currentPage, perPage])

  const isOpen = (project: string, rows: DockerContainer[]) =>
    groupToggles.get(project) ?? rows.some((c) => c.state === 'running' || c.state === 'paused')
  const projectNames = units.filter((u) => u.project).map((u) => u.project)
  const setAllGroups = (open: boolean) => setGroupToggles(new Map(projectNames.map((n) => [n, open])))

  /** Runs one action on every container of a project that it applies to, in parallel. */
  async function actGroup(project: string, rows: DockerContainer[], action: DockerContainerAction) {
    const targets = rows.filter((c) => {
      const on = c.state === 'running' || c.state === 'paused'
      return action === 'start' ? !on : action === 'stop' ? on : action === 'pause' ? c.state === 'running' : action === 'unpause' ? c.state === 'paused' : true
    })
    if (targets.length === 0) return
    setError(null)
    setBusy(`group:${project}:${action}`)
    const results = await Promise.allSettled(targets.map((c) => runCommand({ type: 'docker_container_action', id: c.id, action })))
    const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected')
    if (failed) setError(asDiagnostic(failed.reason))
    setBusy(null)
    void refresh()
  }

  const runningCount = containers.filter((c) => c.state === 'running').length
  const siteCount = containers.filter((c) => c.ols_site).length

  return (
    <PageLayout
      mode="list"
      resetKey={tab}
      toolbar={
        dockerRunning ? (
          <div className="flex flex-col gap-3">
            <RunningLine status={status} loaded={containersLoaded} runningCount={runningCount} total={containers.length} siteCount={siteCount} />
            <div className="flex gap-1 border-b" role="tablist" aria-label="Docker sections">
                      {(
                        [
                          ['containers', 'Containers'],
                          ['images', 'Images'],
                          ['volumes', 'Volumes'],
                          ['compose', 'Compose'],
                        ] as const
                      ).map(([id, label]) => (
                        <button
                          key={id}
                          type="button"
                          role="tab"
                          aria-selected={tab === id}
                          onClick={() => setTab(id)}
                          className={cn(
                            '-mb-px cursor-pointer border-b-2 px-3 py-2 text-sm font-medium transition-colors',
                            tab === id ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground',
                          )}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
          </div>
        ) : undefined
      }
      header={
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Docker</h1>
        <p className="text-sm text-muted-foreground">Run containers and Compose stacks, edit compose files, and give any container a local site with HTTPS. Docker is optional; OLS never installs it.</p>
      </div>
      }
    >

      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      <div className={cn('flex min-h-0 flex-1 flex-col', !(dockerRunning && (tab === 'containers' || tab === 'images' || tab === 'volumes')) && 'ols-scroll')}>
      {composeOnly && !dockerRunning ? (
        <div className="flex flex-col gap-4">
          <button type="button" className="inline-flex w-fit cursor-pointer items-center gap-1 text-sm text-primary hover:underline" onClick={() => setComposeOnly(false)}>
            <ArrowLeft className="size-4" /> Back to Docker
          </button>
          <ComposeTab
            dockerRunning={false}
            reloadKey={composeReload}
            onRunTask={setTaskId}
            onEdit={setEditPath}
            onNew={() => setBuilderOpen(true)}
            onLogs={setLogsFor}
            onCreateSite={(name) => void createSiteFor(name)}
            onSettings={setSettingsFor}
            onShell={setShellFor}
          />
        </div>
      ) : (
      <DockerGate
        view={view}
        status={status}
        startError={startError}
        canStart={!!status?.desktop_path}
        loading={checking}
        onStart={() => void startDocker()}
        onRetry={() => void recheck()}
        onRefresh={() => void recheck()}
        onDetails={showDetails}
        onCompose={() => setComposeOnly(true)}
      >
      {tab === 'images' && (
        <ImagesTab reloadKey={composeReload} onRunTask={setTaskId} onChanged={() => void refresh()} />
      )}
      {tab === 'volumes' && (
        <VolumesTab reloadKey={composeReload} onRunTask={setTaskId} onChanged={() => void refresh()} />
      )}

      {tab === 'compose' && (
        <ComposeTab
          dockerRunning
          reloadKey={composeReload}
          onRunTask={setTaskId}
          onEdit={setEditPath}
          onNew={() => setBuilderOpen(true)}
          onLogs={setLogsFor}
          onCreateSite={(name) => void createSiteFor(name)}
          onSettings={setSettingsFor}
          onShell={setShellFor}
        />
      )}

      {tab === 'containers' && (
        <ScrollableList
          resetKey={`${filter}:${currentPage}`}
          toolbar={
            <div className="@container border-b">
              <div className="px-4 pt-4">
                <CardTitle className="text-sm">Containers</CardTitle>
                <CardDescription>Grouped by Compose project. Click a port to open it in your browser.</CardDescription>
              </div>
              <div className="flex flex-col gap-2 p-4 @2xl:flex-row @2xl:items-center">
              <div className="relative min-w-0 flex-1">
                <Search className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value)
                    setPage(1)
                  }}
                  placeholder="Search containers"
                  aria-label="Search containers"
                  className="h-8 pl-8"
                />
              </div>
              <div className="flex flex-wrap gap-1" role="tablist" aria-label="Filter containers">
                {FILTERS.map((f) => (
                  <Button
                    key={f.id}
                    size="sm"
                    role="tab"
                    aria-selected={filter === f.id}
                    variant={filter === f.id ? 'secondary' : 'ghost'}
                    className="h-8 cursor-pointer text-xs"
                    onClick={() => {
                      setFilter(f.id)
                      setPage(1)
                    }}
                  >
                    {f.label}
                  </Button>
                ))}
                {projectNames.length > 0 && (
                  <>
                    <Button size="icon" variant="ghost" className="size-8 cursor-pointer" onClick={() => setAllGroups(false)} title="Collapse all groups" aria-label="Collapse all groups">
                      <ChevronsDownUp className="size-4" />
                    </Button>
                    <Button size="icon" variant="ghost" className="size-8 cursor-pointer" onClick={() => setAllGroups(true)} title="Expand all groups" aria-label="Expand all groups">
                      <ChevronsUpDown className="size-4" />
                    </Button>
                  </>
                )}
              </div>
              </div>
            </div>
          }
          footer={
            units.length > 0 ? (
              <Pagination
                label="items"
                total={units.length}
                page={currentPage}
                perPage={perPage}
                onPage={setPage}
                onPerPage={(n) => {
                  setPerPage(n)
                  setPage(1)
                }}
                className="border-t border-border"
              />
            ) : null
          }
        >
            {!containersLoaded ? (
              <p className="flex items-center justify-center gap-2 px-6 py-8 text-sm text-muted-foreground" role="status">
                <Spinner className="size-4" /> Loading containers…
              </p>
            ) : visible.length === 0 ? (
              <p className="px-6 py-8 text-center text-sm text-muted-foreground">
                {containers.length === 0 ? 'No containers yet. Containers you create with Docker show up here.' : 'No containers match this filter.'}
              </p>
            ) : (
              <RowList>
                {pageUnits.map(({ project, rows }) => {
                  const rowEl = (c: DockerContainer) => (
                    <ContainerRow
                      key={c.id}
                      c={c}
                      busy={busy}
                      onAct={act}
                      onError={setError}
                      onLogs={() => setLogsFor(c.name)}
                      onCreateSite={() => setSiteFor(c)}
                      onSettings={() => setSettingsFor(c.id)}
                      onShell={() => setShellFor(c.name)}
                    />
                  )
                  if (!project) return rowEl(rows[0])
                  const open = query.trim() !== '' || isOpen(project, rows)
                  return [
                    <GroupRow
                      key={`group:${project}`}
                      project={project}
                      rows={rows}
                      open={open}
                      busy={busy}
                      onToggle={() => setGroupToggles((prev) => new Map(prev).set(project, !open))}
                      onAct={(action) => void actGroup(project, rows, action)}
                    />,
                    ...(open
                      ? [
                          <div key={`rows:${project}`} className="ml-5 divide-y border-l-2 border-sky-500/40 bg-background/20">
                            {rows.map(rowEl)}
                          </div>,
                        ]
                      : []),
                  ]
                })}
              </RowList>
            )}
        </ScrollableList>
      )}
      </DockerGate>
      )}
      </div>

      <LogsDialog container={logsFor} onClose={() => setLogsFor(null)} />
      {shellFor && (
        <Dialog open onClose={() => setShellFor(null)} size="wide" title={`Shell · ${shellFor}`} description="Runs docker exec inside the container. Closing this window ends the shell.">
          <ProjectTerminal key={shellFor} projectId={null} container={shellFor} />
        </Dialog>
      )}
      {settingsFor && (
        <ContainerSettingsDialog
          key={settingsFor}
          id={settingsFor}
          onClose={() => setSettingsFor(null)}
          onRunTask={setTaskId}
          onEditCompose={setEditPath}
          onChanged={() => {
            setComposeReload((n) => n + 1)
            void refresh()
          }}
        />
      )}
      <CreateSiteDialog container={siteFor} onClose={() => setSiteFor(null)} onCreated={() => void refresh()} />
      {editPath && (
        <ComposeEditorDialog
          key={editPath}
          path={editPath}
          onClose={() => setEditPath(null)}
          onSaved={() => setComposeReload((n) => n + 1)}
        />
      )}
      <ComposeBuilderDialog open={builderOpen} onClose={() => setBuilderOpen(false)} onCreated={(f, start) => void composeCreated(f, start)} />
      <TaskOutputDialog
        taskId={taskId}
        onClose={() => setTaskId(null)}
        onFinished={() => {
          setComposeReload((n) => n + 1)
          void refresh()
        }}
      />
    </PageLayout>
  )
}

function RunningLine({ status, loaded, runningCount, total, siteCount }: { status: DockerStatus | null; loaded: boolean; runningCount: number; total: number; siteCount: number }) {
  return (
    <div className="flex items-center gap-2 text-sm">
      <span className="size-2.5 shrink-0 rounded-full bg-emerald-500" aria-hidden />
      <span className="font-medium">Docker is running{status?.version ? ` · ${status.version}` : ''}</span>
      <span className="truncate text-xs text-muted-foreground" title={status?.endpoint}>
        {loaded ? `${runningCount} running · ${total} total · ${siteCount} linked to sites` : 'Loading containers…'}{status?.os_type ? ` · ${status.os_type} containers` : ''}
      </span>
    </div>
  )
}

function GroupRow({
  project,
  rows,
  open,
  busy,
  onToggle,
  onAct,
}: {
  project: string
  rows: DockerContainer[]
  open: boolean
  busy: string | null
  onToggle: () => void
  onAct: (action: DockerContainerAction) => void
}) {
  const running = rows.filter((c) => c.state === 'running').length
  const paused = rows.filter((c) => c.state === 'paused').length
  const anyOn = running + paused > 0
  const resume = paused > 0 && running === 0
  const groupBusy = busy?.startsWith(`group:${project}:`) ?? false
  const spin = (a: string) => busy === `group:${project}:${a}`
  return (
    <div className="flex items-center justify-between gap-3 bg-muted/40 px-4 py-2.5 hover:bg-muted/50">
      <button type="button" className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 overflow-hidden text-left" aria-expanded={open} onClick={onToggle}>
        {open ? <ChevronDown className="size-4 shrink-0 text-muted-foreground" /> : <ChevronRight className="size-4 shrink-0 text-muted-foreground" />}
        <Layers className="size-4 shrink-0 text-sky-500" aria-hidden />
        <Truncated text={project} className="min-w-0 flex-1 text-sm font-medium" />
      </button>
      <Badge variant={running > 0 ? 'success' : 'secondary'} className="w-28 shrink-0 justify-center whitespace-nowrap">
        {running}/{rows.length} running
      </Badge>
      <span className="flex shrink-0 items-center gap-0.5">
        <ServiceIconAction
          title={anyOn ? `Stop all in ${project}` : `Start all in ${project}`}
          disabled={groupBusy}
          spinning={spin('start') || spin('stop')}
          iconColor={anyOn ? 'text-destructive/70' : 'text-emerald-400'}
          onClick={() => onAct(anyOn ? 'stop' : 'start')}
        >
          {anyOn ? <StopIcon /> : <Play className="size-3.5" />}
        </ServiceIconAction>
        <ServiceIconAction title={`Restart all in ${project}`} disabled={groupBusy} spinning={spin('restart')} iconColor="text-amber-400" onClick={() => onAct('restart')}>
          <RotateCw className="size-3.5" />
        </ServiceIconAction>
        <ServiceIconAction
          title={resume ? `Resume all in ${project}` : `Pause all in ${project}`}
          disabled={groupBusy || !anyOn}
          spinning={spin('pause') || spin('unpause')}
          iconColor="text-sky-400"
          onClick={() => onAct(resume ? 'unpause' : 'pause')}
        >
          {resume ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
        </ServiceIconAction>
      </span>
    </div>
  )
}

function ContainerRow({
  c,
  busy,
  onAct,
  onError,
  onLogs,
  onCreateSite,
  onSettings,
  onShell,
}: {
  c: DockerContainer
  busy: string | null
  onAct: (c: DockerContainer, action: DockerContainerAction) => void
  onError: (d: Diagnostic) => void
  onLogs: () => void
  onCreateSite: () => void
  onSettings: () => void
  onShell: () => void
}) {
  const running = c.state === 'running'
  const paused = c.state === 'paused'
  const rowBusy = busy?.startsWith(`${c.id}:`) ?? false
  const badge = STATE_BADGE[c.state] ?? { label: c.state || 'unknown', variant: 'outline' as const }
  const published = c.ports.filter((p) => p.host_port !== null)
  return (
    <Row
      icon={<Box className="size-4 text-sky-500" aria-hidden />}
      title={c.name}
      subtitle={c.image}
      badge={
        <>
          <CopyId id={c.id} />
          {c.ols_site && <Badge variant="outline">{c.ols_site}</Badge>}
        </>
      }
      ports={
        <>
          {published.length === 0 && <span className="text-xs text-muted-foreground">—</span>}
          {published.map((p) => (
            <button
              key={`${p.container_port}/${p.protocol}/${p.host_port}`}
              type="button"
              disabled={!running || p.protocol !== 'tcp'}
              title={`Container port ${p.container_port}/${p.protocol} → this computer's port ${p.host_port}`}
              className="inline-flex cursor-pointer items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-xs tabular-nums hover:bg-muted disabled:cursor-default disabled:opacity-60"
              onClick={() =>
                runCommand({ type: 'open_url', url: `http://localhost:${p.host_port}` }).catch((e) => onError(asDiagnostic(e)))
              }
            >
              :{p.host_port}
              {running && p.protocol === 'tcp' && <ExternalLink className="size-3" />}
            </button>
          ))}
        </>
      }
      status={
        <>
          <Badge variant={badge.variant}>{badge.label}</Badge>
          <span className="text-xs break-words text-muted-foreground">{c.status}</span>
        </>
      }
      actions={
        <span className="flex items-center justify-end gap-0.5">
          <ServiceIconAction
            title={`${running || paused ? 'Stop' : 'Start'} ${c.name}`}
            disabled={rowBusy}
            spinning={busy === `${c.id}:start` || busy === `${c.id}:stop`}
            iconColor={running || paused ? 'text-destructive/70' : 'text-emerald-400'}
            onClick={() => onAct(c, running || paused ? 'stop' : 'start')}
          >
            {running || paused ? <StopIcon /> : <Play className="size-3.5" />}
          </ServiceIconAction>
          <ServiceIconAction
            title={`Restart ${c.name}`}
            disabled={rowBusy}
            spinning={busy === `${c.id}:restart`}
            iconColor="text-amber-400"
            onClick={() => onAct(c, 'restart')}
          >
            <RotateCw className="size-3.5" />
          </ServiceIconAction>
          <ServiceIconAction
            title={paused ? `Resume ${c.name}` : `Pause ${c.name}`}
            disabled={rowBusy || !(running || paused)}
            spinning={busy === `${c.id}:pause` || busy === `${c.id}:unpause`}
            iconColor="text-sky-400"
            onClick={() => onAct(c, paused ? 'unpause' : 'pause')}
          >
            {paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
          </ServiceIconAction>
          <ServiceIconAction title={`Logs of ${c.name}`} disabled={false} iconColor="text-violet-400" onClick={onLogs}>
            <FileText className="size-3.5" />
          </ServiceIconAction>
          <ServiceIconAction title={running ? `Shell in ${c.name}` : 'Start the container to open a shell'} disabled={!running} iconColor="text-emerald-400" onClick={onShell}>
            <SquareTerminal className="size-3.5" />
          </ServiceIconAction>
          <ServiceIconAction
            title={c.ols_site ? `Already serves ${c.ols_site}` : published.length ? `Create a site for ${c.name}` : 'Publish a port first'}
            disabled={!running || published.length === 0 || !!c.ols_site}
            iconColor="text-sky-400"
            onClick={onCreateSite}
          >
            <Globe className="size-3.5" />
          </ServiceIconAction>
          <ServiceIconAction title={`Settings of ${c.name}`} disabled={false} iconColor="text-slate-400" onClick={onSettings}>
            <Settings2 className="size-3.5" />
          </ServiceIconAction>
        </span>
      }
    />
  )
}
