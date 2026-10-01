import { open } from '@tauri-apps/plugin-dialog'
import { Activity, Check, Code2, Copy, Download, ExternalLink, FolderMinus, FolderOpen, FolderPlus, FolderSearch, Globe, KeyRound, Play, Plus, RefreshCw, Search, Settings2, SquareTerminal, Trash2, X } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { GitCloneButton, ImportEnvironmentButton } from '@/components/project/ProjectImports'
import { DomainDialog, newDomain } from '@/components/site/DomainDialog'
import { SiteExportDialog, SiteImportButton } from '@/components/site/SiteBundle'
import { SiteDialog, type SiteTarget } from '@/components/site/SiteDialog'
import { SaveButton } from '@/components/SaveButton'
import { ApplyReportCard, DriftDialog } from '@/components/site/WebApply'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { ActionMenu, type MenuItem } from '@/components/ui/menu'
import { Pagination } from '@/components/ui/pagination'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type Domain, type DomainSummary, type HealthReport, type Project, type WpProject, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { onOpenProject, takePendingProject } from '@/lib/nav'
import { useTheme } from '@/lib/theme'
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
/** The `localhost/<prefix>` URL for a site that asked for one (§55). The port is the one
 * its own server binds, so a site pinned to a non-default server is reachable there. */
function localhostUrl(site: DomainSummary): string {
  return `http://localhost:${site.http_port ?? 80}/${site.path_prefix}/`
}

