import { listen } from '@tauri-apps/api/event'
import {
  Bug,
  Camera,
  FileCode2,
  FileKey2,
  FolderKanban,
  GitBranch,
  Globe,
  Layers,
  LayoutDashboard,
  ListRestart,
  Mail,
  Play,
  ScrollText,
  ServerCog,
  Settings2,
  SquareTerminal,
  Wrench,
  X,
  Zap,
} from 'lucide-react'
import { type ReactNode, useEffect, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { ProjectShortcuts } from '@/components/OpenWithMenu'
import { ProjectTools, TOOL_TABS, type ToolTab } from '@/components/ProjectTools'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { type CoreCommand, type Domain, type ProcessEvent, type ProjectDetail, runCommand } from '@/core'
import { useAnimatedClose } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import type { Web } from '@/lib/web'
import { ProjectCommands } from '@/components/project/ProjectCommands'
import { SiteConfigTab } from '@/pages/Config'
import { HtaccessEditor } from './HtaccessEditor'
import { DomainSettings, newDomain } from './DomainDialog'
import { ServersPanel } from './ServersPanel'
import { SiteLogs } from './SiteLogs'

export type SiteTab = 'settings' | 'config' | 'htaccess' | 'servers' | 'logs' | 'overview' | 'commands' | ToolTab

const TAB_ICON: Record<Exclude<SiteTab, 'settings' | 'config' | 'htaccess' | 'servers' | 'logs'>, ReactNode> = {
  overview: <LayoutDashboard />,
  environment: <Layers />,
  terminal: <SquareTerminal />,
  commands: <Zap />,
  env: <FileKey2 />,
  git: <GitBranch />,
  workers: <ListRestart />,
  snapshots: <Camera />,
  repair: <Wrench />,
  mail: <Mail />,
  composer: <TechIcon id="composer" />,
  node: <TechIcon id="node" />,
  python: <TechIcon id="python" />,
  xdebug: <Bug />,
  load: <TechIcon id="k6" />,
}

/** Tabs that belong to the project, not the site. */
const SITE_TABS: SiteTab[] = ['settings', 'config', 'htaccess', 'servers', 'logs']

/** What the site dialog shows: a site, a project without a site yet, or a site and its project. */
export interface SiteTarget {
  hostname: string | null
  projectId: string | null
  tab?: SiteTab
}

const SOURCE_LABEL: Record<string, string> = {
  manifest: 'from .openlocalserver/environment.yaml',
  detected: 'detected from project files',
  global: 'global default',
  none: 'not resolved',
}

/**
 * aaPanel-style site settings: one large dialog with a side menu. The site's own settings and
 * web server config come first, then everything the project behind it offers.
 */
export function SiteDialog({ target, web, onClose: closeNow, onSaved }: { target: SiteTarget; web: Web; onClose: () => void; onSaved: (hostname: string) => void }) {
  const { state, close: onClose } = useAnimatedClose(closeNow)
  const { projects, installedPhp, run, apply, busy, error, setError } = web
  const [tab, setTab] = useState<SiteTab>(target.tab ?? (target.hostname ? 'settings' : 'overview'))
  const [domain, setDomain] = useState<Domain | null>(null)
  const [detail, setDetail] = useState<ProjectDetail | null>(null)
  const [runOutput, setRunOutput] = useState<string[]>([])
  const [runningProcessId, setRunningProcessId] = useState<number | null>(null)
  const [toolsTick, setToolsTick] = useState(0)

  const projectId = domain?.project_id ?? target.projectId
  const project = projects.find((p) => p.id === projectId) ?? null

  // Re-read the site whenever its settings are shown: the config tab can change ownership and blocks.
  // Two rules keep this from wiping unsaved edits in the settings form, which is seeded from
  // `domain`:
  //   * one fetch per trigger — it used to run twice (once on mount, once for the tab), each
  //     producing a fresh object, so the form reset twice before you could type;
  //   * an unchanged read is dropped, so a tab bounce leaves the object identity alone.
  const loadedFor = useRef<string | null>(null)
  useEffect(() => {
    if (!target.hostname) return
    if (tab !== 'settings' && loadedFor.current === target.hostname) return
    let current = true
    void runCommand({ type: 'get_domain', hostname: target.hostname }).then((r) => {
      if (!current || r.type !== 'domain') return
      loadedFor.current = target.hostname
      setDomain((cur) => (cur && JSON.stringify(cur) === JSON.stringify(r.domain) ? cur : r.domain))
    })
    return () => {
      current = false
    }
  }, [target.hostname, tab])

  useEffect(() => {
    setDetail(null)
    if (!projectId) return
    let current = true
    runCommand({ type: 'get_project_detail', id: projectId }).then((res) => {
      if (current && res.type === 'project_detail') setDetail(res.detail)
    })
    return () => {
      current = false
    }
  }, [projectId])

  useEffect(() => {
    const unlisten = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (runningProcessId === null || e.id !== runningProcessId) return
      if (e.kind === 'output') setRunOutput((prev) => [...prev.slice(-199), e.line])
      // The tools re-read the project's files once their command has finished.
      if (e.kind === 'state_changed' && ['stopped', 'crashed', 'failed'].includes(e.state)) setToolsTick((n) => n + 1)
    })
    return () => {
      unlisten.then((f) => f())
    }
  }, [runningProcessId])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  /** Runs a command that starts a process and follows its output in the dialog. */
  async function startProcess(command: CoreCommand) {
    setRunOutput([])
    const res = await runCommand(command)
    if (res.type === 'process_started') setRunningProcessId(res.id)
  }

  async function save(d: Domain) {
    d = { ...d, public_domain: d.public_domain?.trim().toLowerCase() || null }
    const old = target.hostname
    if (old && old !== d.hostname) await runCommand({ type: 'rename_domain', hostname: old, new_hostname: d.hostname })
    await runCommand({ type: old ? 'update_domain' : 'add_domain', domain: d })
    if (d.public_domain && d.tunnel_id) {
      const listed = await runCommand({ type: 'list_tunnels' })
      const existing = listed.type === 'tunnels' ? listed.tunnels.find((t) => t.config.id === d.tunnel_id)?.config : undefined
      if (existing) await runCommand({ type: 'save_tunnel', tunnel: { ...existing, target: `http://${d.hostname}`, public_hostname: d.public_domain, autostart: true } })
    }
    if (domain?.tunnel_id && domain.tunnel_id !== d.tunnel_id) {
      const listed = await runCommand({ type: 'list_tunnels' })
      const previous = listed.type === 'tunnels' ? listed.tunnels.find((t) => t.config.id === domain.tunnel_id)?.config : undefined
      if (previous) {
        await runCommand({ type: 'stop_tunnel', id: previous.id })
        await runCommand({ type: 'save_tunnel', tunnel: { ...previous, public_hostname: null, autostart: false } })
      }
    }
    await apply()
    onSaved(d.hostname)
  }

  const siteItems: { id: SiteTab; label: string; icon: ReactNode }[] = [
    { id: 'settings', label: target.hostname ? 'Site settings' : 'Add a domain', icon: <Settings2 /> },
    ...(target.hostname ? [{ id: 'config' as SiteTab, label: 'Web server config', icon: <FileCode2 /> }] : []),
    ...(target.hostname && domain?.kind.type === 'php' ? [{ id: 'htaccess' as SiteTab, label: '.htaccess', icon: <FileCode2 /> }] : []),
    { id: 'servers', label: 'Servers', icon: <ServerCog /> },
    { id: 'logs', label: 'Logs & issues', icon: <ScrollText /> },
  ]
  // Commands sit right after the terminal: both run things in the project.
  const toolItems = TOOL_TABS.flatMap((t) => (t.id === 'terminal' ? [t, { id: 'commands' as SiteTab, label: 'Commands' }] : [t]))
  // Tooling (mail, runtimes, testing) lives under its own heading: it is not project setup.
  const PROJECT_TOOL_IDS: SiteTab[] = ['environment', 'terminal', 'commands', 'env', 'git', 'workers', 'snapshots', 'repair']
  const DEV_TOOL_IDS: SiteTab[] = ['mail', 'composer', 'node', 'python', 'xdebug', 'load']
  const projectItems: { id: SiteTab; label: string }[] = projectId
    ? [{ id: 'overview', label: 'Overview' }, ...toolItems.filter((t) => PROJECT_TOOL_IDS.includes(t.id as SiteTab))]
    : []
  const devItems: { id: SiteTab; label: string }[] = projectId ? toolItems.filter((t) => DEV_TOOL_IDS.includes(t.id as SiteTab)) : []
  const isProjectTab = !SITE_TABS.includes(tab)

  const title = target.hostname ?? project?.name ?? 'Site'
  const settingsDomain = target.hostname ? domain : { ...newDomain(), project_id: projectId }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div data-state={state} className="modal-backdrop absolute inset-0 bg-black/50" onClick={onClose} />
      <div data-state={state} role="dialog" aria-modal="true" aria-label={`Settings for ${title}`} className="modal-panel relative flex h-[88vh] w-full max-w-6xl flex-col rounded-xl border border-border bg-card text-card-foreground shadow-2xl">
        <div className="flex items-center justify-between gap-4 border-b border-border px-5 py-3">
          <div className="flex min-w-0 items-center gap-2">
            <Globe className="size-4 shrink-0 text-muted-foreground" />
            <h2 className="truncate text-base font-semibold">{title}</h2>
            {project && target.hostname && <span className="truncate text-sm text-muted-foreground">· {project.name}</span>}
            {project && <span className="hidden truncate text-xs text-muted-foreground md:inline">{project.path}</span>}
          </div>
          <button onClick={onClose} className="rounded-md p-1 text-muted-foreground hover:bg-accent" aria-label="Close">
            <X className="size-4" />
          </button>
        </div>

        <div className="flex min-h-0 flex-1">
          <nav className="flex w-52 shrink-0 flex-col overflow-y-auto border-r border-border px-0 py-2" aria-label="Site settings sections">
            <NavHeading>Site</NavHeading>
            <div className="flex flex-col gap-[3px]">
              {siteItems.map((i) => (
                <NavItem key={i.id} active={tab === i.id} onClick={() => setTab(i.id)}>
                  {i.icon}
                  {i.label}
                </NavItem>
              ))}
            </div>
            {projectItems.length > 0 && (
              <>
                <NavHeading>
                  <FolderKanban className="size-3" /> Project
                </NavHeading>
                <div className="flex flex-col gap-[3px]">
                  {projectItems.map((i) => (
                    <NavItem key={i.id} active={tab === i.id} onClick={() => setTab(i.id)}>
                      {TAB_ICON[i.id as keyof typeof TAB_ICON]}
                      {i.label}
                    </NavItem>
                  ))}
                </div>
              </>
            )}
            {devItems.length > 0 && (
              <>
                <NavHeading>Tools</NavHeading>
                <div className="flex flex-col gap-[3px]">
                  {devItems.map((i) => (
                    <NavItem key={i.id} active={tab === i.id} onClick={() => setTab(i.id)}>
                      {TAB_ICON[i.id as keyof typeof TAB_ICON]}
                      {i.label}
                    </NavItem>
                  ))}
                </div>
              </>
            )}
          </nav>

          <div className="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
            <ErrorCard error={error} onDismiss={() => setError(null)} />
            {tab === 'settings' &&
              (settingsDomain ? (
                <DomainSettings
                  key={target.hostname ?? 'new'}
                  projects={projects}
                  domain={settingsDomain}
                  isNew={!target.hostname}
                  installedPhp={installedPhp}
                  onSave={(d) => run('save', () => save(d))}
                  busy={busy !== null}
                  actions={(submit, b) => (
                    <Button disabled={b} onClick={submit}>
                      {target.hostname ? 'Save and apply' : 'Add and apply'}
                    </Button>
                  )}
                />
              ) : (
                <p className="text-sm text-muted-foreground">Reading the site…</p>
              ))}

            {tab === 'config' && target.hostname && <SiteConfigTab hostname={target.hostname} />}
            {tab === 'htaccess' && target.hostname && <HtaccessEditor hostname={target.hostname} />}
            {tab === 'servers' && <ServersPanel web={web} hostname={target.hostname} />}
            {tab === 'logs' && <SiteLogs hostname={target.hostname} projectId={projectId} projectName={project?.name ?? null} />}

            {isProjectTab && !detail && <p className="text-sm text-muted-foreground">Reading the project…</p>}
            {tab === 'overview' && detail && <ProjectOverview detail={detail} onRun={startProcess} />}
            {tab === 'commands' && detail && <ProjectCommands key={detail.project.id} projectId={detail.project.id} framework={detail.detection.framework} onShowIssues={() => setTab('logs')} />}
            {isProjectTab && tab !== 'overview' && tab !== 'commands' && detail && (
              <ProjectTools key={`${detail.project.id}:${tab}`} detail={detail} start={startProcess} refreshKey={toolsTick} tab={tab as ToolTab} />
            )}

            {runOutput.length > 0 && isProjectTab && tab !== 'commands' && (
              <pre className="max-h-48 shrink-0 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">
                {runOutput.join('\n')}
              </pre>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function NavHeading({ children }: { children: ReactNode }) {
  return <div className="mb-2 mt-5 flex items-center gap-1 px-4 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground/70 first:mt-1">{children}</div>
}

function NavItem({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'flex items-center gap-2 px-4 py-1.5 text-left text-sm transition-colors [&_svg]:size-4 [&_svg]:shrink-0 [&>span]:size-4',
        active
          ? 'bg-accent font-medium text-accent-foreground'
          : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
      )}
    >
      {children}
    </button>
  )
}

