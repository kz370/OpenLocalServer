// Cross-page navigation for the command palette and search: open a site or project (optionally on
// one of its tool tabs) without threading callbacks through every page.

export type ProjectTab = 'environment' | 'terminal' | 'env' | 'git' | 'workers' | 'snapshots' | 'repair' | 'mail' | 'composer' | 'node' | 'python' | 'xdebug'

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
