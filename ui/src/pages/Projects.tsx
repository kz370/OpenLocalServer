import { open } from '@tauri-apps/plugin-dialog'
import {
  Code2,
  Copy,
  EyeOff,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  FolderX,
  Globe,
  Minus,
  Search,
  Settings,
  SquareTerminal,
  X,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { GitCloneButton } from '@/components/project/ProjectImports'
import { ProjectSettingsDialog } from '@/components/project/ProjectSettingsDialog'
import { SiteLogo } from '@/components/SiteLogo'
import { DomainDialog, newDomain } from '@/components/site/DomainDialog'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { ActionMenu, type MenuItem } from '@/components/ui/menu'
import { Pagination } from '@/components/ui/pagination'
import {
  type Domain,
  type EditorInfo,
  type Page,
  type Project,
  runCommand,
} from '@/core'
import { createPersistentCache } from '@/lib/cache'
import { confirmAction } from '@/lib/confirm'
import { usePoll } from '@/lib/hooks'
import { openProject } from '@/lib/nav'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

const projectsCache = createPersistentCache<Project[]>('projects_page_list', [])
const domainsCache = createPersistentCache<Domain[]>('projects_page_domains', [])
const editorsCache = createPersistentCache<EditorInfo[]>('projects_page_editors', [])

function normPath(p: string | null | undefined): string {
  if (!p) return ''
  return p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

export function ProjectsPage({ onNavigate }: { onNavigate: (page: Page) => void }) {
  const [projects, setProjects] = useState<Project[]>(projectsCache.get)
  const [domains, setDomains] = useState<Domain[]>(domainsCache.get)
  const [editors, setEditors] = useState<EditorInfo[]>(editorsCache.get)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<'all' | 'missing'>('all')
  const [currentPage, setCurrentPage] = useState(1)
  const perPage = 8

  const [editingProject, setEditingProject] = useState<Project | null>(null)
  const [addingDomain, setAddingDomain] = useState<Domain | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const refreshProjects = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_projects' })
      if (res.type === 'projects') {
        setProjects(res.projects)
        projectsCache.set(res.projects)
      }
    } catch {
      // quiet poll
    }
  }, [])

  const refreshDomains = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_domains' })
      if (res.type === 'domains') {
        setDomains(res.domains)
        domainsCache.set(res.domains)
      }
    } catch {
      // quiet poll
    }
  }, [])

  const refreshEditors = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_editors' })
      if (res.type === 'editors') {
        setEditors(res.editors)
        editorsCache.set(res.editors)
      }
    } catch {
      // quiet poll
    }
  }, [])

  const refreshAll = useCallback(async () => {
    await Promise.all([refreshProjects(), refreshDomains(), refreshEditors()])
  }, [refreshProjects, refreshDomains, refreshEditors])

  useEffect(() => {
    void refreshAll()
  }, [refreshAll])

  usePoll(refreshAll, 3000)

  // Find linked domains for a given project
  const getProjectDomains = useCallback(
    (p: Project): Domain[] => {
      const normP = normPath(p.path)
      return domains.filter((d) => {
        if (d.project_id && d.project_id === p.id) return true
        if (normP && normPath(d.root) === normP) return true
        return false
      })
    },
    [domains],
  )

  const isProjectMissing = (p: Project) => Boolean(p.health && p.health.status !== 'healthy')

  // Search & Filter
  const matchesQuery = (p: Project, term: string) => {
    if (!term) return true
    const linked = getProjectDomains(p)
    const hostnames = linked.map((d) => d.hostname).join(' ')
    const haystack = `${p.name} ${p.path} ${p.project_type ?? ''} ${hostnames}`.toLowerCase()
    return haystack.includes(term)
  }

  const q = query.trim().toLowerCase()
  const matchingProjects = useMemo(() => {
    return projects.filter((p) => matchesQuery(p, q))
  }, [projects, q])

  const missingCount = useMemo(() => {
    return matchingProjects.filter(isProjectMissing).length
  }, [matchingProjects])

  const visibleProjects = useMemo(() => {
    if (filter === 'missing') {
      return matchingProjects.filter(isProjectMissing)
    }
    return matchingProjects
  }, [matchingProjects, filter])

  const totalPages = Math.max(1, Math.ceil(visibleProjects.length / perPage))
  const pageProjects = useMemo(() => {
    const start = (currentPage - 1) * perPage
    return visibleProjects.slice(start, start + perPage)
  }, [visibleProjects, currentPage, perPage])

  // Reset page when filter or search changes
  useEffect(() => {
    setCurrentPage(1)
  }, [filter, query])

  // Top action: Add folder
  const handleAddFolder = async () => {
    const picked = await open({ directory: true, title: 'Select a project folder' })
    if (!picked || Array.isArray(picked)) return

    setBusy('add-folder')
    try {
      const res = await runCommand({ type: 'add_project', path: picked })
      if (res.type === 'project') {
        notify.success(`Added project "${res.project.name}"`)
        await refreshProjects()
      }
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  // Row action: Relocate project
  const handleRelocateProject = async (p: Project) => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: `Locate folder for ${p.name}`,
    })
    if (!selected || typeof selected !== 'string') return

    setBusy(`relocate:${p.id}`)
    try {
      const res = await runCommand({ type: 'relocate_project', id: p.id, path: selected })
      if (res.type === 'project') {
        notify.success(`Relocated project "${p.name}" to ${selected}`)
        await refreshAll()
      }
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  // Row action: Create site for project
  const handleCreateSite = async (p: Project) => {
    setBusy(`create-site:${p.id}`)
    try {
      const res = await runCommand({ type: 'create_site_for_project', project_id: p.id })
      if (res.type === 'create_site_for_project') {
        if (res.domain) {
          notify.success(`Created site ${res.domain.hostname} for ${p.name}`)
          await refreshAll()
        } else if (res.needs_setup) {
          setAddingDomain({
            ...newDomain(),
            hostname: res.suggested_hostname,
            folder: p.path,
            project_id: p.id,
          })
        }
      }
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  // Save manual site creation
  const handleSaveDomain = async (d: Domain) => {
    try {
      await runCommand({ type: 'add_domain', domain: d })
      setAddingDomain(null)
      await runCommand({ type: 'apply_web', overwrite: [] }).catch(() => undefined)
      notify.success(`Created site ${d.hostname}`)
      await refreshAll()
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    }
  }

  // Row action: Hide project
  const handleHideProject = async (p: Project) => {
    const ok = await confirmAction(
      `"${p.name}" will be hidden from the projects list, but its folder on disk is not touched. You can restore it anytime in Settings > Hide/Exclude.`,
      `Hide ${p.name}?`,
      'Hide from list',
    )
    if (!ok) return

    setBusy(`hide:${p.id}`)
    try {
      await runCommand({ type: 'remove_project', id: p.id })
      notify.info(`Hidden "${p.name}" from projects`)
      await refreshProjects()
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  // Row action: Open terminal
  const handleOpenTerminal = async (p: Project) => {
    try {
      await runCommand({ type: 'open_project_terminal', project_id: p.id })
    } catch (e) {
      notify.error(e instanceof Error ? e.message : String(e))
    }
  }

  // Construct context menu items for a project row
  const menuFor = (p: Project): MenuItem[] => {
    const isHealthy = !isProjectMissing(p)
    const linked = getProjectDomains(p)
    const hasSite = linked.length > 0
    const items: MenuItem[] = []

    if (!isHealthy) {
      items.push({
        label: 'Locate project folder…',
        icon: <FolderSearch className="size-3.5" />,
        hint: `Re-point ${p.name} to its new folder location`,
        onSelect: () => void handleRelocateProject(p),
      })
    }

    // 1. Create site from project
    items.push({
      label: 'Create site from project',
      icon: <Globe className="size-3.5" />,
      disabled: hasSite || !isHealthy || busy === `create-site:${p.id}`,
      hint: hasSite
        ? `Site already exists: ${linked[0].hostname}`
        : !isHealthy
          ? 'Project folder is not accessible'
          : 'Creates a local domain for this project',
      onSelect: () => void handleCreateSite(p),
    })

    // 2. Open project directory
    items.push({
      label: 'Open project directory',
      icon: <FolderOpen className="size-3.5" />,
      disabled: !isHealthy,
      hint: isHealthy ? p.path : 'Project folder is not accessible',
      onSelect: () => void runCommand({ type: 'open_path', path: p.path }),
    })

    // 3. Open with editor
    const installedEditors = editors.filter((e) => e.path)
    const editorSubmenu: MenuItem[] = [
      ...installedEditors.map((e) => ({
        label: e.name,
        icon: <Code2 className="size-3.5" />,
        onSelect: () => void runCommand({ type: 'open_with', path: p.path, app: e.id }),
      })),
      'separator' as const,
      {
        label: 'Manage editors…',
        icon: <Settings className="size-3.5" />,
        onSelect: () => onNavigate('settings'),
      },
    ]

    items.push({
      label: 'Open with editor',
      icon: <Code2 className="size-3.5" />,
      disabled: !isHealthy || installedEditors.length === 0,
      hint: isHealthy ? undefined : 'Project folder is not accessible',
      submenu: editorSubmenu,
    })

    // 4. Open terminal
    items.push({
      label: 'Open terminal',
      icon: <SquareTerminal className="size-3.5" />,
      disabled: !isHealthy,
      hint: isHealthy ? 'Opens in external Windows Terminal window' : 'Project folder is not accessible',
      onSelect: () => void handleOpenTerminal(p),
    })

    items.push('separator')

    // 5. Hide from list
    items.push({
      label: 'Hide from list',
      icon: <EyeOff className="size-3.5" />,
      disabled: busy === `hide:${p.id}`,
      onSelect: () => void handleHideProject(p),
    })

    // 6. Settings
    items.push({
      label: 'Settings',
      icon: <Settings className="size-3.5" />,
      onSelect: () => setEditingProject(p),
    })

    return items
  }

  const effectiveParent = projects[0]?.path.replace(/[\\/][^\\/]+$/, '') || ''

  return (
    <div className="flex flex-col gap-4">
      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Projects</h1>
          <p className="text-sm text-muted-foreground">
            Manage your project directories, open them in your preferred code editors or terminal, and link them to local sites.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => void handleAddFolder()} disabled={busy === 'add-folder'}>
            <FolderPlus /> Add project folder
          </Button>
          <GitCloneButton
            defaultParent={effectiveParent}
            standalone={true}
            onDone={async () => {
              await refreshProjects()
            }}
          />
        </div>
      </div>

      {/* Main Card with Tabs & Table */}
      <Card>
        <CardContent className="p-0">
          {/* Controls bar: Search & Filter Tabs */}
          <div className="flex flex-col gap-3 border-b border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
            {/* Filter Tabs */}
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant={filter === 'all' ? 'secondary' : 'ghost'}
                className="h-8 gap-1.5 text-xs cursor-pointer"
                onClick={() => setFilter('all')}
              >
                All
                <Badge variant="secondary" className="px-1.5 py-0 text-[10px]">
                  {matchingProjects.length}
                </Badge>
              </Button>
              <Button
                size="sm"
                variant={filter === 'missing' ? 'secondary' : 'ghost'}
                className={cn('h-8 gap-1.5 text-xs cursor-pointer', filter === 'missing' ? 'border-destructive/30' : '')}
                onClick={() => setFilter('missing')}
              >
                <FolderX className="size-3.5 text-destructive" />
                Folder not found
                {missingCount > 0 && (
                  <Badge variant="destructive" className="px-1.5 py-0 text-[10px]">
                    {missingCount}
                  </Badge>
                )}
              </Button>
            </div>

            {/* Search Input */}
            <div className="relative w-full sm:w-72">
              <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search projects by name, path, site…"
                className="h-8 pl-8 pr-8 text-xs"
              />
              {query && (
                <button
                  type="button"
                  onClick={() => setQuery('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground cursor-pointer"
                >
                  <X className="size-3.5" />
                </button>
              )}
            </div>
          </div>

          {/* Projects Table */}
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="border-b border-border/80 bg-muted/40 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                <tr>
                  <th className="py-2.5 pl-4 pr-3">Project</th>
                  <th className="py-2.5 px-3">Type</th>
                  <th className="py-2.5 px-3">Connected Site</th>
                  <th className="py-2.5 px-3">Directory</th>
                  <th className="py-2.5 pr-4 pl-3 text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border/60">
                {pageProjects.length === 0 ? (
                  <tr>
                    <td colSpan={5} className="py-12 text-center text-sm text-muted-foreground">
                      {filter === 'missing'
                        ? 'No project folders are missing.'
                        : query
                          ? `No projects match "${query}".`
                          : 'No projects added yet. Click "Add project folder" or "Clone from Git" to add one.'}
                    </td>
                  </tr>
                ) : (
                  pageProjects.map((p) => {
                    const isHealthy = !isProjectMissing(p)
                    const linked = getProjectDomains(p)
                    const primaryDomain = linked[0]
                    const extraDomainsCount = Math.max(0, linked.length - 1)
                    const projectTypeLabel = p.project_type
                      ? p.project_type.replace(/^custom:/, '')
                      : 'Auto'

                    return (
                      <tr
                        key={p.id}
                        className={cn(
                          'group transition-colors hover:bg-accent/40',
                          !isHealthy ? 'bg-destructive/5' : '',
                        )}
                        onContextMenu={(e) => {
                          e.preventDefault()
                          // Right-click opens context menu
                        }}
                      >
                        {/* Project Col (Icon + Name + Missing badge) */}
                        <td className="py-3 pl-4 pr-3">
                          <div className="flex items-center gap-3">
                            <SiteLogo
                              icon={p.icon}
                              iconImage={p.icon_image}
                              displayName={p.name}
                              className="size-8 rounded-lg shadow-xs"
                            />
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span className="font-semibold text-foreground truncate max-w-xs" title={p.name}>
                                  {p.name}
                                </span>
                                {!isHealthy && (
                                  <Badge
                                    variant="destructive"
                                    className="gap-1 py-0 px-1.5 text-[10px] font-medium"
                                    title={
                                      p.health?.status === 'offline'
                                        ? 'The drive containing this project folder is disconnected or offline.'
                                        : 'The folder for this project does not exist on disk.'
                                    }
                                  >
                                    <FolderX className="size-2.5" />
                                    {p.health?.status === 'offline' ? 'Drive offline' : 'Folder missing'}
                                  </Badge>
                                )}
                              </div>
                              <span className="text-[11px] text-muted-foreground block truncate max-w-sm">
                                {p.path}
                              </span>
                            </div>
                          </div>
                        </td>

                        {/* Project Type Col */}
                        <td className="py-3 px-3">
                          <div className="flex items-center gap-1.5">
                            <TechIcon
                              brand={p.project_type ? p.project_type.replace(/^custom:/, '').toLowerCase() : 'folder'}
                              className="size-4 shrink-0 text-muted-foreground"
                            />
                            <span className="text-xs font-medium capitalize text-foreground">
                              {projectTypeLabel}
                            </span>
                          </div>
                        </td>

                        {/* Connected Site Col */}
                        <td className="py-3 px-3">
                          {primaryDomain ? (
                            <div className="flex items-center gap-1.5">
                              <button
                                type="button"
                                className="group/site flex items-center gap-1 font-mono text-xs text-primary hover:underline cursor-pointer"
                                onClick={() => {
                                  openProject({ site: primaryDomain.hostname })
                                  onNavigate('sites')
                                }}
                                title={`Open ${primaryDomain.hostname} in Sites`}
                              >
                                <Globe className="size-3.5 text-primary shrink-0 transition-transform group-hover/site:scale-110" />
                                <span className="truncate max-w-[180px]">{primaryDomain.hostname}</span>
                              </button>
                              {extraDomainsCount > 0 && (
                                <Badge variant="secondary" className="px-1 py-0 text-[10px] font-mono">
                                  +{extraDomainsCount}
                                </Badge>
                              )}
                            </div>
                          ) : (
                            <span className="flex items-center gap-1 text-xs text-muted-foreground">
                              <Minus className="size-3 opacity-60" /> No site
                            </span>
                          )}
                        </td>

                        {/* Directory Col */}
                        <td className="py-3 px-3">
                          <div className="flex items-center gap-1 max-w-[220px]">
                            <span className="truncate font-mono text-xs text-muted-foreground" title={p.path}>
                              {p.path}
                            </span>
                            <button
                              type="button"
                              className="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:text-foreground text-muted-foreground cursor-pointer"
                              title="Copy directory path"
                              onClick={() => {
                                void navigator.clipboard.writeText(p.path)
                                notify.success('Copied folder path to clipboard')
                              }}
                            >
                              <Copy className="size-3" />
                            </button>
                          </div>
                        </td>

                        {/* Actions Col */}
                        <td className="py-3 pr-4 pl-3 text-right">
                          <div className="flex items-center justify-end gap-1">
                            {!isHealthy && (
                              <Button
                                size="sm"
                                variant="outline"
                                className="h-7 px-2 text-xs border-destructive/40 text-destructive hover:bg-destructive/10"
                                onClick={() => void handleRelocateProject(p)}
                                title="Locate missing project folder"
                              >
                                <FolderSearch className="size-3.5 mr-1" /> Locate
                              </Button>
                            )}
                            <ActionMenu items={menuFor(p)} label={`Actions for ${p.name}`} />
                          </div>
                        </td>
                      </tr>
                    )
                  })
                )}
              </tbody>
            </table>
          </div>

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="border-t border-border px-4 py-3">
              <Pagination
                page={currentPage}
                totalPages={totalPages}
                totalItems={visibleProjects.length}
                perPage={perPage}
                onChange={setCurrentPage}
              />
            </div>
          )}
        </CardContent>
      </Card>

      {/* Project Settings Modal */}
      <ProjectSettingsDialog
        project={editingProject}
        open={Boolean(editingProject)}
        onClose={() => setEditingProject(null)}
        onUpdated={refreshAll}
      />

      {/* Prefilled Add Domain Modal (if create site needs setup) */}
      {addingDomain && (
        <DomainDialog
          domain={addingDomain}
          onClose={() => setAddingDomain(null)}
          onSave={handleSaveDomain}
          existingDomains={domains}
        />
      )}
    </div>
  )
}