function StatusDot({ site, running }: { site: DomainSummary | null; running: boolean }) {  const [label, color, text] = !site
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
  // The WordPress sign-in screen lives outside this window, so it is told which colourway
  // the app is showing rather than left to guess from the browser.
  const { resolvedTheme } = useTheme()
  const { projects, domains, status, busy, error, setError, run, apply, refresh, refreshProjects, installedPhp, removing, applying } = web
  const defaultServer = status?.default_server

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
  // Bulk selection. Only site rows are picked — a project with no domain yet has nothing
  // to enable, pin to a web server or delete, so it gets no checkbox.
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set())
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)
  const [bulkServerChoice, setBulkServerChoice] = useState('')
  /** Which sites the export dialog is open for: one row, or the whole selection. */
  const [exportFor, setExportFor] = useState<string[] | null>(null)
  // WordPress projects, straight from the folders on disk: this is what decides which rows
  // get a "WP Admin" action. Re-read when the project list changes, so a folder added or
  // deleted elsewhere gains or loses the action without a reload.
  const [wpProjects, setWpProjects] = useState<WpProject[]>([])

  const refreshWpProjects = async () => {
    const r = await runCommand({ type: 'list_wp_projects' })
    if (r.type === 'wp_projects') setWpProjects(r.projects)
  }

  useEffect(() => {
    void refreshWpProjects().catch(() => undefined)
  }, [projects])

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
    // openlocalserver.test is built in: served and configurable, but never listed, so it can't be deleted.
    ...domains.filter((d) => d.hostname !== 'openlocalserver.test').map((d) => ({ key: `site:${d.hostname}`, site: d, project: projects.find((p) => p.id === d.project_id) ?? null })),
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

  // ---- bulk selection and paging -------------------------------------------------
  // Every row is picked, site or not: a folder that has no domain yet is exactly the row
  // a bulk edit is wanted on, and a checkbox that only appears once the site exists would
  // be useless for it. A batch that needs a site creates the automatic one first.
  const visibleKeys = visible.map((r) => r.key)
  // A row the search or a delete removed must not stay picked: the next batch would name a
  // site that is not there, and the backend would report it as skipped for no visible
  // reason. The selection is therefore derived from the rows on screen rather than read off
  // the state, so it can never outlive the rows it was made on. A key left behind in the
  // state is inert: it is in no count, no button and no batch.
  const pickedKeys = [...picked].filter((k) => visibleKeys.includes(k)).sort()
  const pickedRows = pickedKeys.map((k) => rows.find((r) => r.key === k)).filter((r): r is Row => !!r)

  const pages = Math.max(1, Math.ceil(visible.length / perPage))
  // Filtering, a delete or a smaller page size can leave the reader past the last page,
  // which would draw an empty table with no explanation. The page is clamped on read, not
  // corrected in an effect, so the table is right on the very first frame after the change.
  const currentPage = Math.min(page, pages)
  const pageRows = visible.slice((currentPage - 1) * perPage, currentPage * perPage)
  const pagePicked = pageRows.filter((r) => picked.has(r.key)).length
  const allOnPagePicked = pageRows.length > 0 && pagePicked === pageRows.length
  /** The header checkbox covers the page, not the whole filtered list, so offer the rest. */
  const restCount = visibleKeys.length - pagePicked

  const toggle = (key: string, on: boolean) =>
    setPicked((current) => {
      const next = new Set(current)
      if (on) next.add(key)
      else next.delete(key)
      return next
    })

  const pickedCount = pickedKeys.length
  /** Only site rows have a hostname, and a hostname is what a bundle is keyed by. */
  const pickedHostnames = pickedRows.map((r) => r.site?.hostname).filter((h): h is string => !!h)
  /** Rows that have no site yet: the only ones "Add auto domain" can do anything to. */
  const noDomainCount = pickedRows.filter((r) => !r.site).length
  // Enable and Disable are each hidden when they would change nothing: a disabled button
  // beside an enabled twin is a hint, an enabled button that does nothing is a lie. A row
  // with no site counts as off, because the batch is about to give it one.
  const enabledCount = pickedRows.filter((r) => r.site?.enabled).length
  const allEnabled = pickedCount > 0 && enabledCount === pickedCount
  const noneEnabled = enabledCount === 0
  const busyBulk = busy === 'bulk'
  /** The web servers a bulk pin can move sites to, default first so its tile names the
   *  right one — the same ordering the per-site dialog uses. */
  const serverChoices = [...(status?.servers ?? [])].sort((a, b) => Number(b.active) - Number(a.active))

  /**
   * The hostnames a bulk action works on. A project with no domain yet gets its automatic
   * one first — the same `<folder>.<tld>` the folder watcher creates for a folder dropped
   * into a projects folder or added from the Explorer's right-click menu — so waiting for
   * the watcher is never required. A folder the watcher also refuses (nothing to serve in
   * it) comes back in `noSite` and is reported, never silently dropped.
   */
  async function resolveSites(): Promise<{ hostnames: string[]; noSite: string[] }> {
    if (pickedRows.every((r) => r.site)) {
      return { hostnames: pickedRows.map((r) => r.site!.hostname), noSite: [] }
    }
    await runCommand({ type: 'sync_auto_domains' })
    const res = await runCommand({ type: 'list_domains' })
    const known = res.type === 'domains' ? res.domains : []
    const hostnames: string[] = []
    const noSite: string[] = []
    for (const r of pickedRows) {
      const host = r.site?.hostname ?? known.find((d) => d.project_id === r.project?.id)?.hostname
      if (host) hostnames.push(host)
      else noSite.push(r.project?.name ?? r.key)
    }
    // Those rows are site rows now, so the selection follows them onto their new keys
    // instead of vanishing when the project-only row they were made on disappears.
    setPicked(new Set(hostnames.map((h) => `site:${h}`)))
    return { hostnames, noSite }
  }

  /** One batch, one message: how many changed, and every site the backend or the folder
   *  itself refused, so a partial edit is reported instead of looking complete. */
  function reportBulk(label: string, changed: number, skipped: string[]) {
    setMessage(
      skipped.length === 0
        ? `${label}: ${changed} site${changed === 1 ? '' : 's'} changed.`
        : `${label}: ${changed} site${changed === 1 ? '' : 's'} changed, ${skipped.length} left alone — ${skipped.join('; ')}.`,
    )
  }



  // New projects default to <install>/sites (or quickapps.projects_dir override).
  // User can change folder name or location in each creation dialog.
  const [projectsParent, setProjectsParent] = useState('')
  useEffect(() => {
    let alive = true
    async function loadDefault() {
      try {
        const override = await runCommand({ type: 'get_setting', key: 'quickapps.projects_dir' })
        const custom = override.type === 'setting' && typeof override.value === 'string' ? override.value.trim() : ''
        if (custom) {
          if (alive) setProjectsParent(custom)
          return
        }
        const sites = await runCommand({ type: 'get_setting', key: 'paths.sites_dir' })
        if (sites.type === 'setting' && typeof sites.value === 'string' && alive) setProjectsParent(sites.value)
      } catch {
        // Keep fallback below when backend unreachable.
      }
    }
    void loadDefault()
    return () => {
      alive = false
    }
  }, [])
  const effectiveParent = projectsParent || projects[0]?.path.replace(/[\\/][^\\/]+$/, '') || ''
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
    const ok = await confirmAction(
      `Your project files are not deleted, and its sites stay. It won't be re-added by folder scans — add the folder again to bring it back.`,
      `Remove ${p.name}?`,
      'Remove from list',
    )
    if (!ok) return
    await run(`remove:${p.id}`, () => web.removeProjectFromList(p.id))
  }

  async function saveNew(d: Domain) {
    await runCommand({ type: 'add_domain', domain: d })
    setAdding(null)
    await apply()
  }

  /**
   * Gives one project its automatic site: the same `<folder>.<tld>` the folder watcher
   * creates for a folder dropped into a projects folder, so the row turns from "no domain
   * yet" into a real site without waiting for the watcher. A folder with nothing to serve
   * is refused with the reason, not left looking like a click that did nothing.
   */
  async function addAutoDomain(p: Project) {
    await run(`auto-domain:${p.id}`, async () => {
      const res = await runCommand({ type: 'sync_auto_domains' })
      await refresh()
      const created = res.type === 'count' ? res.count : 0
      const now = domains.find((d) => d.project_id === p.id)
      if (now) {
        setTarget({ hostname: now.hostname, projectId: p.id })
        return
      }
      setMessage(
        created > 0
          ? `No site for ${p.name}: the other folders were added, but this one has no index.html or project file to serve.`
          : `No site for ${p.name}: its folder has no index.html or project file to serve. Add one, or create the site by hand.`,
      )
    })
  }

  /**
   * The same `<folder>.<tld>` for every selected project that has no site yet, in one
   * batch. `sync_auto_domains` walks the roots itself, so one call covers all of them —
   * and it is the very call the folder watcher makes, so this only front-runs it rather
   * than duplicating it. Rows that already have a site are left alone: their name is the
   * user's, not the folder's.
   */
  async function bulkAutoDomain() {
    const waiting = pickedRows.filter((r) => !r.site)
    if (waiting.length === 0) return
    await run('bulk', async () => {
      await runCommand({ type: 'sync_auto_domains' })
      const res = await runCommand({ type: 'list_domains' })
      const known = res.type === 'domains' ? res.domains : []
      const hostnames: string[] = []
      const skipped: string[] = []
      for (const r of waiting) {
        const host = known.find((d) => d.project_id === r.project?.id)?.hostname
        if (host) hostnames.push(host)
        else skipped.push(`${r.project?.name ?? r.key} has no index.html or project file to serve`)
      }
      // The rows that just gained a site are site rows now; the selection follows them.
      setPicked(new Set([...pickedKeys.filter((k) => !k.startsWith('project:')), ...hostnames.map((h) => `site:${h}`)]))
      reportBulk('Added an automatic domain to', hostnames.length, skipped)
      if (hostnames.length > 0) await web.refresh()
    })
  }

  async function bulkEnable(enabled: boolean) {
    if (pickedCount === 0) return
    await run('bulk', async () => {
      const { hostnames, noSite } = await resolveSites()
      if (hostnames.length === 0) {
        setMessage(`Nothing to ${enabled ? 'enable' : 'disable'}: ${noSite.join(', ')} ${noSite.length === 1 ? 'has' : 'have'} no site to serve. Add an index.html or a project file to the folder first.`)
        return
      }
      const out = await web.setDomainsEnabled(hostnames, enabled)
      reportBulk(enabled ? 'Enabled' : 'Disabled', out.changed, [...out.skipped, ...noSite.map((n) => `${n} has no site to serve`)])
      setPicked(new Set())
    })
  }

  async function movePickedToServer() {
    if (pickedCount === 0 || !bulkServerChoice) return
    // The default server is not a pin: `null` clears the override and puts the sites back
    // on whatever the default is today.
    const toDefault = bulkServerChoice === 'default'
    const target = serverChoices.find((s) => s.id === bulkServerChoice)
    const label = toDefault ? `the default (${target?.name ?? defaultServer})` : target?.name ?? bulkServerChoice
    await run('bulk', async () => {
      const { hostnames, noSite } = await resolveSites()
      if (hostnames.length === 0) {
        setMessage(`Nothing to move: ${noSite.join(', ')} ${noSite.length === 1 ? 'has' : 'have'} no site to serve.`)
        return
      }
      const out = await web.setDomainsServer(hostnames, toDefault ? null : bulkServerChoice)
      reportBulk(`Moved to ${label}`, out.changed, [...out.skipped, ...noSite.map((n) => `${n} has no site to serve`)])
      setPicked(new Set())
      setBulkServerChoice('')
    })
  }

  async function bulkDelete() {
    if (pickedCount === 0) return
    const names = pickedCount <= 3 ? pickedRows.map((r) => r.site?.hostname ?? r.project?.name).join(', ') : `these ${pickedCount} rows`
    const ok = await confirmAction(
      `Each site's certificate is revoked and its config file is removed. Projects behind a site leave the list when that was their last site. Project folders are never touched.`,
      `Delete ${names}?`,
      'Delete',
    )
    if (!ok) return
    await run('bulk', async () => {
      // Two different things: a site row is deleted, and a project row with no domain
      // yet is only taken off the list. Both are one call each, and both leave the files
      // alone, so a mixed selection is not a reason to refuse the whole batch.
      const hostnames = pickedRows.map((r) => r.site?.hostname).filter((h): h is string => !!h)
      const projectIds = pickedRows.filter((r) => !r.site).map((r) => r.project!.id)
      const skipped: string[] = []
      let changed = 0
      if (hostnames.length > 0) {
        const out = await web.deleteSites(hostnames)
        changed += out.changed
        skipped.push(...out.skipped)
      }
      for (const id of projectIds) {
        try {
          await web.removeProjectFromList(id)
          changed += 1
        } catch (e) {
          skipped.push(`${projects.find((p) => p.id === id)?.name ?? id} could not be taken off the list: ${e instanceof Error ? e.message : String(e)}`)
        }
      }
      reportBulk('Deleted', changed, skipped)
      setPicked(new Set())
    })
  }

  function menuFor({ site: d, project: p }: Row): MenuItem[] {
    const items: MenuItem[] = []
    const folder = d?.folder ?? p?.path
    if (folder) items.push({ label: 'Open project folder', icon: <FolderOpen />, hint: folder, onSelect: () => void run('open', () => runCommand({ type: 'open_path', path: folder })) })
    if (folder) items.push({ label: 'Open folder in code editor', icon: <Code2 />, hint: folder, onSelect: () => void run('code', () => runCommand({ type: 'open_in_editor', path: folder })) })
    if (p) items.push({ label: 'Terminal', icon: <SquareTerminal />, onSelect: () => setTarget({ hostname: d?.hostname ?? null, projectId: p.id, tab: 'terminal' }) })
    // WordPress gets a passwordless admin entry. The backend answers with a link that works
    // once, from this machine, for a few minutes; no password is read, asked for or stored.
    const wp = p ? wpProjects.find((w) => w.project_id === p.id) : undefined
    if (wp && p) {
      const projectId = p.id
      items.push({
        label: 'WP Admin',
        icon: <KeyRound />,
        hint: wp.url ? `One-time sign-in on ${wp.url.replace(/\/$/, '')} — no password needed` : 'This project has no domain yet',
        disabled: !wp.url || busy === `wp:${projectId}`,
        onSelect: () =>
          void run(`wp:${projectId}`, async () => {
            const signin = await runCommand({ type: 'wp_sign_in', project_id: projectId, hostname: d?.hostname ?? undefined, theme: resolvedTheme })
            if (signin.type !== 'wp_sign_in') return
            await runCommand({ type: 'open_url', url: signin.url })
            await refreshWpProjects().catch(() => undefined)
          }),
      })
      if (wp.pending_until)
        items.push({
          label: 'Cancel pending WP sign-in',
          icon: <X />,
          hint: 'Deletes the temporary link before it is used',
          disabled: busy === `wp-revoke:${projectId}`,
          onSelect: () =>
            void run(`wp-revoke:${projectId}`, async () => {
              await runCommand({ type: 'wp_sign_in_revoke', project_id: projectId })
              await refreshWpProjects().catch(() => undefined)
            }),
        })
    }
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
      items.push({
        label: 'Export to bundle…',
        icon: <Download />,
        hint: 'Its settings, .env files, database and files — pick what goes in',
        onSelect: () => setExportFor([d.hostname]),
      })
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
      items.push(
        {
          label: 'Add auto domain',
          icon: <Globe />,
          hint: 'The same <folder>.<tld> the folder watcher would create',
          disabled: busy === `auto-domain:${p.id}`,
          onSelect: () => void addAutoDomain(p),
        },
        { label: 'Add a domain', icon: <Plus />, onSelect: () => setTarget({ hostname: null, projectId: p.id, tab: 'settings' }) },
      )
    }
    items.push('separator')
    // One way off the list, not two: deleting a site takes it off the list, and the
    // project behind it goes with it when that was its last site. A row with no domain
    // has no site to delete, so it is removed as the project it is.
    if (d) {
      const key = d.hostname
      items.push({
        label: 'Delete site',
        icon: <Trash2 />,
        danger: true,
        disabled: removing.has(key) || busy === `delete:${key}`,
        onSelect: async () => {
          const last = p ? ` ${p.name} leaves the list with it.` : ''
          const ok = await confirmAction(
            `Its certificate is revoked and its config file is removed.${last} The project folder is not touched.`,
            `Delete ${d.hostname}?`,
            'Delete site',
          )
          if (!ok) return
          await run(`delete:${key}`, () => web.deleteSite(key))
        },
      })
    } else if (p) {
      items.push({
        label: 'Remove from list',
        icon: <FolderMinus />,
        danger: true,
        disabled: removing.has(p.id) || busy === `remove:${p.id}`,
        onSelect: () => void removeProject(p),
      })
    }
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
            <SaveButton action={web} name="apply" size="sm" variant="outline" busyLabel="Starting…" savedLabel="Started" onClick={() => void run('apply', () => apply())} title="Sites can't be opened while the web server is stopped">
              <Play /> Start web server
            </SaveButton>
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
          <GitCloneButton defaultParent={effectiveParent} onDone={adopt} />
          <ImportEnvironmentButton defaultParent={effectiveParent} onDone={adopt} />
          <SiteImportButton onWatch={() => onNavigate('tasks')} />
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {message && (
        <p className="text-sm text-muted-foreground">
          {message}{' '}
          <button className="cursor-pointer underline" onClick={() => setMessage(null)}>
            Dismiss
          </button>
        </p>
      )}
      {applying && (
        <p className="flex items-center gap-2 text-sm text-muted-foreground" role="status">
          <Spinner /> Updating the web-server configuration…
        </p>
      )}
      <ApplyReportCard web={web} />

      <Card>
        <CardContent className="p-0">
          <div className="flex flex-col gap-2 border-b border-border px-4 pb-2 pt-3">
            <div className="flex flex-wrap items-center gap-2">
              <div className="relative min-w-56 flex-1 sm:max-w-sm">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input className="h-8 pl-9" value={query} onChange={(e) => onSearch(e.target.value)} placeholder="Search sites by domain, project or folder" />
              </div>
              {/* The bulk bar only exists while something is picked, so the toolbar is the
                  same height whether it is there or not. */}
              {pickedCount > 0 && (
                <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Bulk actions">
                  <span className="whitespace-nowrap text-sm text-muted-foreground">
                    {pickedCount} selected
                  </span>
                  <Button size="sm" variant="outline" className="h-8" disabled={busyBulk || allEnabled} onClick={() => void bulkEnable(true)} title="Turn every selected site on">
                    <Play /> Enable
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8"
                    disabled={busyBulk || noDomainCount === 0}
                    onClick={() => void bulkAutoDomain()}
                    title={noDomainCount === 0 ? 'Every selected row already has a site' : `Give ${noDomainCount} project${noDomainCount === 1 ? '' : 's'} its <folder>.<tld> site`}
                  >
                    <Globe /> Add auto domain
                  </Button>
                  <Button size="sm" variant="outline" className="h-8" disabled={busyBulk || noneEnabled} onClick={() => void bulkEnable(false)} title="Turn every selected site off">
                    <StopIcon /> Disable
                  </Button>
                  <Select aria-label="Move the selected sites to a web server" className="h-8 w-40" value={bulkServerChoice} onChange={(e) => setBulkServerChoice(e.target.value)}>
                    <option value="">Web server…</option>
                    <option value="default">Default ({serverChoices.find((s) => s.active)?.name ?? defaultServer})</option>
                    {serverChoices
                      .filter((s) => !s.active)
                      .map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.name}
                        </option>
                      ))}
                  </Select>
                  <Button size="sm" variant="outline" className="h-8" disabled={busyBulk || !bulkServerChoice} onClick={() => void movePickedToServer()}>
                    Move sites
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8"
                    disabled={pickedHostnames.length === 0}
                    onClick={() => setExportFor(pickedHostnames)}
                    title="Export the selected sites into one bundle"
                  >
                    <Download /> Export
                  </Button>
                  <Button size="sm" variant="outline" className="h-8 text-destructive hover:bg-destructive/10" disabled={busyBulk} onClick={() => void bulkDelete()}>
                    <Trash2 /> Delete
                  </Button>
                  <Button size="sm" variant="ghost" className="h-8" disabled={busyBulk} onClick={() => setPicked(new Set())}>
                    Clear
                  </Button>
                </div>
              )}
            </div>
            {pickedCount > 0 && restCount > 0 && (
              <button
                className="cursor-pointer self-start text-xs text-muted-foreground underline"
                onClick={() => setPicked(new Set(visibleKeys))}
              >
                Select all {visibleKeys.length} matching {visibleKeys.length === 1 ? 'row' : 'rows'}
              </button>
            )}
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
          <Table wrapperClassName="rounded-none">
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    checked={allOnPagePicked}
                    onChange={(on) => pageRows.forEach((r) => toggle(r.key, on))}
                    disabled={pageRows.length === 0}
                    label={allOnPagePicked ? 'Clear the selection on this page' : 'Select every row on this page'}
                  />
                </TableHead>
                <TableHead>Domain</TableHead>
                <TableHead className="whitespace-nowrap">Status</TableHead>
                <TableHead className="w-24 whitespace-nowrap text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.map((r) => {
                const { site: d, project: p } = r
                const openSettings = () => setTarget({ hostname: d?.hostname ?? null, projectId: p?.id ?? null })
                const folder = d?.folder ?? p?.path
                // A row mid-removal dims and says so, so deleting never looks like a click
                // that did nothing.
                const pending = removing.has(d?.hostname ?? p?.id ?? '')
                return (
                  <TableRow key={r.key} aria-busy={pending} className={pending ? 'opacity-50 transition-opacity' : undefined}>
                    <TableCell>
                      {/* A project with no domain yet is pickable too: the batch gives it
                          its automatic `<folder>.<tld>` first, so a folder dropped into a
                          projects folder can be turned on or moved in the same click. */}
                      <Checkbox
                        checked={picked.has(r.key)}
                        onChange={(on) => toggle(r.key, on)}
                        disabled={pending}
                        label={`Select ${d?.hostname ?? p?.name}`}
                      />
                    </TableCell>
                    <TableCell className="min-w-0">
                      {/* The two icon buttons sit in their own column, centred against
                          the name *and* the sub-line below it. */}
                      <div className="flex min-w-0 items-center gap-1">
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center">
                            <button className="min-w-0 flex-1 cursor-pointer truncate text-left font-medium leading-6 hover:underline" onClick={openSettings} title={d?.hostname ?? p?.name}>
                              {d?.hostname ?? p?.name}
                            </button>
                          </div>
                          <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                            {d ? (
                              <>
                                <TechIcon id={GROUPS.find((g) => g.id === d.group)?.icon ?? 'static'} className="size-3.5 shrink-0" />
                                <span className="shrink-0">
                                  {d.kind}
                                  {d.has_app && ' + app'}
                                </span>
                                {defaultServer && d.server !== defaultServer && (
                                  <span className="flex shrink-0 items-center gap-1" title={`Served by ${d.server}, not the default (${defaultServer})`}>
                                    <TechIcon id={d.server} className="size-3.5" />
                                    {d.server}
                                  </span>
                                )}
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
                        </div>
                        {d && (
                          <>
                            <Button
                              size="sm"
                              variant="ghost"
                              className="size-7 shrink-0 cursor-pointer self-center px-0"
                              title={!d.enabled ? 'The site is disabled' : !status?.running ? 'Start the web server first' : `Open ${d.hostname}`}
                              aria-label={`Open ${d.hostname}`}
                              disabled={!d.enabled || !status?.running}
                              onClick={() => run('open', () => runCommand({ type: 'open_url', url: d.url }))}
                            >
                              <ExternalLink className="size-3.5" />
                            </Button>
                            {d.path_prefix && (
                              <Button
                                size="sm"
                                variant="ghost"
                                className="size-7 shrink-0 cursor-pointer self-center px-0"
                                title={
                                  !d.enabled
                                    ? 'The site is disabled'
                                    : !status?.running
                                      ? 'Start the web server first'
                                      : `Open localhost/${d.path_prefix}/`
                                }
                                aria-label={`Open localhost/${d.path_prefix}`}
                                disabled={!d.enabled || !status?.running}
                                onClick={() =>
                                  run('open-path', () =>
                                    runCommand({ type: 'open_url', url: localhostUrl(d) }),
                                  )
                                }
                              >
                                <Globe className="size-3.5" />
                              </Button>
                            )}
                            <Button
                              size="sm"
                              variant="ghost"
                              className="size-7 shrink-0 cursor-pointer self-center px-0"
                              title={copiedUrl === d.url ? 'Copied' : 'Copy domain'}
                              aria-label={copiedUrl === d.url ? 'Domain copied' : `Copy ${d.hostname}`}
                              onClick={async () => {
                                await navigator.clipboard.writeText(d.hostname)
                                setCopiedUrl(d.url)
                                window.setTimeout(() => setCopiedUrl((current) => (current === d.url ? null : d.url)), 1500)
                              }}
                            >
                              {copiedUrl === d.url ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                            </Button>
                          </>
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
                      {d?.path_prefix && (
                        <div className="mt-0.5 font-mono text-[11px] text-muted-foreground" title={`Also served at ${localhostUrl(d)}`}>
                          /{d.path_prefix}
                        </div>
                      )}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center justify-end gap-1">
                        <Button size="sm" variant="ghost" className="h-8 w-8 cursor-pointer px-0" onClick={openSettings} title="Settings" aria-label={`Settings for ${d?.hostname ?? p?.name}`}>
                          <Settings2 className="size-3.5" />
                        </Button>
                        {pending ? (
                          <span className="flex h-8 w-8 items-center justify-center" title="Removing…" role="status">
                            <Spinner />
                          </span>
                        ) : (
                          <ActionMenu label={`More actions for ${d?.hostname ?? p?.name}`} items={menuFor(r)} />
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                )
              })}
              {visible.length === 0 && (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-sm text-muted-foreground">
                    {rows.length === 0 ? 'No sites yet. Add one, add a project folder, or create one from Quick Apps.' : q ? `No site matches "${query.trim()}".` : 'No sites of this type.'}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
          {visible.length > 0 && (
            <Pagination
              label="sites"
              total={visible.length}
              page={page}
              perPage={perPage}
              onPage={setPage}
              // A new page size always starts at the top: page 7 of 25 rows per page has no
              // page 7, and stopping on a page past the end reads as an empty table.
              onPerPage={(size) => {
                setPerPage(size)
                setPage(1)
              }}
              className="border-t border-border"
            />
          )}
        </CardContent>
      </Card>

      {shownTarget && <SiteDialog key={`${shownTarget.hostname}:${shownTarget.projectId}`} target={shownTarget} web={web} onClose={() => setTarget(null)} onSaved={(hostname) => setTarget((t) => (t ? { ...t, hostname, tab: 'settings' } : t))} />}

      <DomainDialog
        projects={projects}
        domain={adding}
        defaultParent={effectiveParent}
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

      {/* One dialog for both entry points: a row's "Export to bundle…" and the bulk bar's
          Export. Rendered once, because two copies would each own their own state. */}
      <SiteExportDialog
        isOpen={exportFor !== null}
        hostnames={exportFor ?? []}
        onClose={() => setExportFor(null)}
        onWatch={() => {
          setExportFor(null)
          onNavigate('tasks')
        }}
      />

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
