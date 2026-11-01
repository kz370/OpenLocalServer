import type { PluginManifest } from '@/core'

export interface RuntimeConsentItem {
  id: string
  name: string
  version: string
  host: string
  url: string
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
    const match = rawUrl.match(/^[a-zA-Z]+:\/\/([^/?#:]+)/)
    return match ? match[1] : ''
  }
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
    return {
      id: r.id,
      name: r.name,
      version: r.version,
      host,
      url: r.url || '',
      explanation: 'Downloads only when you click Install on the Runtimes page. Verified against a fixed SHA-256 checksum.',
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
  let closingSentence = 'Once turned on, this plugin is active. You can turn this plugin off at any time from the Plugins page.'
  if (runtimes.length > 0) {
    closingSentence =
      'Once turned on, runtimes appear on the Runtimes page (nothing downloads until you click Install). You can turn this plugin off at any time from the Plugins page.'
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
