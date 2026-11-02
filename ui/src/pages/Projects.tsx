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
  MoreHorizontal,
  Search,
  Settings,
  SquareTerminal,
  X,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { type Page } from '@/components/layout/Sidebar'
import { GitCloneButton } from '@/components/project/ProjectImports'
import { ProjectSettingsDialog } from '@/components/project/ProjectSettingsDialog'
import { SiteLogo } from '@/components/SiteLogo'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { FloatingMenu, type MenuAnchor, type MenuItem } from '@/components/ui/menu'
import { Pagination } from '@/components/ui/pagination'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  type DomainSummary,
  type EditorInfo,
  type Project,
  type ProjectTypePreset,
  runCommand,
} from '@/core'
import { createPersistentCache } from '@/lib/cache'
import { confirmAction } from '@/lib/confirm'
import { usePoll } from '@/lib/hooks'
import { openProject, openSettingsSection } from '@/lib/nav'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

const projectsCache = createPersistentCache<Project[]>('projects_page_list', [])
const domainsCache = createPersistentCache<DomainSummary[]>('projects_page_domains', [])
const editorsCache = createPersistentCache<EditorInfo[]>('projects_page_editors', [])
const presetsCache = createPersistentCache<ProjectTypePreset[]>('projects_page_types', [])

