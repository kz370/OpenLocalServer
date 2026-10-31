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

