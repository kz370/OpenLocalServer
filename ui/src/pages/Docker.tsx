import { listen } from '@tauri-apps/api/event'
import { Box, ExternalLink, FileText, Globe, Pause, Play, RefreshCw, RotateCw, Settings2, SquareTerminal } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { ComposeBuilderDialog } from '@/components/docker/ComposeBuilderDialog'
import { ComposeEditorDialog } from '@/components/docker/ComposeEditorDialog'
import { ComposeTab } from '@/components/docker/ComposeTab'
import { ContainerSettingsDialog } from '@/components/docker/ContainerSettingsDialog'
import { ImagesTab } from '@/components/docker/ImagesTab'
import { VolumesTab } from '@/components/docker/VolumesTab'
import { Pagination } from '@/components/ui/pagination'
import { CreateSiteDialog } from '@/components/docker/CreateSiteDialog'
import { CopyId, GroupHeading, Row, RowList } from '@/components/docker/DockerRows'
import { LogsDialog } from '@/components/docker/LogsDialog'
import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { ProjectTerminal } from '@/components/Terminal'
import { Dialog } from '@/components/ui/dialog'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
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
 * - No timers: data loads when the page opens, after each action, and on Refresh.
 * - The only live listeners are `task-event` while "Start Docker" or a compose action is
 *   being followed.
 */

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
  const [status, setStatus] = useState<DockerStatus | null>(null)
  const [containers, setContainers] = useState<DockerContainer[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [filter, setFilter] = useState<Filter>('all')
  const [busy, setBusy] = useState<string | null>(null)
  const [startTask, setStartTask] = useState<string | null>(null)
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
      setStatus(s.status)
      if (s.status.running) {
        const r = await runCommand({ type: 'list_docker_containers', all: true })
        if (r.type === 'docker_containers') setContainers(r.containers)
      } else {
        setContainers([])
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setLoading(false)
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
      if (task.error) setError(task.error)
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
    try {
      const r = await runCommand({ type: 'start_docker_desktop' })
      if (r.type === 'task_started') setStartTask(r.id)
      else void refresh()
    } catch (err) {
      setError(asDiagnostic(err))
    }
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

  const visible = useMemo(
    () =>
      containers.filter((c) =>
        filter === 'running' ? c.state === 'running' : filter === 'sites' ? !!c.ols_site : filter === 'managed' ? c.managed : true,
      ),
    [containers, filter],
  )

  // Compose stacks first (the engine order already puts them first), loose containers last.
  // One page of containers, still grouped by Compose project.
  const pageCount = Math.max(1, Math.ceil(visible.length / perPage))
  const currentPage = Math.min(page, pageCount)
  const pageRows = useMemo(() => visible.slice((currentPage - 1) * perPage, currentPage * perPage), [visible, currentPage, perPage])
  const groups = useMemo(() => {
    const map = new Map<string, DockerContainer[]>()
    for (const c of pageRows) {
      const key = c.compose_project ?? ''
      map.set(key, [...(map.get(key) ?? []), c])
    }
    return [...map.entries()]
  }, [pageRows])

  const runningCount = containers.filter((c) => c.state === 'running').length
  const siteCount = containers.filter((c) => c.ols_site).length

  return (
    <PageLayout
      mode="scroll"
      header={
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Docker</h1>
        <p className="text-sm text-muted-foreground">Run containers and Compose stacks, edit compose files, and give any container a local site with HTTPS. Docker is optional; OLS never installs it.</p>
      </div>
      }
    >

      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      <StatusBar
        status={status}
        loading={loading}
        starting={!!startTask}
        runningCount={runningCount}
        total={containers.length}
        siteCount={siteCount}
        onRefresh={() => {
          setLoading(true)
          void refresh()
        }}
        onStart={() => void startDocker()}
      />

      {status && (status.running || status.installed) && (
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
      )}

      {tab === 'images' && status?.running && (
        <ImagesTab reloadKey={composeReload} onRunTask={setTaskId} onChanged={() => void refresh()} />
      )}
      {tab === 'volumes' && status?.running && (
        <VolumesTab reloadKey={composeReload} onRunTask={setTaskId} onChanged={() => void refresh()} />
      )}

      {tab === 'compose' && status && (status.running || status.installed) && (
        <ComposeTab
          dockerRunning={status.running}
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

      {tab === 'containers' && status?.running && (
        <Card>
          <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3 pb-2">
            <div>
              <CardTitle className="text-sm">Containers</CardTitle>
              <CardDescription>Grouped by Compose project. Click a port to open it in your browser.</CardDescription>
            </div>
            <div className="flex flex-wrap gap-1" role="tablist" aria-label="Filter containers">
              {FILTERS.map((f) => (
                <Button
                  key={f.id}
                  size="sm"
                  role="tab"
                  aria-selected={filter === f.id}
                  variant={filter === f.id ? 'secondary' : 'ghost'}
                  className="h-7 cursor-pointer text-xs"
                  onClick={() => {
                    setFilter(f.id)
                    setPage(1)
                  }}
                >
                  {f.label}
                </Button>
              ))}
            </div>
          </CardHeader>
          <CardContent className="p-0">
            {visible.length === 0 ? (
              <p className="px-6 py-8 text-center text-sm text-muted-foreground">
                {containers.length === 0 ? 'No containers yet. Containers you create with Docker show up here.' : 'No containers match this filter.'}
              </p>
            ) : (
              <RowList>
                {groups.map(([project, rows]) => [
                  project && <GroupHeading key={`group:${project}`}>Compose · {project}</GroupHeading>,
                  ...rows.map((c) => (
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
                  )),
                ])}
              </RowList>
            )}
            {visible.length > 0 && (
              <Pagination
                label="containers"
                total={visible.length}
                page={currentPage}
                perPage={perPage}
                onPage={setPage}
                onPerPage={(n) => {
                  setPerPage(n)
                  setPage(1)
                }}
                className="border-t border-border"
              />
            )}
          </CardContent>
        </Card>
      )}

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

function StatusBar({
  status,
  loading,
  starting,
  runningCount,
  total,
  siteCount,
  onRefresh,
  onStart,
}: {
  status: DockerStatus | null
  loading: boolean
  starting: boolean
  runningCount: number
  total: number
  siteCount: number
  onRefresh: () => void
  onStart: () => void
}) {
  if (!status) {
    return (
      <Card>
        <CardContent className="flex items-center gap-2 py-4 text-sm text-muted-foreground">
          <Spinner className="size-4" /> Checking Docker…
        </CardContent>
      </Card>
    )
  }
  return (
    <Card>
      <CardContent className="flex flex-wrap items-center justify-between gap-3 py-4">
        <div className="flex min-w-0 flex-wrap items-center gap-3">
          <span
            className={cn('size-2.5 shrink-0 rounded-full', status.running ? 'bg-emerald-500' : 'bg-destructive')}
            aria-hidden
          />
          <div className="min-w-0">
            <p className="text-sm font-medium">
              {status.running ? `Docker is running${status.version ? ` · ${status.version}` : ''}` : status.installed ? 'Docker is stopped' : 'Docker is not installed'}
            </p>
            <p className="truncate text-xs text-muted-foreground" title={status.endpoint}>
              {status.running
                ? `${runningCount} running · ${total} total · ${siteCount} linked to sites${status.os_type ? ` · ${status.os_type} containers` : ''}`
                : (status.message ?? status.endpoint)}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {!status.running && status.desktop_path && (
            <Button size="sm" className="cursor-pointer" disabled={starting} onClick={onStart}>
              {starting ? <Spinner className="size-3.5" /> : <Play className="size-3.5" />}
              {starting ? 'Starting Docker…' : 'Start Docker'}
            </Button>
          )}
          {!status.installed && (
            <Button
              size="sm"
              variant="outline"
              className="cursor-pointer"
              onClick={() => void runCommand({ type: 'open_url', url: 'https://www.docker.com/products/docker-desktop/' })}
            >
              Get Docker <ExternalLink className="size-3.5" />
            </Button>
          )}
          <Button size="sm" variant="ghost" className="cursor-pointer" disabled={loading} onClick={onRefresh} title="Refresh">
            <RefreshCw className={cn('size-3.5', loading && 'animate-spin')} />
            Refresh
          </Button>
        </div>
      </CardContent>
    </Card>
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
