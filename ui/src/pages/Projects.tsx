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
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { RootFolderControls, RootGroupRow } from '@/components/RootFolderControls'
import { Spinner } from '@/components/Spinner'
import { ScrollableList } from '@/components/ScrollableList'
import { type Page } from '@/components/layout/Sidebar'
import { AddProjectDialog, type AddProjectItem } from '@/components/project/AddProjectDialog'
import { ManageProjectTypesDialog } from '@/components/project/ManageProjectTypesDialog'
import { GitCloneButton } from '@/components/project/ProjectImports'
import { ProjectSettingsDialog } from '@/components/project/ProjectSettingsDialog'
import { ProjectTypeFilterBar } from '@/components/project/ProjectTypeFilterBar'
import { SiteLogo } from '@/components/SiteLogo'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Checkbox } from '@/components/ui/checkbox'
import { MiddleTruncate } from '@/components/ui/MiddleTruncate'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { FloatingMenu, type MenuAnchor, type MenuItem } from '@/components/ui/menu'
import { DockerizeDialog } from '@/components/docker/DockerizeDialog'
import { useDockerizeMenu } from '@/lib/dockerizeMenu'
import { DockerIcon } from '@/components/layout/SidebarIcons'
import { Pagination } from '@/components/ui/pagination'
import {
  Table,
  TableBody,
  TableCell,
  TableSpanCell,
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
import { onOpenProjectsFilter, openAddSiteForProject, openProject, openSettingsSection, takePendingProjectsFilter } from '@/lib/nav'
import { notify } from '@/lib/notify'
import { compareByRoot, rootFolderOf, rootKeyOf, sortsByRoot, useListView } from '@/lib/rootFolder'
import { invalidateSiteProjectCaches } from '@/lib/siteCaches'
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

export function ProjectsPage({ onNavigate }: { onNavigate: (page: Page) => void }) {
  const [projects, setProjects] = useState<Project[]>(projectsCache.get)
  const dockerizeMenu = useDockerizeMenu()
  const [dockerizeFor, setDockerizeFor] = useState<string | null>(null)
  // Read storage directly because Sites can update this cache while ProjectsPage is unmounted.
  const [domains, setDomains] = useState<DomainSummary[]>(() => getCached('projects_page_domains', []))
  /** False until the project list has been read from the backend once (or a cached copy exists). */
  const [projectsLoaded, setProjectsLoaded] = useState(() => projectsCache.get().length > 0)
  const [unconnectedProjectIds, setUnconnectedProjectIds] = useState<string[] | null>(null)
  const [siteIgnoredProjectIds, setSiteIgnoredProjectIds] = useState<string[]>([])
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
  const [missingOnly, setMissingOnly] = useState<boolean>(() => takePendingProjectsFilter() === 'missing')
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(25)
  useEffect(
    () =>
      onOpenProjectsFilter((f) => {
        if (f === 'missing') {
          setMissingOnly(true)
          setPage(1)
        }
      }),
    [],
  )

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

  // Drag and drop state
  const [isDragOver, setIsDragOver] = useState(false)
  const [addProjectQueue, setAddProjectQueue] = useState<AddProjectItem[]>([])
  const [addProjectOpen, setAddProjectOpen] = useState(false)
  const [manageTypesOpen, setManageTypesOpen] = useState(false)

  // Bulk selection: project ids ticked in the first column.
  const [picked, setPicked] = useState<Set<string>>(() => new Set())
  const [busyBulk, setBusyBulk] = useState(false)
  // Sort by name or root folder, and optional headings per root folder (remembered).
  const [view, setView] = useListView('ols:projects-view')
  /** While above zero a bulk action is running and the page ignores backend change events. */
  const quietRefreshes = useRef(0)

  const refreshProjects = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'list_projects' })
      if (res.type === 'projects') {
        setProjects(res.projects)
        setUnconnectedProjectIds(res.unconnected_project_ids ?? null)
        setSiteIgnoredProjectIds(res.site_ignored_project_ids ?? [])
        projectsCache.set(res.projects)
        setProjectsLoaded(true)
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
      // A bulk action refreshes once at the end; mid-batch refreshes would bring rows back.
      if (alive && quietRefreshes.current === 0) void refreshAll()
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
    const byRoot = sortsByRoot(view)
    return [...projects].sort((a, b) =>
      byRoot ? compareByRoot(rootKeyOf(a.path), rootKeyOf(b.path), a.name, b.name) : a.name.localeCompare(b.name),
    )
  }, [projects, view])

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

  /** How many of the listed projects share each root folder, for the group headings. */
  const rootCounts = useMemo(() => {
    const counts = new Map<string, number>()
    for (const p of filteredProjects) {
      const key = rootKeyOf(p.path)
      counts.set(key, (counts.get(key) ?? 0) + 1)
    }
    return counts
  }, [filteredProjects])

  // Pagination slice
  const total = filteredProjects.length
  const pageProjects = useMemo(() => {
    const from = (page - 1) * perPage
    return filteredProjects.slice(from, from + perPage)
  }, [filteredProjects, page, perPage])

  // Ticks that point at a project that no longer exists drop away.
  useEffect(() => {
    setPicked((prev) => {
      if (prev.size === 0) return prev
      const alive = new Set(projects.map((p) => p.id))
      const next = new Set([...prev].filter((id) => alive.has(id)))
      return next.size === prev.size ? prev : next
    })
  }, [projects])

  const pickedProjects = useMemo(() => allProjects.filter((p) => picked.has(p.id)), [allProjects, picked])
  const pickedCount = pickedProjects.length
  const allOnPagePicked = pageProjects.length > 0 && pageProjects.every((p) => picked.has(p.id))
  const restCount = filteredProjects.filter((p) => !picked.has(p.id)).length

  const togglePicked = (id: string, on: boolean) =>
    setPicked((prev) => {
      const next = new Set(prev)
      if (on) next.add(id)
      else next.delete(id)
      return next
    })

  /** Selected projects that have no site yet and can get one. */
  const pickedWithoutSite = pickedProjects.filter(
    (p) => (p.health?.status === 'healthy' || !p.health) && getProjectDomains(p).length === 0 && !siteIgnoredProjectIds.includes(p.id),
  )
  /** Selected projects not connected to a site, so they can be hidden. */
  const pickedHideable = pickedProjects.filter((p) => getProjectDomains(p).length === 0)

  const errorText = (e: unknown) =>
    e instanceof Error ? e.message : String((e as { problem?: string } | null)?.problem ?? e)

  /** Takes projects out of the list and the selection right away; a refresh restores any that failed. */
  const dropProjects = (ids: string[]) => {
    const gone = new Set(ids)
    setProjects((list) => {
      const next = list.filter((p) => !gone.has(p.id))
      projectsCache.set(next)
      return next
    })
    setPicked((prev) => new Set([...prev].filter((id) => !gone.has(id))))
  }

  /** One batch, one message: how many worked and which ones failed and why. */
  const reportBulk = (done: string, changed: number, failed: string[]) => {
    if (failed.length === 0) {
      notify.success(`${done}: ${changed} project${changed === 1 ? '' : 's'}.`)
    } else if (changed === 0) {
      notify.warning(`No projects changed. ${failed.join('; ')}`, { details: failed.join('\n') })
    } else {
      notify.batch({
        operation: 'projects',
        total: changed + failed.length,
        succeeded: changed,
        failed: failed.length,
        failedItems: failed,
      })
    }
  }

  const runBulk = async (work: () => Promise<void>) => {
    setBusyBulk(true)
    try {
      await work()
    } finally {
      setBusyBulk(false)
      await refreshAll()
    }
  }

  const bulkCreateSites = () =>
    runBulk(async () => {
      const failed: string[] = []
      let changed = 0
      for (const p of pickedWithoutSite) {
        try {
          await runCommand({ type: 'create_site_for_project', project_id: p.id })
          invalidateSiteProjectCaches()
          changed += 1
        } catch (e) {
          failed.push(`${p.name}: ${errorText(e)}`)
        }
      }
      reportBulk('Added a site to', changed, failed)
      setPicked(new Set())
    })

  const bulkHide = async () => {
    const skipped = pickedCount - pickedHideable.length
    const n = pickedHideable.length
    const ok = await confirmAction(
      `This removes ${n} project${n === 1 ? '' : 's'} from your projects list in OpenLocalServer. The folders and all files on disk stay untouched.${skipped > 0 ? ` ${skipped} selected project${skipped === 1 ? ' is' : 's are'} connected to a site and will be skipped.` : ''}`,
      `Hide ${n} project${n === 1 ? '' : 's'}?`,
      'Hide projects',
    )
    if (!ok) return
    const targets = pickedHideable
    quietRefreshes.current += 1
    dropProjects(targets.map((p) => p.id))
    const failed: string[] = []
    let changed = 0
    try {
      for (const p of targets) {
        try {
          await runCommand({ type: 'remove_project', id: p.id })
          changed += 1
        } catch (e) {
          failed.push(`${p.name}: ${errorText(e)}`)
        }
      }
    } finally {
      quietRefreshes.current -= 1
      invalidateSiteProjectCaches()
      await refreshAll()
    }
    reportBulk('Hidden', changed, failed)
  }

  const bulkRemoveFromList = async () => {
    const ok = await confirmAction(
      `Remove ${pickedCount} project${pickedCount === 1 ? '' : 's'} from the list? They are removed from OpenLocalServer but files on disk are not touched.`,
      `Remove ${pickedCount} project${pickedCount === 1 ? '' : 's'} from list?`,
      'Remove from list',
    )
    if (!ok) return
    // Rows and the selection bar leave at once; one backend call, one refresh at the end.
    const targets = pickedProjects
    quietRefreshes.current += 1
    dropProjects(targets.map((p) => p.id))
    let changed = 0
    const failed: string[] = []
    try {
      const res = await runCommand({ type: 'ignore_projects', ids: targets.map((p) => p.id) })
      changed = res.type === 'count' ? res.count : targets.length
      if (changed < targets.length) failed.push(`${targets.length - changed} could not be removed`)
    } catch (e) {
      failed.push(errorText(e))
    } finally {
      quietRefreshes.current -= 1
      invalidateSiteProjectCaches()
      await refreshAll()
    }
    reportBulk('Removed from list', changed, failed)
  }

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

    dropProjects([p.id])
    try {
      await runCommand({ type: 'remove_project', id: p.id })
      invalidateSiteProjectCaches()
      notify.success(`Hidden "${p.name}" from projects`)
      await refreshAll()
    } catch (e) {
      notify.error(`Failed to hide project: ${errorText(e)}`)
      await refreshAll()
    }
  }

  const handleRemoveFromList = async (p: Project) => {
    const ok = await confirmAction(
      `Remove "${p.name}" from the list? Its folder and database entry are removed from OpenLocalServer but files on disk are not touched.`,
      `Remove ${p.name} from list?`,
      'Remove from list',
    )
    if (!ok) return
    // The row leaves at once; the backend (site config re-render, reload) finishes behind it.
    dropProjects([p.id])
    try {
      await runCommand({ type: 'ignore_project', id: p.id })
      invalidateSiteProjectCaches()
      notify.success(`Removed "${p.name}" from list`)
    } catch (e) {
      notify.error(`Failed to remove project: ${errorText(e)}`)
    } finally {
      await refreshAll()
    }
  }

  const menuFor = (p: Project): MenuItem[] => {
    const isHealthy = p.health?.status === 'healthy' || !p.health
    const linked = getProjectDomains(p)
    const hasSite = unconnectedProjectIds != null
      ? !unconnectedProjectIds.includes(p.id)
      : linked.length > 0
    const siteIgnored = siteIgnoredProjectIds.includes(p.id)

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
        label: siteIgnored ? 'Unignore site' : 'Create site from project',
        icon: <Globe className="size-4" />,
        disabled: (!siteIgnored && hasSite) || !isHealthy,
        hint: siteIgnored ? 'Remove .ols_ignore_site and restore this site' : hasSite ? 'Site already exists' : !isHealthy ? 'Folder missing' : undefined,
        onSelect: () => {
          if (siteIgnored) {
            void runCommand({ type: 'unignore_site', project_id: p.id }).then(async (result) => {
              if (result.type === 'ok') {
                await Promise.all([refreshProjects(), refreshDomains()])
                notify.success(`Restored the site for ${p.name}.`)
              }
            }).catch((error: unknown) => {
              notify.error(`Could not restore the site for ${p.name}.`, { details: String(error) })
            })
            return
          }
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
      ...(dockerizeMenu
        ? [
            {
              label: 'Dockerize…',
              icon: <DockerIcon className="size-4" />,
              disabled: !isHealthy,
              hint: isHealthy ? 'Make a Dockerfile and compose file for this project' : 'Folder missing',
              onSelect: () => setDockerizeFor(p.id),
            } as MenuItem,
          ]
        : []),
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
    <PageLayout
      mode="list"
      className="relative"
      header={
      <div>
        <h1 className="text-2xl font-bold tracking-tight text-foreground">Projects</h1>
        <p className="text-sm text-muted-foreground">
          Manage your project directories, organize them by project type, open them in your preferred code editors or terminal, and link them to local sites.
        </p>
      </div>

      }
      toolbar={
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={handleAddProjectFolder} className="cursor-pointer gap-2">
          <FolderPlus className="size-4" /> Add project folder
        </Button>
        <GitCloneButton defaultParent={effectiveParent} onDone={() => void refreshAll()} standalone={true} />
        <div className="ml-auto">
          <RootFolderControls view={view} onChange={setView} />
        </div>
      </div>
      }
    >
      {/* Main card */}
      <ScrollableList
        className="overflow-hidden"
        resetKey={`${query}|${typeTab}|${effectiveMissingOnly}`}
        onDragOver={(e) => {
          e.preventDefault()
          setIsDragOver(true)
        }}
        onDragLeave={() => setIsDragOver(false)}
        onDrop={(e) => {
          e.preventDefault()
          setIsDragOver(false)
        }}
        overlay={<>
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
        </>}
        toolbar={
          /* Top toolbar: Type tabs, missing filter, manage types, and search */
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
        }
        footer={total > 0 || pickedCount > 0 ? (
          <>
            {pickedCount > 0 && (
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-t border-border bg-[var(--dialog-surface)] px-4 py-2">
                <span className="whitespace-nowrap text-sm text-muted-foreground">{pickedCount} selected</span>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-8"
                  disabled={busyBulk || pickedWithoutSite.length === 0}
                  onClick={() => void bulkCreateSites()}
                  title={pickedWithoutSite.length === 0 ? 'Every selected project already has a site, or its folder is missing' : `Give ${pickedWithoutSite.length} project${pickedWithoutSite.length === 1 ? '' : 's'} its <folder>.<tld> site`}
                >
                  <Globe className="size-4" /> Create sites
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-8"
                  disabled={busyBulk || pickedHideable.length === 0}
                  onClick={() => void bulkHide()}
                  title={pickedHideable.length === 0 ? 'Every selected project is connected to a site' : 'Hide the selected projects that have no site (files stay on disk)'}
                >
                  <EyeOff className="size-4" /> Hide
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-8 text-destructive hover:bg-destructive/10"
                  disabled={busyBulk}
                  onClick={() => void bulkRemoveFromList()}
                  title="Remove the selected projects from the list (files stay on disk)"
                >
                  <X className="size-4" /> Remove from list
                </Button>
                <Button size="sm" variant="ghost" className="h-8" disabled={busyBulk} onClick={() => setPicked(new Set())}>
                  Clear
                </Button>
                {restCount > 0 && (
                  <button
                    type="button"
                    className="cursor-pointer self-start text-xs text-muted-foreground underline"
                    onClick={() => setPicked(new Set(filteredProjects.map((p) => p.id)))}
                  >
                    Select all {filteredProjects.length} matching {filteredProjects.length === 1 ? 'project' : 'projects'}
                  </button>
                )}
              </div>
            )}
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
          </>
        ) : undefined}
      >
          {/* Table */}
          <Table className="table-fixed" wrapperClassName={'overflow-visible border-0 rounded-none shadow-none'}>
            <TableHeader sticky>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    checked={allOnPagePicked}
                    onChange={(on) => pageProjects.forEach((p) => togglePicked(p.id, on))}
                    disabled={pageProjects.length === 0 || busyBulk}
                    label={allOnPagePicked ? 'Clear the selection on this page' : 'Select every project on this page'}
                  />
                </TableHead>
                <TableHead>Project</TableHead>
                <TableHead className="hidden w-36 whitespace-nowrap @2xl:table-cell">Type</TableHead>
                <TableHead className="hidden w-56 whitespace-nowrap @4xl:table-cell">Connected Site</TableHead>
                <TableHead className="w-24 whitespace-nowrap text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageProjects.length === 0 ? (
                <TableRow>
                  <TableSpanCell colSpan={5} className="h-32 text-center text-sm text-muted-foreground">
                    {allProjects.length === 0 && !projectsLoaded
                      ? (
                        <span className="inline-flex items-center gap-2" role="status">
                          <Spinner className="size-4" /> Loading projects…
                        </span>
                      )
                      : allProjects.length === 0
                      ? 'No projects added yet. Click "Add project folder", drag and drop a folder, or use the Windows Explorer context menu to add one.'
                      : query.trim()
                        ? `No projects matching "${query.trim()}".`
                        : effectiveMissingOnly
                          ? 'No projects with missing or offline directories.'
                          : typeTab !== 'all'
                            ? `No projects under the "${typeGroups.tabs.find((t) => t.id === typeTab)?.label ?? typeTab}" type.`
                            : 'No projects found.'}
                  </TableSpanCell>
                </TableRow>
              ) : (
                pageProjects.map((p, rowIndex) => {
                  const isHealthy = p.health?.status === 'healthy' || !p.health
                  const isMenuOpen = activeMenu?.id === p.id
                  const linkedDomains = getProjectDomains(p)
                  const primaryDomain = linkedDomains[0]
                  const extraDomainsCount = linkedDomains.length > 1 ? linkedDomains.length - 1 : 0
                  const { id: typeId, label: typeLabel } = getProjectDisplayType(p, presets, customTypes)

                  const rootKey = rootKeyOf(p.path)
                  const startsGroup = view.group && (rowIndex === 0 || rootKeyOf(pageProjects[rowIndex - 1].path) !== rootKey)

                  return (
                    <Fragment key={p.id}>
                    {startsGroup && <RootGroupRow root={rootFolderOf(p.path)} count={rootCounts.get(rootKey) ?? 1} colSpan={5} />}
                    <TableRow
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
                      {/* Select */}
                      <TableCell className="h-[88px] w-10 py-1.5">
                        <Checkbox
                          checked={picked.has(p.id)}
                          onChange={(on) => togglePicked(p.id, on)}
                          disabled={busyBulk}
                          label={`Select ${p.name}`}
                        />
                      </TableCell>

                      {/* Project Col */}
                      <TableCell className="h-[88px] min-w-0 py-1.5">
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
                              <MiddleTruncate className="max-w-md flex-initial font-mono" text={p.path} />
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
                            <div className="mt-1.5 flex h-4 min-w-0 items-center gap-2 overflow-hidden">
                              {/* Type / Connected Site columns hide at narrow widths: their info moves here. */}
                              <span className="flex shrink-0 items-center gap-1 text-[10px] text-muted-foreground @2xl:hidden" title={`Type: ${typeLabel}`}>
                                <TechIcon id={typeId} className="size-3" />
                                {typeLabel}
                              </span>
                              {primaryDomain && (
                                <button
                                  type="button"
                                  className="flex min-w-0 shrink items-center gap-1 font-mono text-[10px] text-primary hover:underline cursor-pointer @4xl:hidden"
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    openProject({ id: p.id, site: primaryDomain.hostname })
                                    onNavigate('sites')
                                  }}
                                  title={`Open ${primaryDomain.hostname} in Sites${extraDomainsCount > 0 ? ` (+${extraDomainsCount} more)` : ''}`}
                                >
                                  <Globe className="size-3 shrink-0" />
                                  <span className="truncate">{primaryDomain.hostname}</span>
                                  {extraDomainsCount > 0 && <span className="shrink-0">+{extraDomainsCount}</span>}
                                </button>
                              )}
                              <span className={cn(
                                'inline-flex h-4 shrink-0 items-center gap-1 rounded px-1.5 text-[10px] leading-none',
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
                      <TableCell className="hidden h-[88px] w-36 whitespace-nowrap py-1.5 px-3 @2xl:table-cell">
                        <div className="flex items-center gap-1.5">
                          <TechIcon id={typeId} className="size-4 shrink-0 text-muted-foreground" />
                          <span className="text-xs font-medium text-foreground">{typeLabel}</span>
                        </div>
                      </TableCell>

                      {/* Connected Site Col */}
                      <TableCell className="hidden h-[88px] w-56 whitespace-nowrap py-1.5 px-3 @4xl:table-cell">
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
                              className="mr-2 h-7 px-2 text-xs border-destructive/40 text-destructive hover:bg-destructive/10 cursor-pointer"
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
                    </Fragment>
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
      </ScrollableList>

      <DockerizeDialog open={dockerizeFor != null} projectId={dockerizeFor} onClose={() => setDockerizeFor(null)} />

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
    </PageLayout>
  )
}