/** Framework, resolved runtime versions and file shortcuts for the project behind a site. */
function ProjectOverview({ detail, onRun }: { detail: ProjectDetail; onRun: (c: CoreCommand) => Promise<void> }) {
  const shown = detail.resolved.filter((r) => r.requested_version || r.installed_version)
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge>{detail.detection.framework.replace(/_/g, ' ')}</Badge>
        {detail.detection.markers.map((m) => (
          <Badge key={m} variant="secondary">
            {m}
          </Badge>
        ))}
      </div>

      <div className="flex flex-col gap-2">
        {shown.map((r) => (
          <div key={r.id} className="flex items-center justify-between rounded-md border border-border px-3 py-2 text-sm">
            <div className="flex flex-col">
              <span className="font-medium uppercase">{r.id}</span>
              <span className="text-xs text-muted-foreground">
                wants {r.requested_version ?? '?'} · {SOURCE_LABEL[r.source]}
              </span>
            </div>
            {r.installed_version ? (
              <div className="flex items-center gap-2">
                <Badge variant="success">{r.installed_version} installed</Badge>
                <Button size="sm" variant="secondary" onClick={() => onRun({ type: 'run_in_project', project_id: detail.project.id, runtime_id: r.id, args: ['--version'] })}>
                  <Play /> --version
                </Button>
              </div>
            ) : (
              <Badge variant="destructive">not installed</Badge>
            )}
          </div>
        ))}
        {shown.length === 0 && <p className="text-sm text-muted-foreground">No runtime requirement detected and no manifest present.</p>}
      </div>

      <ProjectShortcuts key={detail.project.id} projectId={detail.project.id} />
    </div>
  )
}
