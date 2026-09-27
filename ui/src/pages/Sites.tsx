import { open } from '@tauri-apps/plugin-dialog'
import { Activity, Check, Code2, Copy, ExternalLink, FolderMinus, FolderOpen, FolderPlus, FolderSearch, Play, Plus, RefreshCw, Search, Settings2, SquareTerminal, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { GitCloneButton, ImportEnvironmentButton } from '@/components/project/ProjectImports'
import { DomainDialog, newDomain } from '@/components/site/DomainDialog'
import { SiteDialog, type SiteTarget } from '@/components/site/SiteDialog'
import { ApplyReportCard, DriftDialog } from '@/components/site/WebApply'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { ActionMenu, type MenuItem } from '@/components/ui/menu'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type Domain, type DomainSummary, type HealthReport, type Project, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { onOpenProject, takePendingProject } from '@/lib/nav'
import { useWeb } from '@/lib/web'
import { Wizard } from '@/pages/QuickApps'

/** One row of the table: a site, or a registered project that has no site yet. */
interface Row {
  key: string
  site: DomainSummary | null
  project: Project | null
}

type Group = DomainSummary['group'] | 'none'

/** Website types in display order; a type with no sites is not listed. */
const GROUPS: { id: Group; label: string; icon: string }[] = [
  { id: 'php', label: 'PHP', icon: 'php' },
  { id: 'nodejs', label: 'Node.js', icon: 'node' },
  { id: 'python', label: 'Python', icon: 'python' },
  { id: 'static', label: 'Static HTML', icon: 'static' },
  { id: 'proxy', label: 'Reverse proxy', icon: 'proxy' },
  { id: 'none', label: 'No domain', icon: 'static' },
]

const groupOf = (r: Row): Group => r.site?.group ?? 'none'

/** Compact status: coloured dot + short label; full meaning shows on hover. */
function StatusDot({ site, running }: { site: DomainSummary | null; running: boolean }) {
  const [label, color, text] = !site
    ? ['Project only: no domain yet', 'border border-muted-foreground/60 bg-transparent', 'No domain']
    : !site.enabled
      ? ['Disabled', 'bg-warning', 'Disabled']
      : running
        ? ['Running', 'bg-emerald-500 shadow-[0_0_0_3px] shadow-emerald-500/20', site.https ? 'HTTPS' : 'HTTP']
        : ['Stopped: the web server is not running', 'bg-muted-foreground/50', site.https ? 'HTTPS' : 'HTTP']
  return (
    <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground" title={label} aria-label={label} role="img">
      <span className={`size-2.5 shrink-0 rounded-full ${color}`} />
      {text}
    </span>
  )
}

export function SitesPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const web = useWeb()
  const { projects, domains, status, busy, error, setError, run, apply, refresh, refreshProjects, installedPhp } = web

  const [target, setTarget] = useState<SiteTarget | null>(null)
  const [adding, setAdding] = useState<Domain | null>(null)
  const [wizardId, setWizardId] = useState<string | null>(null)
  const [health, setHealth] = useState<HealthReport | null>(null)
  const [dupOf, setDupOf] = useState<string | null>(null)
  const [dupName, setDupName] = useState('')
  const [query, setQuery] = useState('')
  const [group, setGroup] = useState<'all' | Group>('all')
  const [message, setMessage] = useState<string | null>(null)
  const [copiedUrl, setCopiedUrl] = useState<string | null>(null)

  // The command palette and search open a project or a site (and a section) from anywhere.
  useEffect(() => {
    const show = (p: { id: string; tab?: SiteTarget['tab']; site?: string }) => setTarget({ hostname: p.site ?? null, projectId: p.id || null, tab: p.tab })
    const pending = takePendingProject()
    if (pending) show(pending)
    return onOpenProject(show)
  }, [])

  // A project opened by id shows its first site when it has one.
  const shownTarget: SiteTarget | null =
    target && !target.hostname && target.projectId
      ? { ...target, hostname: domains.filter((d) => d.project_id === target.projectId).sort((a, b) => a.hostname.localeCompare(b.hostname))[0]?.hostname ?? null }
      : target

  const rows: Row[] = [
    ...domains.map((d) => ({ key: `site:${d.hostname}`, site: d, project: projects.find((p) => p.id === d.project_id) ?? null })),
    ...projects.filter((p) => !domains.some((d) => d.project_id === p.id)).map((p) => ({ key: `project:${p.id}`, site: null, project: p })),
  ].sort((a, b) => (a.site?.hostname ?? a.project?.name ?? '').localeCompare(b.site?.hostname ?? b.project?.name ?? ''))

  const q = query.trim().toLowerCase()
  const matches = rows.filter((r) => !q || `${r.site?.hostname ?? ''} ${r.project?.name ?? ''} ${r.site?.folder ?? r.project?.path ?? ''}`.toLowerCase().includes(q))
  const groupsPresent = GROUPS.filter((g) => rows.some((r) => groupOf(r) === g.id))
  const activeGroup = group === 'all' || groupsPresent.some((g) => g.id === group) ? group : 'all'
  const visible = activeGroup === 'all' ? matches : matches.filter((r) => groupOf(r) === activeGroup)
  const onSearch = (value: string) => {
    setQuery(value)
    // A search covers every type: if the open tab has no hit, show all tabs' hits instead.
    const term = value.trim().toLowerCase()
    if (term && activeGroup !== 'all' && !rows.some((r) => groupOf(r) === activeGroup && `${r.site?.hostname ?? ''} ${r.project?.name ?? ''}`.toLowerCase().includes(term))) setGroup('all')
  }

  // New projects from Git or an environment file go beside the existing ones by default.
  const projectsParent = projects[0]?.path.replace(/[\\/][^\\/]+$/, '') ?? ''
  async function adopt(p: Project) {
    await refreshProjects()
    setTarget({ hostname: null, projectId: p.id, tab: 'environment' })
  }

  async function addFolder() {
    const picked = await open({ directory: true, title: 'Select a project folder' })
    if (!picked || Array.isArray(picked)) return
    await run('register', async () => {
      const res = await runCommand({ type: 'register_project', path: picked })
      if (res.type === 'project') {
        await refreshProjects()
        // Laragon-style: a folder in the projects folder gets its <name>.test site right away.
        await runCommand({ type: 'sync_auto_domains' }).catch(() => undefined)
        await refresh()
        setTarget({ hostname: null, projectId: res.project.id })
      }
    })
  }

  async function scanFolder() {
    const picked = await open({ directory: true, title: 'Select a folder containing multiple projects' })
    if (!picked || Array.isArray(picked)) return
    await run('scan', async () => {
      const res = await runCommand({ type: 'scan_and_register_projects', path: picked })
      if (res.type === 'projects') {
        await refreshProjects()
        setMessage(res.projects.length === 0 ? 'No project folders found in there.' : `Found and registered ${res.projects.length} project${res.projects.length > 1 ? 's' : ''}.`)
      }
    })
  }

  async function removeProject(p: Project) {
    if (!(await confirmAction(`Remove ${p.name} from OpenLocalServer?\n\nYour project files are not deleted, and its sites stay. It won't be re-added by folder scans — add the folder again to bring it back.`))) return
    await run('remove', async () => {
      await runCommand({ type: 'remove_project', id: p.id })
      await refreshProjects()
    })
  }

  async function saveNew(d: Domain) {
    await runCommand({ type: 'add_domain', domain: d })
    setAdding(null)
    await apply()
  }

  function menuFor({ site: d, project: p }: Row): MenuItem[] {
    const items: MenuItem[] = []
    const folder = d?.folder ?? p?.path
    if (folder) items.push({ label: 'Open project folder', icon: <FolderOpen />, hint: folder, onSelect: () => void run('open', () => runCommand({ type: 'open_path', path: folder })) })
    if (folder) items.push({ label: 'Open folder in code editor', icon: <Code2 />, hint: folder, onSelect: () => void run('code', () => runCommand({ type: 'open_in_editor', path: folder })) })
    if (p) items.push({ label: 'Terminal', icon: <SquareTerminal />, onSelect: () => setTarget({ hostname: d?.hostname ?? null, projectId: p.id, tab: 'terminal' }) })
    if (d) {
      items.push({
        label: 'Health check',
        icon: <Activity />,
        hint: 'DNS → TCP → TLS → certificate → trust → HTTP',
        onSelect: () =>
          void run('health', async () => {
            const r = await runCommand({ type: 'health_check', hostname: d.hostname })
            if (r.type === 'health') setHealth(r.report)
          }),
      })
      if (d.has_app) items.push({ label: 'Restart app process', icon: <RefreshCw />, onSelect: () => void run('restart', () => runCommand({ type: 'restart_site_app', hostname: d.hostname })) })
      items.push(
        'separator',
        {
          label: d.enabled ? 'Disable' : 'Enable',
          icon: d.enabled ? <StopIcon /> : <Play />,
          disabled: busy === `toggle:${d.hostname}`,
          onSelect: () =>
            void run(`toggle:${d.hostname}`, async () => {
              await runCommand({ type: 'set_domain_enabled', hostname: d.hostname, enabled: !d.enabled })
              await apply()
            }),
        },
        {
          label: 'Duplicate',
          icon: <Copy />,
          onSelect: () => {
            setDupOf(d.hostname)
            setDupName(`copy.${d.hostname}`)
          },
        },
      )
    } else if (p) {
      items.push({ label: 'Add a domain', icon: <Plus />, onSelect: () => setTarget({ hostname: null, projectId: p.id, tab: 'settings' }) })
    }
    items.push('separator')
    if (d)
      items.push({
        label: 'Delete site',
        icon: <Trash2 />,
        danger: true,
        onSelect: async () => {
          if (!(await confirmAction(`Delete ${d.hostname}? Its certificate is revoked and its config removed. The project folder is not touched.`))) return
          void run('delete', async () => {
            await runCommand({ type: 'remove_domain', hostname: d.hostname })
            await apply()
          })
        },
      })
    if (p) items.push({ label: 'Remove project from the list', icon: <FolderMinus />, danger: !d, onSelect: () => void removeProject(p) })
    return items
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Sites</h1>
          <p className="text-sm text-muted-foreground">Every site and the project behind it. Open a site's settings for its domain, web config, environment, terminal, Git and more.</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {status && !status.running && domains.length > 0 && (
            <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => run('apply', () => apply())} title="Sites can't be opened while the web server is stopped">
              {busy === 'apply' ? <Spinner /> : <Play />} Start web server
            </Button>
          )}
          <Button size="sm" onClick={() => setAdding(newDomain())}>
            <Plus /> Add site
          </Button>
          <Button size="sm" variant="secondary" onClick={addFolder} title="Pick one project folder">
            <FolderPlus /> Add a folder
          </Button>
          <Button size="sm" variant="secondary" onClick={scanFolder} title="Pick a workspace folder holding several projects side by side">
            <FolderSearch /> Scan a folder
          </Button>
          <GitCloneButton defaultParent={projectsParent} onDone={adopt} />
          <ImportEnvironmentButton defaultParent={projectsParent} onDone={adopt} />
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {message && (
        <p className="text-sm text-muted-foreground">
          {message}{' '}
          <button className="underline" onClick={() => setMessage(null)}>
            Dismiss
          </button>
        </p>
      )}
      <ApplyReportCard web={web} />

      <Card>
        <CardContent className="p-0">
          <div className="flex flex-col gap-2 border-b border-border px-4 pb-2 pt-3">
            <div className="relative max-w-sm">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input className="h-8 pl-9" value={query} onChange={(e) => onSearch(e.target.value)} placeholder="Search sites by domain, project or folder" />
            </div>
            <Tabs
              tabs={[
                { id: 'all' as 'all' | Group, label: 'All', badge: matches.length },
                ...groupsPresent.map((g) => ({
                  id: g.id as 'all' | Group,
                  label: g.label,
                  badge: matches.filter((r) => groupOf(r) === g.id).length,
                  icon: <TechIcon id={g.icon} className="size-3.5" />,
                })),
              ]}
              value={activeGroup}
              onChange={setGroup}
            />
          </div>
          <Table wrapperClassName="rounded-b-lg">
            <TableHeader>
              <TableRow>
                <TableHead>Domain</TableHead>
                <TableHead className="whitespace-nowrap">Status</TableHead>
                <TableHead className="w-24 whitespace-nowrap text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map((r) => {
                const { site: d, project: p } = r
                const openSettings = () => setTarget({ hostname: d?.hostname ?? null, projectId: p?.id ?? null })
                const folder = d?.folder ?? p?.path
                return (
                  <TableRow key={r.key}>
                    <TableCell className="min-w-0">
                      <div className="flex min-w-0 items-center gap-1">
                        <button className="min-w-0 flex-1 cursor-pointer truncate text-left font-medium hover:underline" onClick={openSettings} title={d?.hostname ?? p?.name}>
                          {d?.hostname ?? p?.name}
                        </button>
                        {d && (
                          <>
                            <Button
                              size="sm"
                              variant="ghost"
                              className="h-7 w-7 shrink-0 cursor-pointer px-0"
                              title={!d.enabled ? 'The site is disabled' : !status?.running ? 'Start the web server first' : `Open ${d.hostname}`}
                              aria-label={`Open ${d.hostname}`}
                              disabled={!d.enabled || !status?.running}
                              onClick={() => run('open', () => runCommand({ type: 'open_url', url: d.url }))}
                            >
                              <ExternalLink className="size-3.5" />
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              className="h-7 w-7 shrink-0 cursor-pointer px-0"
                              title={copiedUrl === d.url ? 'Copied' : 'Copy domain'}
                              aria-label={copiedUrl === d.url ? 'Domain copied' : `Copy ${d.hostname}`}
                              onClick={async () => {
                                await navigator.clipboard.writeText(d.hostname)
                                setCopiedUrl(d.url)
                                window.setTimeout(() => setCopiedUrl((current) => (current === d.url ? null : current)), 1500)
                              }}
                            >
                              {copiedUrl === d.url ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                            </Button>
                          </>
                        )}
                      </div>
                      <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                        {d ? (
                          <>
                            <TechIcon id={GROUPS.find((g) => g.id === d.group)?.icon ?? 'static'} className="size-3.5 shrink-0" />
                            <span className="shrink-0">
                              {d.kind}
                              {d.has_app && ' + app'}
                            </span>
                            {d && p && p.name !== d.hostname && <span className="shrink-0">{p.name}</span>}
                            <span className="min-w-0 flex-1 truncate" title={folder}>
                              {folder}
                            </span>
                          </>
                        ) : (
                          <span className="truncate" title={folder}>
                            no domain yet · {folder}
                          </span>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <StatusDot site={d} running={!!status?.running} />
                      {d?.public_domain && (
                        <div className="mt-0.5 text-[11px] text-warning" title={`Public through Cloudflare: ${d.public_domain}`}>
                          Public
                        </div>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center justify-end gap-1">
                        <Button size="sm" variant="ghost" className="h-8 w-8 cursor-pointer px-0" onClick={openSettings} title="Settings" aria-label={`Settings for ${d?.hostname ?? p?.name}`}>
                          <Settings2 className="size-3.5" />
                        </Button>
                        <ActionMenu label={`More actions for ${d?.hostname ?? p?.name}`} items={menuFor(r)} />
                      </div>
                    </TableCell>
                  </TableRow>
                )
              })}
              {visible.length === 0 && (
                <TableRow>
                  <TableCell colSpan={3} className="text-center text-sm text-muted-foreground">
                    {rows.length === 0 ? 'No sites yet. Add one, add a project folder, or create one from Quick Apps.' : q ? `No site matches "${query.trim()}".` : 'No sites of this type.'}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {shownTarget && <SiteDialog key={`${shownTarget.hostname}:${shownTarget.projectId}`} target={shownTarget} web={web} onClose={() => setTarget(null)} onSaved={(hostname) => setTarget((t) => (t ? { ...t, hostname, tab: 'settings' } : t))} />}

      <DomainDialog
        projects={projects}
        domain={adding}
        installedPhp={installedPhp}
        onQuickApp={(id) => {
          setAdding(null)
          setWizardId(id)
        }}
        onClose={() => setAdding(null)}
        onSave={(d) => run('save', () => saveNew(d))}
        busy={busy !== null}
      />

      {wizardId && (
        <Wizard
          id={wizardId}
          onClose={() => {
            setWizardId(null)
            void refresh()
            void refreshProjects()
          }}
          onNavigate={onNavigate}
        />
      )}

      <DriftDialog web={web} />

      <Dialog
        open={!!health}
        onClose={() => setHealth(null)}
        title={`Health: ${health?.hostname ?? ''}`}
        description={health?.ok ? 'Every link in the chain works.' : 'Something in the chain is broken. The first failing step is the one to fix.'}
      >
        <div className="flex flex-col gap-2">
          {health?.steps.map((s) => (
            <div key={s.name} className="flex items-start gap-2 text-sm">
              <span className={s.skipped ? 'text-muted-foreground' : s.ok ? 'text-success' : 'text-destructive'}>{s.skipped ? '–' : s.ok ? '✓' : '✗'}</span>
              <div>
                <span className="font-medium">{s.name}</span>
                <div className="text-xs text-muted-foreground">{s.detail}</div>
              </div>
            </div>
          ))}
        </div>
      </Dialog>

      <Dialog
        open={dupOf !== null}
        onClose={() => setDupOf(null)}
        title={`Duplicate ${dupOf ?? ''}`}
        description="The copy gets the same settings, but its own hostname (and no app process)."
        footer={
          <>
            <Button variant="ghost" onClick={() => setDupOf(null)}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null}
              onClick={() =>
                run('dup', async () => {
                  await runCommand({ type: 'duplicate_domain', hostname: dupOf!, new_hostname: dupName })
                  setDupOf(null)
                  await apply()
                })
              }
            >
              Duplicate
            </Button>
          </>
        }
      >
        <Field label="New domain">
          <Input value={dupName} onChange={(e) => setDupName(e.target.value)} />
        </Field>
      </Dialog>
    </div>
  )
}
