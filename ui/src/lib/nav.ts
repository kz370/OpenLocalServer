// Cross-page navigation for the command palette and search: open a site or project (optionally on
// one of its tool tabs) without threading callbacks through every page.

export type ProjectTab = 'environment' | 'terminal' | 'env' | 'git' | 'workers' | 'backups' | 'repair' | 'mail' | 'composer' | 'node' | 'python' | 'xdebug' | 'load'

export interface OpenProject {
  id: string
  tab?: ProjectTab
  /** A site's hostname: opens that site's settings. */
  site?: string
}

const EVENT = 'ols:open-project'
let pending: OpenProject | null = null

/** Asks the Sites page to show a site or project; remembered until that page picks it up. */
export function openProject(target: OpenProject) {
  pending = target
  window.dispatchEvent(new CustomEvent<OpenProject>(EVENT, { detail: target }))
}

/** For the Sites page: the request made before it mounted, if any. */
export function takePendingProject(): OpenProject | null {
  const p = pending
  pending = null
  return p
}

export function onOpenProject(fn: (p: OpenProject) => void): () => void {
  const handler = (e: Event) => {
    pending = null
    fn((e as CustomEvent<OpenProject>).detail)
  }
  window.addEventListener(EVENT, handler)
  return () => window.removeEventListener(EVENT, handler)
}

const ADD_SITE_PROJECT_EVENT = 'ols:add-site-project'
let pendingAddSiteProject: string | null = null

/** Opens the Add Site dialog with an existing project selected, including before Sites mounts. */
export function openAddSiteForProject(projectId: string) {
  pendingAddSiteProject = projectId
  window.dispatchEvent(new CustomEvent<string>(ADD_SITE_PROJECT_EVENT, { detail: projectId }))
}

export function takePendingAddSiteProject(): string | null {
  const projectId = pendingAddSiteProject
  pendingAddSiteProject = null
  return projectId
}

export function onOpenAddSiteForProject(fn: (projectId: string) => void): () => void {
  const handler = (e: Event) => {
    pendingAddSiteProject = null
    fn((e as CustomEvent<string>).detail)
  }
  window.addEventListener(ADD_SITE_PROJECT_EVENT, handler)
  return () => window.removeEventListener(ADD_SITE_PROJECT_EVENT, handler)
}

const SITES_FILTER_EVENT = 'ols:open-sites-filter'
let pendingSitesFilter: string | null = null

/** Asks the Sites page to switch to a specific tab filter (e.g. 'missing'); remembered until mounted. */
export function openSitesFilter(filter: string) {
  pendingSitesFilter = filter
  window.dispatchEvent(new CustomEvent<string>(SITES_FILTER_EVENT, { detail: filter }))
}

export function takePendingSitesFilter(): string | null {
  const f = pendingSitesFilter
  pendingSitesFilter = null
  return f
}

export function onOpenSitesFilter(fn: (filter: string) => void): () => void {
  const handler = (e: Event) => {
    pendingSitesFilter = null
    fn((e as CustomEvent<string>).detail)
  }
  window.addEventListener(SITES_FILTER_EVENT, handler)
  return () => window.removeEventListener(SITES_FILTER_EVENT, handler)
}

const PROJECTS_FILTER_EVENT = 'ols:open-projects-filter'
let pendingProjectsFilter: string | null = null

/** Asks the Projects page to apply a filter (e.g. 'missing'); remembered until mounted. */
export function openProjectsFilter(filter: string) {
  pendingProjectsFilter = filter
  window.dispatchEvent(new CustomEvent<string>(PROJECTS_FILTER_EVENT, { detail: filter }))
}

export function takePendingProjectsFilter(): string | null {
  const f = pendingProjectsFilter
  pendingProjectsFilter = null
  return f
}

export function onOpenProjectsFilter(fn: (filter: string) => void): () => void {
  const handler = (e: Event) => {
    pendingProjectsFilter = null
    fn((e as CustomEvent<string>).detail)
  }
  window.addEventListener(PROJECTS_FILTER_EVENT, handler)
  return () => window.removeEventListener(PROJECTS_FILTER_EVENT, handler)
}

const SETTINGS_SECTION_EVENT ='ols:open-settings-section'
let pendingSettingsSection: string | null = null

/** Asks the Settings page to show a specific section (e.g. 'general'); remembered until mounted. */
export function openSettingsSection(section: string) {
  pendingSettingsSection = section
  try {
    sessionStorage.setItem('ols:settings-section', section)
  } catch {}
  window.dispatchEvent(new CustomEvent<string>(SETTINGS_SECTION_EVENT, { detail: section }))
}

export function takePendingSettingsSection(): string | null {
  const s = pendingSettingsSection
  pendingSettingsSection = null
  return s
}

export function onOpenSettingsSection(fn: (section: string) => void): () => void {
  const handler = (e: Event) => {
    pendingSettingsSection = null
    fn((e as CustomEvent<string>).detail)
  }
  window.addEventListener(SETTINGS_SECTION_EVENT, handler)
  return () => window.removeEventListener(SETTINGS_SECTION_EVENT, handler)
}


