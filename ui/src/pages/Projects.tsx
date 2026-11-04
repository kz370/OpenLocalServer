import { open } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
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
  UserRound,
  X,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { type Page } from '@/components/layout/Sidebar'
import { AddProjectDialog, type AddProjectItem } from '@/components/project/AddProjectDialog'
import { ManageProjectTypesDialog } from '@/components/project/ManageProjectTypesDialog'
import { GitCloneButton } from '@/components/project/ProjectImports'
import { ProjectSettingsDialog } from '@/components/project/ProjectSettingsDialog'
import { ProjectTypeFilterBar } from '@/components/project/ProjectTypeFilterBar'
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
  type CustomProjectType,
  type DomainSummary,
  type EditorInfo,
  type Project,
  type ProjectTypePreset,
  runCommand,
} from '@/core'
import { createPersistentCache, getCached } from '@/lib/cache'
import { confirmAction } from '@/lib/confirm'
import { openAddSiteForProject, openProject, openSettingsSection } from '@/lib/nav'
import { notify } from '@/lib/notify'
import {
  getProjectDisplayType,
  getProjectFilterPreferences,
  getProjectTypeId,
  groupProjectsByType,
  PROJECTS_TAB_KEY,
  type ProjectFilterPreferences,
} from '@/lib/projectTypes'
import { cn } from '@/lib/utils'

const projectsCache = createPersistentCache<Project[]>('projects_page_list', [])
const domainsCache = createPersistentCache<DomainSummary[]>('projects_page_domains', [])
const editorsCache = createPersistentCache<EditorInfo[]>('projects_page_editors', [])
const presetsCache = createPersistentCache<ProjectTypePreset[]>('projects_page_types', [])
const customTypesCache = createPersistentCache<CustomProjectType[]>('projects_page_custom_types', [])

