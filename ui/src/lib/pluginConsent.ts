import type { PluginManifest } from '@/core'

export interface RuntimeConsentItem {
  id: string
  name: string
  version: string
  host: string
  source: string
  url: string
  sizeText: string | null
  explanation: string
}

export interface DetectionConsentItem {
  id: string
  name: string
  markers: string[]
}

export interface DetectionConsentSection {
  items: DetectionConsentItem[]
  explanation: string
}

export interface QuickAppConsentSection {
  folder: string
  explanation: string
}

export interface HealthCheckConsentItem {
  id: string
  name: string
  kind: string
  target: string
  explanation: string
}

export interface PluginConsentSummary {
  runtimes: RuntimeConsentItem[]
  detections: DetectionConsentSection | null
  quickApps: QuickAppConsentSection | null
  healthChecks: HealthCheckConsentItem[]
  rawPermissions: string[]
  closingSentence: string
}

/**
 * Safely extracts the domain/hostname from a download URL.
 */
export function extractHostname(rawUrl: string): string {
  if (!rawUrl) return ''
  try {
    const url = new URL(rawUrl)
    return url.hostname || ''
  } catch {
    const match = rawUrl.match(/^[a-zA-Z]+:\/\/(?:[^@]+@)?([^/?#:]+)/)
    return match ? match[1] : ''
  }
}

/**
 * Formats a download source from an actual runtime URL into a recognizable,
 * human-readable source prefix without exposing query parameters or credentials:
 * - For github.com: shows host/owner/repo (e.g. "github.com/oven-sh/bun")
 * - For other hosts: shows host + first path segment (e.g. "go.dev/dl", "builds.dotnet.microsoft.com/dotnet")
 *   or just host if path is empty.
 * - Non-default ports are preserved (e.g. "custom.mirror.org:8443/runtimes"), default ports (80/443) are omitted.
 */
export function formatDownloadSource(rawUrl: string): string {
  if (!rawUrl) return ''
  try {
    const url = new URL(rawUrl)
    const hostname = url.hostname.toLowerCase()

    let hostWithPort = hostname
    if (url.port) {
      const isDefaultHttp = url.protocol === 'http:' && url.port === '80'
      const isDefaultHttps = url.protocol === 'https:' && url.port === '443'
      if (!isDefaultHttp && !isDefaultHttps) {
        hostWithPort = `${hostname}:${url.port}`
      }
    }

    const segments = url.pathname.split('/').filter(Boolean)

    if (hostname === 'github.com') {
      if (segments.length >= 2) {
        return `${hostWithPort}/${segments[0]}/${segments[1]}`
      }
      if (segments.length === 1) {
        return `${hostWithPort}/${segments[0]}`
      }
      return hostWithPort
    }

    if (segments.length >= 1) {
      return `${hostWithPort}/${segments[0]}`
    }
    return hostWithPort
  } catch {
    const match = rawUrl.match(/^[a-zA-Z]+:\/\/(?:[^@]+@)?([^/?#]+)(?:\/([^/?#]+))?/)
    if (!match) return rawUrl
    return match[2] ? `${match[1]}/${match[2]}` : match[1]
  }
}

/**
 * Formats an optional download size into a friendly string (e.g. "about 45 MB").
 * Returns null if size is undefined, null, or empty.
 */
export function formatSize(size?: number | string | null): string | null {
  if (size === undefined || size === null || size === '') return null
  if (typeof size === 'string') {
    const trimmed = size.trim()
    if (!trimmed) return null
    if (trimmed.toLowerCase().startsWith('about')) return trimmed
    return `about ${trimmed}`
  }
  if (typeof size === 'number' && Number.isFinite(size) && size > 0) {
    if (size < 1024 * 1024) {
      return `about ${Math.round(size / 1024)} KB`
    }
    const mb = Math.round(size / (1024 * 1024))
    if (mb < 1024) {
      return `about ${mb} MB`
    }
    const gb = (size / (1024 * 1024 * 1024)).toFixed(1)
    return `about ${gb} GB`
  }
  return null
}

/**
 * Generates an objective, structured consent summary directly from
 * verified plugin manifest contributes & permissions data.
 * Does NOT rely on author-supplied free text to avoid misleading consent information.
 */
export function buildPluginConsentSummary(manifest: PluginManifest): PluginConsentSummary {
  const contributes = manifest.contributes || {
    runtimes: [],
    quick_apps: null,
    detections: [],
    health_checks: [],
  }

  // 1. Runtime downloads
  const runtimes: RuntimeConsentItem[] = (contributes.runtimes || []).map((r) => {
    const host = extractHostname(r.url || '') || 'unknown host'
    const source = formatDownloadSource(r.url || '') || host
    const sizeText = formatSize(r.size)
    return {
      id: r.id,
      name: r.name,
      version: r.version,
      host,
      source,
      url: r.url || '',
      sizeText,
      explanation: 'Only downloads when you click Install on the Runtimes page. Verified against a fixed SHA-256 checksum.',
    }
  })

  // 2. Project detections
  let detections: DetectionConsentSection | null = null
  if (contributes.detections && contributes.detections.length > 0) {
    const items: DetectionConsentItem[] = contributes.detections.map((d) => {
      const markers = [...(d.markers || [])]
      if (d.all && d.all.length > 0) {
        markers.push(...d.all.map((m) => `${m} (all required)`))
      }
      return {
        id: d.id,
        name: d.name,
        markers,
      }
    })
    detections = {
      items,
      explanation: 'Reads files only to recognise projects — nothing is modified.',
    }
  }

  // 3. Quick Apps
  let quickApps: QuickAppConsentSection | null = null
  if (contributes.quick_apps) {
    quickApps = {
      folder: contributes.quick_apps,
      explanation: 'Adds Quick App recipes from the plugin folder. Each recipe displays the commands it will run before executing.',
    }
  }

  // 4. Health checks
  const healthChecks: HealthCheckConsentItem[] = (contributes.health_checks || []).map((h) => ({
    id: h.id,
    name: h.name,
    kind: h.kind,
    target: `${h.kind.toUpperCase()} ${h.target}`,
    explanation: 'Checks whether local endpoints respond. Network access is restricted to these probes.',
  }))

  // 5. Raw permissions
  const rawPermissions = [...(manifest.permissions || [])]

  // 6. Plain-language closing sentence
  // Note: "Nothing downloads until you click Install" is kept in the Runtime downloads card only.
  let closingSentence = 'Once turned on, this plugin is active. You can turn this plugin off at any time from the Plugins page.'
  if (runtimes.length > 0) {
    closingSentence =
      'Once turned on, its runtimes appear on the Runtimes page. You can turn this plugin off at any time from the Plugins page.'
  }

  return {
    runtimes,
    detections,
    quickApps,
    healthChecks,
    rawPermissions,
    closingSentence,
  }
}