function normPath(p: string | null | undefined): string {
  if (!p) return ''
  return p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

export function ProjectsPage({ onNavigate }: { onNavigate: (page: Page) => void }) {
  const [projects, setProjects] = useState<Project[]>(projectsCache.get)
  const [domains, setDomains] = useState<DomainSummary[]>(domainsCache.get)
  const [editors, setEditors] = useState<EditorInfo[]>(editorsCache.get)
  const [presets, setPresets] = useState<ProjectTypePreset[]>(presetsCache.get)

  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<'all' | 'missing'>('all')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)

  const [activeMenu, setActiveMenu] = useState<{ id: string; anchor: MenuAnchor } | null>(null)
  const [editingProject, setEditingProject] = useState<Project | null>(null)

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

  const refreshPresets = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_project_types' })
      if (res.type === 'project_types') {
        setPresets(res.types)
        presetsCache.set(res.types)
      }
    } catch {
      // quiet
    }
  }, [])

  const refreshAll = useCallback(async () => {
    await Promise.all([refreshProjects(), refreshDomains(), refreshEditors(), refreshPresets()])
  }, [refreshProjects, refreshDomains, refreshEditors, refreshPresets])

  useEffect(() => {
    void refreshAll()
  }, [refreshAll])

  usePoll(refreshAll, 3000)

  // Map of project id / normalized folder to linked domains
  const getProjectDomains = useCallback(
    (p: Project): DomainSummary[] => {
      const normP = normPath(p.path)
      return domains.filter((d) => {
        if (d.project_id && d.project_id === p.id) return true
        if (normP && normPath(d.folder) === normP) return true
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
    return haystack.includes(term.toLowerCase())
  }

  const allProjects = useMemo(() => {
    return [...projects].sort((a, b) => a.name.localeCompare(b.name))
  }, [projects])

  const missingCount = useMemo(() => {
    return allProjects.filter(isProjectMissing).length
  }, [allProjects])

  const filteredProjects = useMemo(() => {
    const term = query.trim()
    return allProjects.filter((p) => {
      if (filter === 'missing' && !isProjectMissing(p)) return false
      return matchesQuery(p, term)
    })
  }, [allProjects, filter, query, domains])

  // Pagination slice
  const total = filteredProjects.length
  const pageProjects = useMemo(() => {
    const from = (page - 1) * perPage
    return filteredProjects.slice(from, from + perPage)
  }, [filteredProjects, page, perPage])

  // Handlers
  const handleAddProjectFolder = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select project folder',
      })
      if (!selected || typeof selected !== 'string') return
      const res = await runCommand({
        type: 'add_project',
        path: selected,
      })
      if (res.type === 'project') {
        notify.success(`Added project "${res.project.name}"`)
        await refreshAll()
      }
    } catch (e) {
      notify.error(`Failed to add project: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const handleRelocateProject = async (p: Project) => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: `Select new folder for project "${p.name}"`,
        defaultPath: p.path,
      })
      if (!selected || typeof selected !== 'string') return
      await runCommand({
        type: 'relocate_project',
        id: p.id,
        path: selected,
      })
      notify.success(`Relocated project "${p.name}"`)
      await refreshAll()
    } catch (e) {
      notify.error(`Failed to relocate project: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const handleCreateSite = async (p: Project) => {
    try {
      const res = await runCommand({ type: 'create_site_for_project', project_id: p.id })
      if (res.type === 'create_site_for_project') {
        if (res.domain) {
          notify.success(`Created site ${res.domain.hostname}`)
          await refreshAll()
        } else if (res.needs_setup) {
          notify.info(
            `Could not auto-detect a web server configuration for "${p.name}". You can configure a site manually in Sites.`,
          )
          onNavigate('sites')
        }
      }
    } catch (e) {
      notify.error(`Failed to create site: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const handleOpenTerminal = async (p: Project) => {
    try {
      await runCommand({ type: 'open_project_terminal', project_id: p.id })
    } catch (e) {
      notify.error(`Failed to open terminal: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const handleHideProject = async (p: Project) => {
    const ok = await confirmAction(
      `This will remove "${p.name}" from your projects list in OpenLocalServer. The project directory and all files on disk remain untouched.`,
      `Hide project "${p.name}"?`,
      'Hide project',
    )
    if (!ok) return

    try {
      await runCommand({ type: 'remove_project', id: p.id })
      notify.success(`Hidden "${p.name}" from projects`)
      await refreshAll()
    } catch (e) {
      notify.error(`Failed to hide project: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const getProjectTypeDisplay = (p: Project) => {
    if (!p.project_type) return { id: 'folder', label: 'Auto' }
    if (p.project_type.startsWith('custom:')) {
      const customName = p.project_type.replace(/^custom:/, '')
      return { id: customName.toLowerCase(), label: customName }
    }
    const preset = presets.find((pr) => pr.id === p.project_type)
    return {
      id: preset?.id ?? p.project_type,
      label: preset?.name ?? p.project_type,
    }
  }

  const menuFor = (p: Project): MenuItem[] => {
    const isHealthy = p.health?.status === 'healthy' || !p.health
    const linked = getProjectDomains(p)
    const hasSite = linked.length > 0

    const visibleEditors = editors.filter((ed) => ed.path && !ed.hidden)

    const editorSubmenu: MenuItem[] = [
      ...(visibleEditors.length > 0
        ? visibleEditors.map((ed) => ({
            label: ed.name,
            icon: <Code2 className="size-3.5" />,
            onSelect: () => {
              void runCommand({ type: 'open_with', app: ed.id, path: p.path })
            },
          }))
        : [{ label: 'No editors configured', disabled: true }]),
      'separator',
      {
        label: 'Edit editors...',
        icon: <Settings className="size-3.5" />,
        onSelect: () => {
          openSettingsSection('general')
          onNavigate('settings')
        },
      },
    ]

    return [
      {
        label: 'Create site from project',
        icon: <Globe className="size-4" />,
        disabled: hasSite || !isHealthy,
        hint: hasSite ? 'Site already exists' : !isHealthy ? 'Folder missing' : undefined,
        onSelect: () => void handleCreateSite(p),
      },
      'separator',
      {
        label: 'Open project directory',
        icon: <FolderOpen className="size-4" />,
        disabled: !isHealthy,
        onSelect: () => {
          void runCommand({ type: 'open_path', path: p.path })
        },
      },
      {
        label: 'Open with editor',
        icon: <Code2 className="size-4" />,
        disabled: !isHealthy,
        submenu: editorSubmenu,
      },
      {
        label: 'Open terminal',
        icon: <SquareTerminal className="size-4" />,
        disabled: !isHealthy,
        onSelect: () => void handleOpenTerminal(p),
      },
      'separator',
      {
        label: 'Settings',
        icon: <Settings className="size-4" />,
        onSelect: () => setEditingProject(p),
      },
      'separator',
      {
        label: 'Hide from list',
        icon: <EyeOff className="size-4" />,
        danger: true,
        onSelect: () => void handleHideProject(p),
      },
    ]
  }

  const effectiveParent = projects[0]?.path.replace(/[\\/][^\\/]+$/, '') || ''

  return (
    <div className="space-y-4">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold tracking-tight text-foreground">Projects</h1>
        <p className="text-sm text-muted-foreground">
          Manage your project directories, open them in your preferred code editors or terminal, and link them to local sites.
        </p>
      </div>

      {/* Action buttons */}
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={handleAddProjectFolder} className="cursor-pointer gap-2">
          <FolderPlus className="size-4" /> Add project folder
        </Button>
        <GitCloneButton defaultParent={effectiveParent} onDone={() => void refreshAll()} standalone={true} />
      </div>

      {/* Main card */}
      <Card>
        <CardContent className="p-0">
          {/* Top toolbar: filters and search */}
          <div className="flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:justify-between border-b border-border/80">
            {/* Filter tabs */}
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => {
                  setFilter('all')
                  setPage(1)
                }}
                className={cn(
                  'cursor-pointer rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                  filter === 'all'
                    ? 'bg-primary text-primary-foreground shadow-xs'
                    : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground',
                )}
              >
                All <span className="ml-1 opacity-70">{allProjects.length}</span>
              </button>

              <button
                type="button"
                onClick={() => {
                  setFilter('missing')
                  setPage(1)
                }}
                className={cn(
                  'cursor-pointer rounded-md px-3 py-1.5 text-xs font-medium transition-colors flex items-center gap-1.5',
                  filter === 'missing'
                    ? 'bg-destructive/20 text-destructive border border-destructive/40 shadow-xs'
                    : missingCount > 0
                      ? 'text-destructive/80 hover:bg-destructive/10'
                      : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground',
                )}
              >
                <FolderX className="size-3.5" />
                Folder not found
                {missingCount > 0 && (
                  <Badge variant="destructive" className="ml-1 px-1 py-0 text-[10px]">
                    {missingCount}
                  </Badge>
                )}
              </button>
            </div>

            {/* Search */}
            <div className="relative w-full max-w-xs">
              <Search className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value)
                  setPage(1)
                }}
                placeholder="Search projects by name, path, site..."
                className="pl-8 pr-7 h-9 text-xs"
              />
              {query && (
                <button
                  type="button"
                  onClick={() => {
                    setQuery('')
                    setPage(1)
                  }}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 text-muted-foreground hover:text-foreground cursor-pointer"
                >
                  <X className="size-3.5" />
                </button>
              )}
            </div>
          </div>

          {/* Table */}
          <Table wrapperClassName="border-0 rounded-none shadow-none">
            <TableHeader>
              <TableRow>
                <TableHead>Project</TableHead>
                <TableHead className="w-36 whitespace-nowrap">Type</TableHead>
                <TableHead className="w-56 whitespace-nowrap">Connected Site</TableHead>
                <TableHead className="w-24 whitespace-nowrap text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageProjects.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={4} className="h-32 text-center text-sm text-muted-foreground">
                    {allProjects.length === 0
                      ? 'No projects added yet. Click "Add project folder" or "Clone from Git" to add one.'
                      : query.trim()
                        ? `No projects matching "${query.trim()}".`
                        : filter === 'missing'
                          ? 'No projects with missing or offline directories.'
                          : 'No projects found.'}
                  </TableCell>
                </TableRow>
              ) : (
                pageProjects.map((p) => {
                  const isHealthy = p.health?.status === 'healthy' || !p.health
                  const isMenuOpen = activeMenu?.id === p.id
                  const linkedDomains = getProjectDomains(p)
                  const primaryDomain = linkedDomains[0]
                  const extraDomainsCount = linkedDomains.length > 1 ? linkedDomains.length - 1 : 0
                  const { id: typeId, label: typeLabel } = getProjectTypeDisplay(p)

                  return (
                    <TableRow
                      key={p.id}
                      data-state={isMenuOpen ? 'selected' : undefined}
                      className={cn(
                        'group cursor-default',
                        isMenuOpen && 'bg-primary/10 hover:bg-primary/15 shadow-[inset_3px_0_0_0_var(--primary)]',
                      )}
                      onContextMenu={(e) => {
                        e.preventDefault()
                        setActiveMenu({
                          id: p.id,
                          anchor: { type: 'cursor', x: e.clientX, y: e.clientY },
                        })
                      }}
                    >
                      {/* Project Col */}
                      <TableCell className="min-w-0 py-2.5 first:pl-4">
                        <div className="flex min-w-0 items-center">
                          <button
                            type="button"
                            onClick={() => setEditingProject(p)}
                            className="group/logo flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md transition-all hover:scale-105 active:scale-95 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                            title="Change project icon and settings"
                            aria-label={`Change icon for ${p.name}`}
                          >
                            <SiteLogo
                              icon={p.icon}
                              iconImage={p.icon_image}
                              hostname={p.name}
                              className={p.icon_image ? 'size-8' : 'size-6'}
                            />
                          </button>

                          <div className="mx-3.5 h-8 w-px bg-border/80 shrink-0" aria-hidden="true" />

                          <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                              <button
                                type="button"
                                className={cn(
                                  'cursor-pointer truncate text-left font-medium leading-6 hover:underline',
                                  isMenuOpen ? 'text-primary' : 'text-foreground',
                                )}
                                onClick={() => setEditingProject(p)}
                                title={p.name}
                              >
                                {p.name}
                              </button>
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
                            <div className="mt-0.5 flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
                              <span className="truncate max-w-md font-mono" title={p.path}>
                                {p.path}
                              </span>
                              <button
                                type="button"
                                className="opacity-0 group-hover:opacity-100 hover:text-foreground p-0.5 transition-opacity cursor-pointer shrink-0"
                                title="Copy folder path"
                                onClick={(e) => {
                                  e.stopPropagation()
                                  void navigator.clipboard.writeText(p.path)
                                  notify.success('Copied folder path to clipboard')
                                }}
                              >
                                <Copy className="size-3" />
                              </button>
                            </div>
                          </div>
                        </div>
                      </TableCell>

                      {/* Type Col */}
                      <TableCell className="w-36 whitespace-nowrap py-2.5 px-3">
                        <div className="flex items-center gap-1.5">
                          <TechIcon id={typeId} className="size-4 shrink-0 text-muted-foreground" />
                          <span className="text-xs font-medium text-foreground">{typeLabel}</span>
                        </div>
                      </TableCell>

                      {/* Connected Site Col */}
                      <TableCell className="w-56 whitespace-nowrap py-2.5 px-3">
                        {primaryDomain ? (
                          <div className="flex items-center gap-1.5">
                            <button
                              type="button"
                              className="group/site flex items-center gap-1 font-mono text-xs text-primary hover:underline cursor-pointer"
                              onClick={(e) => {
                                e.stopPropagation()
                                openProject({ id: p.id, site: primaryDomain.hostname })
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
                      </TableCell>

                      {/* Actions Col */}
                      <TableCell className="w-24 whitespace-nowrap py-2.5 pr-4 pl-3 text-right">
                        <div className="flex items-center justify-end gap-1">
                          {!isHealthy && (
                            <Button
                              size="sm"
                              variant="outline"
                              className="h-7 px-2 text-xs border-destructive/40 text-destructive hover:bg-destructive/10 cursor-pointer"
                              onClick={(e) => {
                                e.stopPropagation()
                                void handleRelocateProject(p)
                              }}
                              title="Locate missing project folder"
                            >
                              <FolderSearch className="size-3.5 mr-1" /> Locate
                            </Button>
                          )}
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-8 w-8 cursor-pointer px-0"
                            onClick={(e) => {
                              e.stopPropagation()
                              setEditingProject(p)
                            }}
                            title="Project settings"
                            aria-label={`Settings for ${p.name}`}
                          >
                            <Settings className="size-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            className={cn(
                              'h-8 w-8 shrink-0 cursor-pointer px-0',
                              isMenuOpen && 'bg-accent text-accent-foreground',
                            )}
                            title={`More actions for ${p.name}`}
                            aria-label={`More actions for ${p.name}`}
                            aria-haspopup="menu"
                            aria-expanded={isMenuOpen}
                            onClick={(e) => {
                              e.stopPropagation()
                              if (isMenuOpen && activeMenu?.anchor.type === 'button') {
                                setActiveMenu(null)
                              } else {
                                setActiveMenu({
                                  id: p.id,
                                  anchor: {
                                    type: 'button',
                                    rect: e.currentTarget.getBoundingClientRect(),
                                    element: e.currentTarget,
                                  },
                                })
                              }
                            }}
                          >
                            <MoreHorizontal className="size-4" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  )
                })
              )}
            </TableBody>
          </Table>

          {/* Context menu / action popup menu */}
          {activeMenu &&
            (() => {
              const p = projects.find((proj) => proj.id === activeMenu.id)
              if (!p) return null

              return (
                <FloatingMenu
                  header={
                    activeMenu.anchor.type === 'cursor' ? (
                      <>
                        <SiteLogo
                          icon={p.icon}
                          iconImage={p.icon_image}
                          hostname={p.name}
                          className="size-3.5 shrink-0"
                        />
                        <span className="truncate font-semibold text-xs" title={p.name}>
                          {p.name}
                        </span>
                      </>
                    ) : undefined
                  }
                  items={menuFor(p)}
                  anchor={activeMenu.anchor}
                  onClose={() => setActiveMenu(null)}
                />
              )
            })()}

          {/* Pagination */}
          {total > 0 && (
            <Pagination
              label="projects"
              total={total}
              page={page}
              perPage={perPage}
              onPage={setPage}
              onPerPage={(size) => {
                setPerPage(size)
                setPage(1)
              }}
              className="border-t border-border"
            />
          )}
        </CardContent>
      </Card>

      {/* Project Settings Dialog */}
      <ProjectSettingsDialog
        project={editingProject}
        open={Boolean(editingProject)}
        onClose={() => setEditingProject(null)}
        onUpdated={refreshAll}
      />
    </div>
  )
}
