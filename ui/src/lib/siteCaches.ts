import { listen } from '@tauri-apps/api/event'

import { runCommand } from '@/core'
import { writeCaches } from '@/lib/cache'

/**
 * The caches that hold a copy of the site list and the project list. Pages paint these first
 * and refresh behind them, so one that outlives a delete or an edit shows a removed row.
 */
const DOMAIN_CACHE_KEYS = ['web_domains', 'projects_page_domains'] as const
const PROJECT_CACHE_KEYS = ['web_projects', 'projects_page_list'] as const

let rebuilding: Promise<void> | null = null

/**
 * Replaces the cached site and project lists with what the backend has now. The old copy is
 * overwritten rather than emptied, so a page that opens meanwhile still paints something true
 * instead of an empty table while the backend answers.
 */
export function rebuildSiteProjectCaches(): Promise<void> {
  rebuilding ??= (async () => {
    try {
      const [projects, domains] = await Promise.all([runCommand({ type: 'list_projects' }), runCommand({ type: 'list_domains' })])
      if (projects.type === 'projects') writeCaches(PROJECT_CACHE_KEYS, projects.projects)
      if (domains.type === 'domains') writeCaches(DOMAIN_CACHE_KEYS, domains.domains)
    } catch {
      // the next successful refresh on a page rewrites them anyway
    } finally {
      rebuilding = null
    }
  })()
  return rebuilding
}

/** Call after anything that deletes or changes sites or projects. Runs in the background. */
export function invalidateSiteProjectCaches(): void {
  void rebuildSiteProjectCaches()
}

/** The backend says the project list changed (any window, the CLI, the API): rebuild the caches. */
export function installSiteCacheInvalidation(): () => void {
  let alive = true
  let unlisten: (() => void) | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  void listen('projects-changed', () => {
    // A scan can fire many of these in a row; one rebuild after the burst is enough.
    clearTimeout(timer)
    timer = setTimeout(() => {
      if (alive) invalidateSiteProjectCaches()
    }, 500)
  })
    .then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    .catch(() => undefined)
  return () => {
    alive = false
    clearTimeout(timer)
    unlisten?.()
  }
}