function normPath(p: string | null | undefined): string {
  if (!p) return ''
  return p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

function ProjectsProgress({ label }: { label: string }) {
  return (
    <div className="absolute inset-x-0 top-0 z-10" role="status" aria-label={label}>
      <div className="h-0.5 overflow-hidden rounded-t-xl bg-muted">
        <div className="h-full w-1/3 animate-[ols-indeterminate_1.4s_ease-in-out_infinite] bg-primary" />
      </div>
      <span className="absolute left-3 top-1 rounded-b bg-card px-2 py-1 text-xs text-muted-foreground shadow-sm">{label}</span>
    </div>
  )
}

export function ProjectsPage({ onNavigate }: { onNavigate: (page: Page) => void }) {
  const [projects, setProjects] = useState<Project[]>(projectsCache.get)
  // Read storage directly because Sites can update this cache while ProjectsPage is unmounted.
  const [domains, setDomains] = useState<DomainSummary[]>(() => getCached('projects_page_domains', []))
  const [unconnectedProjectIds, setUnconnectedProjectIds] = useState<string[] | null>(null)
  const [editors, setEditors] = useState<EditorInfo[]>(editorsCache.get)
  const [presets, setPresets] = useState<ProjectTypePreset[]>(presetsCache.get)
  const [customTypes, setCustomTypes] = useState<CustomProjectType[]>(customTypesCache.get)

  const [query, setQuery] = useState('')
  const [preferences, setPreferences] = useState<ProjectFilterPreferences>(() =>
    getProjectFilterPreferences(),
  )
  const [typeTab, setTypeTab] = useState<string>(() => {
    try {
      return (
        localStorage.getItem(PROJECTS_TAB_KEY) ||
        sessionStorage.getItem('ols:projects-tab') ||
        'all'
      )
    } catch {
      return 'all'
    }
  })
  const [missingOnly, setMissingOnly] = useState<boolean>(false)
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)

  const handleSelectTypeTab = useCallback((id: string) => {
    setTypeTab(id)
    setPage(1)
    try {
      localStorage.setItem(PROJECTS_TAB_KEY, id)
      sessionStorage.setItem('ols:projects-tab', id)
    } catch {}
  }, [])

  const [activeMenu, setActiveMenu] = useState<{ id: string; anchor: MenuAnchor } | null>(null)
  const [editingProject, setEditingProject] = useState<Project | null>(null)
  const [removingProject, setRemovingProject] = useState<Project | null>(null)

  // Drag and drop state
  const [isDragOver, setIsDragOver] = useState(false)
  const [addProjectQueue, setAddProjectQueue] = useState<AddProjectItem[]>([])
  const [addProjectOpen, setAddProjectOpen] = useState(false)
  const [manageTypesOpen, setManageTypesOpen] = useState(false)

  const refreshProjects = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_projects' })
      if (res.type === 'projects') {
        setProjects(res.projects)
        setUnconnectedProjectIds(res.unconnected_project_ids ?? null)
        projectsCache.set(res.projects)
        const hasMissingProjects = res.projects.some(
          (project) => project.health && project.health.status !== 'healthy',
        )
        if (!hasMissingProjects && missingOnly) {
          setMissingOnly(false)
          setPage(1)
        }
      }
    } catch {
      // quiet poll
    }
  }, [missingOnly])

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

  const refreshCustomTypes = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_custom_project_types' })
      if (res.type === 'custom_project_types') {
        setCustomTypes(res.types)
        customTypesCache.set(res.types)
      }
    } catch {
      // quiet
    }
  }, [])

  const refreshAll = useCallback(async () => {
    setPreferences(getProjectFilterPreferences())
    await Promise.all([
      refreshProjects(),
      refreshDomains(),
      refreshEditors(),
      refreshPresets(),
      refreshCustomTypes(),
    ])
  }, [refreshProjects, refreshDomains, refreshEditors, refreshPresets, refreshCustomTypes])

  useEffect(() => {
    void refreshAll()
  }, [refreshAll])

  useEffect(() => {
    const handleFocus = () => void refreshAll()
    window.addEventListener('focus', handleFocus)
    return () => window.removeEventListener('focus', handleFocus)
  }, [refreshAll])

  useEffect(() => {
    let alive = true
    let unlisten: (() => void) | undefined
    void listen('projects-changed', () => {
      if (alive) void refreshAll()
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    return () => {
      alive = false
      unlisten?.()
    }
  }, [refreshAll])

  // Listen for Tauri drag & drop events
  useEffect(() => {
    let unlisten: (() => void) | undefined
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === 'enter' || event.payload.type === 'over') {
            setIsDragOver(true)
          } else if (event.payload.type === 'leave') {
            setIsDragOver(false)
          } else if (event.payload.type === 'drop') {
            setIsDragOver(false)
            const droppedPaths = event.payload.paths
            if (droppedPaths && droppedPaths.length > 0) {
              const queueItems: AddProjectItem[] = droppedPaths.map((p) => ({ path: p }))
              setAddProjectQueue(queueItems)
              setAddProjectOpen(true)
            }
          }
        })
        .then((cleanup) => {
          unlisten = cleanup
        })
        .catch(() => undefined)
    } catch {
      // not in Tauri environment
    }
    return () => unlisten?.()
  }, [])

  // Listen for context menu triggers and cold start pending project
  useEffect(() => {
    const handleTriggerAddProject = (e: Event) => {
      const path = (e as CustomEvent<string>).detail
      if (path) {
        setAddProjectQueue([{ path }])
        setAddProjectOpen(true)
      }
    }
    window.addEventListener('ols:trigger-add-project', handleTriggerAddProject)

    // Check cold-start pending path on mount
    runCommand({ type: 'take_pending_add_project' })
      .then((res) => {
        if (res.type === 'pending_add_project' && res.path) {
          setAddProjectQueue([{ path: res.path }])
          setAddProjectOpen(true)
        }
      })
      .catch(() => undefined)

    return () => window.removeEventListener('ols:trigger-add-project', handleTriggerAddProject)
  }, [])

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

  // Group tabs by type
  const typeGroups = useMemo(() => {
    return groupProjectsByType(allProjects, presets, customTypes, preferences)
  }, [allProjects, presets, customTypes, preferences])

  const effectiveMissingOnly = missingOnly && typeGroups.missingCount > 0

  // Reset typeTab if active tab no longer exists in available tabs or becomes empty (count < 1)
  useEffect(() => {
    if (typeTab !== 'all') {
      const tabExists = typeGroups.tabs.some((t) => t.id === typeTab && t.count >= 1)
      if (!tabExists) {
        setTypeTab('all')
        setPage(1)
        try {
          localStorage.setItem(PROJECTS_TAB_KEY, 'all')
          sessionStorage.setItem('ols:projects-tab', 'all')
        } catch {}
      }
    }
  }, [typeGroups.tabs, typeTab])

  const filteredProjects = useMemo(() => {
    const term = query.trim()
    return allProjects.filter((p) => {
      if (effectiveMissingOnly && !isProjectMissing(p)) return false
      if (typeTab !== 'all') {
        const typeId = getProjectTypeId(p)
        if (typeTab === 'auto') {
          if (typeId !== 'auto') return false
        } else if (typeTab.startsWith('custom:')) {
          if (p.project_type?.toLowerCase() !== typeTab.toLowerCase()) return false
        } else {
          if (p.project_type?.toLowerCase() !== typeTab.toLowerCase()) return false
        }
      }
      return matchesQuery(p, term)
    })
  }, [allProjects, effectiveMissingOnly, typeTab, query, domains])

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
        multiple: true,
        title: 'Select project folder',
      })
      if (!selected) return
      const paths = Array.isArray(selected) ? selected : [selected]
      if (paths.length === 0) return
      setAddProjectQueue(paths.map((p) => ({ path: p })))
      setAddProjectOpen(true)
    } catch (e) {
      notify.error(`Failed to select project folder: ${e instanceof Error ? e.message : String(e)}`)
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

  const handleOpenTerminal = async (p: Project) => {
    try {
      await runCommand({ type: 'open_project_terminal', project_id: p.id })
    } catch (e) {
      notify.error(`Failed to open terminal: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  const handleHideProject = async (p: Project) => {
    const linked = getProjectDomains(p)
    if (linked.length > 0) {
      notify.error(
        `Cannot hide "${p.name}" because it is currently connected to site(s): ${linked.map((d) => d.hostname).join(', ')}. Delete or unlink the site(s) first.`,
      )
      return
    }

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

  const handleRemoveFromList = async (p: Project) => {
    const ok = await confirmAction(
      `Remove "${p.name}" from the list? Its folder and database entry are removed from OpenLocalServer but files on disk are not touched.`,
      `Remove ${p.name} from list?`,
      'Remove from list',
    )
    if (!ok) return
    setRemovingProject(p)
    notify.info(`Removing "${p.name}" from the list…`)
    try {
      await runCommand({ type: 'ignore_project', id: p.id })
      notify.success(`Removed "${p.name}" from list`)
      await refreshAll()
    } catch (e) {
      notify.error(`Failed to remove project: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setRemovingProject(null)
    }
  }

  const menuFor = (p: Project): MenuItem[] => {
    const isHealthy = p.health?.status === 'healthy' || !p.health
    const linked = getProjectDomains(p)
    const hasSite = unconnectedProjectIds != null
      ? !unconnectedProjectIds.includes(p.id)
      : linked.length > 0

    const visibleEditors = editors.filter((ed) => ed.path && !ed.hidden)

    const editorSubmenu: MenuItem[] = [
      ...visibleEditors.map((ed) => ({
            label: ed.name,
            icon: <Code2 className="size-3.5" />,
            onSelect: () => {
              void runCommand({ type: 'open_with', app: ed.id, path: p.path })
            },
          })),
      ...(visibleEditors.length === 0 ? [{ label: 'No editors configured', disabled: true } as MenuItem] : []),
      'separator',
      {
        label: 'Edit editors...',
        icon: <Settings className="size-3.5" />,
        onSelect: () => {
        openSettingsSection('editors')
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
        onSelect: () => {
          openAddSiteForProject(p.id)
          onNavigate('sites')
        },
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
        ...(visibleEditors.length > 1 ? { submenu: editorSubmenu } : {}),
        onSelect: visibleEditors.length === 1
          ? () => void runCommand({ type: 'open_with', app: visibleEditors[0].id, path: p.path })
          : visibleEditors.length === 0
            ? () => { openSettingsSection('editors'); onNavigate('settings') }
            : undefined,
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
        disabled: linked.length > 0,
        hint: linked.length > 0 ? 'Connected to site' : undefined,
        onSelect: () => void handleHideProject(p),
      },
      {
        label: 'Remove from list',
        icon: <X className="size-4" />,
        danger: true,
        onSelect: () => void handleRemoveFromList(p),
      },
    ]
  }

  const effectiveParent = projects[0]?.path.replace(/[\\/][^\\/]+$/, '') || ''

  return (
    <div className="relative space-y-4">
      {/* Header */}
      <div>
        <h1 className="text-2xl font-bold tracking-tight text-foreground">Projects</h1>
        <p className="text-sm text-muted-foreground">
          Manage your project directories, organize them by project type, open them in your preferred code editors or terminal, and link them to local sites.
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
      <Card
        className="relative overflow-hidden"
        onDragOver={(e) => {
          e.preventDefault()
          setIsDragOver(true)
        }}
        onDragLeave={() => setIsDragOver(false)}
        onDrop={(e) => {
          e.preventDefault()
          setIsDragOver(false)
        }}
      >
        {/* Drag and Drop Visual Overlay */}
        {isDragOver && (
          <div className="absolute inset-0 z-50 flex flex-col items-center justify-center bg-background/90 backdrop-blur-xs border-2 border-dashed border-primary rounded-xl p-6 text-center animate-in fade-in duration-150 pointer-events-none">
            <div className="size-16 rounded-full bg-primary/10 flex items-center justify-center text-primary mb-3">
              <FolderPlus className="size-8" />
            </div>
            <h3 className="text-lg font-semibold text-foreground">Drop folder to add as project</h3>
            <p className="text-xs text-muted-foreground max-w-sm mt-1">
              Release folder here to open the settings modal and add it to OpenLocalServer.
            </p>
          </div>
        )}

        <CardContent className="p-0">
          {removingProject && <ProjectsProgress label={`Removing ${removingProject.name} from the list and updating site configuration…`} />}
          {/* Top toolbar: Type tabs, missing filter, manage types, and search */}
          <div className="flex flex-col gap-3 p-4 border-b border-border/80">
            {/* Type filter row: All first, chips, +N more popover, Manage types pinned right */}
            <ProjectTypeFilterBar
              tabs={typeGroups.tabs}
              selectedTab={typeTab}
              onSelectTab={handleSelectTypeTab}
              missingCount={typeGroups.missingCount}
              missingOnly={effectiveMissingOnly}
              onToggleMissingOnly={() => {
                setMissingOnly((prev) => !prev)
                setPage(1)
              }}
              onOpenManageTypes={() => setManageTypesOpen(true)}
              pinnedIds={preferences.pinnedIds}
            />

            {/* Search */}
            <div className="relative w-full">
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
          <Table wrapperClassName={cn('border-0 rounded-none shadow-none transition-opacity', removingProject && 'opacity-90')}>
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
                      ? 'No projects added yet. Click "Add project folder", drag and drop a folder, or use the Windows Explorer context menu to add one.'
                      : query.trim()
                        ? `No projects matching "${query.trim()}".`
                        : effectiveMissingOnly
                          ? 'No projects with missing or offline directories.'
                          : typeTab !== 'all'
                            ? `No projects under the "${typeGroups.tabs.find((t) => t.id === typeTab)?.label ?? typeTab}" type.`
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
                  const { id: typeId, label: typeLabel } = getProjectDisplayType(p, presets, customTypes)

                  return (
                    <TableRow
                      key={p.id}
                      data-state={isMenuOpen ? 'selected' : undefined}
                      className={cn(
                        'group h-[88px] cursor-default',
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
                      <TableCell className="h-[88px] min-w-0 py-1.5 first:pl-4">
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
                              projectType={p.project_type}
                              fallback="folder"
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
                            <div className="mt-1.5 flex h-4 items-center">
                              <span className={cn(
                                'inline-flex h-4 items-center gap-1 rounded px-1.5 text-[10px] leading-none',
                                p.auto_discovered
                                  ? 'bg-sky-500/10 text-sky-700 dark:text-sky-300'
                                  : 'bg-violet-500/10 text-violet-700 dark:text-violet-300',
                              )}>
                                {p.auto_discovered ? <FolderSearch className="size-2.5" /> : <UserRound className="size-2.5" />}
                                {p.auto_discovered ? 'Auto-discovered' : 'Added manually'}
                              </span>
                            </div>
                          </div>
                        </div>
                      </TableCell>

                      {/* Type Col */}
                      <TableCell className="h-[88px] w-36 whitespace-nowrap py-1.5 px-3">
                        <div className="flex items-center gap-1.5">
                          <TechIcon id={typeId} className="size-4 shrink-0 text-muted-foreground" />
                          <span className="text-xs font-medium text-foreground">{typeLabel}</span>
                        </div>
                      </TableCell>

                      {/* Connected Site Col */}
                      <TableCell className="h-[88px] w-56 whitespace-nowrap py-1.5 px-3">
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
                      <TableCell className="h-[88px] w-24 whitespace-nowrap py-1.5 pr-4 pl-3 text-right">
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
                          projectType={p.project_type}
                          fallback="folder"
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

      {/* Add Project Dialog (for drag-drop, context menu, and add button) */}
      <AddProjectDialog
        items={addProjectQueue}
        open={addProjectOpen}
        presets={presets}
        customTypes={customTypes}
        onCustomTypesChanged={refreshCustomTypes}
        onClose={() => {
          setAddProjectOpen(false)
          setAddProjectQueue([])
        }}
        onAdded={refreshAll}
      />

      {/* Manage Project Types Dialog */}
      <ManageProjectTypesDialog
        open={manageTypesOpen}
        onClose={() => setManageTypesOpen(false)}
        presets={presets}
        customTypes={customTypes}
        projects={projects}
        onUpdated={refreshAll}
      />

      {/* Project Settings Dialog */}
      <ProjectSettingsDialog
        project={editingProject}
        open={Boolean(editingProject)}
        onClose={() => setEditingProject(null)}
        onUpdated={refreshAll}
        presets={presets}
        customTypes={customTypes}
        onCustomTypesChanged={refreshCustomTypes}
      />
    </div>
  )
}
