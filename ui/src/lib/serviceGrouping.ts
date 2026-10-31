import type { ServiceStatus } from '@/core/types/base'

export type ServiceGroupId = 'web' | 'database' | 'cache' | 'mail' | 'other'

export interface ServiceGroupDefinition {
  id: ServiceGroupId
  label: string
}

export const SERVICE_GROUPS: readonly ServiceGroupDefinition[] = [
  { id: 'web', label: 'Web servers' },
  { id: 'database', label: 'Databases' },
  { id: 'cache', label: 'Cache & queues' },
  { id: 'mail', label: 'Mail' },
  { id: 'other', label: 'Other' },
] as const

/**
 * Maps a service's category (or fallback kind) to one of the 5 canonical group IDs.
 * Does NOT check service name or id: category comes strictly from the service definition.
 * Any unknown or missing category falls back to 'other'.
 */
export function getServiceGroupId(service: { category?: string | null; kind?: string | null }): ServiceGroupId {
  const raw = (service.category ?? service.kind ?? '').trim().toLowerCase()
  if (!raw) return 'other'

  switch (raw) {
    case 'web':
    case 'web_server':
    case 'web_servers':
    case 'web-server':
    case 'web-servers':
    case 'webserver':
    case 'webservers':
    case 'web server':
    case 'web servers':
      return 'web'

    case 'database':
    case 'databases':
    case 'db':
    case 'sql':
    case 'nosql':
    case 'document':
      return 'database'

    case 'cache':
    case 'queue':
    case 'queues':
    case 'cache_queue':
    case 'cache_queues':
    case 'cache-queue':
    case 'cache-queues':
    case 'cache & queues':
    case 'cache and queues':
      return 'cache'

    case 'mail':
    case 'email':
    case 'smtp':
      return 'mail'

    default:
      return 'other'
  }
}

export interface ServiceGroup<T extends { category?: string | null; kind?: string | null } = ServiceStatus> {
  id: ServiceGroupId
  label: string
  services: T[]
}

/**
 * Groups services in the Dashboard "Services" card by type in the strict order:
 * 1. Web servers
 * 2. Databases
 * 3. Cache & queues
 * 4. Mail
 * 5. Other
 *
 * Empty groups are hidden. Services inside each group preserve their original order.
 */
export function groupServices<T extends { category?: string | null; kind?: string | null } = ServiceStatus>(
  services: T[] | null | undefined
): ServiceGroup<T>[] {
  if (!services || services.length === 0) return []

  const result: ServiceGroup<T>[] = []

  for (const def of SERVICE_GROUPS) {
    const matching = services.filter((s) => getServiceGroupId(s) === def.id)
    if (matching.length > 0) {
      result.push({
        id: def.id,
        label: def.label,
        services: matching,
      })
    }
  }

  return result
}
