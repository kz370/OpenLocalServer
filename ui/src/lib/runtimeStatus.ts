export interface RuntimeVersionRow {
  key: string
  version: string
  kind: 'managed' | 'custom'
  installed: boolean
  isDefault?: boolean
}

export interface RuntimeStatusOptions {
  rows: RuntimeVersionRow[]
  runtimeId?: string
  isChecking?: boolean
  isError?: boolean
}

export type RuntimeStatusKind =
  | 'checking'
  | 'error'
  | 'not_installed'
  | 'up_to_date'
  | 'update_available'

export interface RuntimeStatusResult {
  totalInstalled: number
  managedInstalledCount: number
  customInstalledCount: number
  defaultVersion: string | null
  hasDefaultBadge: boolean
  installableCount: number
  highestInstalledVersion: string | null
  latestAvailableVersion: string | null
  targetUpdateVersion: string | null
  hasUpdate: boolean
  isMajorUpgrade: boolean
  statusKind: RuntimeStatusKind
  statusLabel: string
}

export function parseVersionKey(v: string): number[] {
  const clean = v.trim().replace(/^v/i, '')
  return clean.split(/[-.+]/).map((p) => {
    const n = parseInt(p, 10)
    return Number.isFinite(n) ? n : 0
  })
}

export function compareVersionsDesc(a: string, b: string): number {
  const ka = parseVersionKey(a)
  const kb = parseVersionKey(b)
  for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
    const d = (kb[i] ?? 0) - (ka[i] ?? 0)
    if (d !== 0) return d
  }
  return b.localeCompare(a)
}

/** Check if a version string represents an alpha, beta, rc, dev, preview, or canary build. */
export function isPrerelease(version: string): boolean {
  if (!version) return false
  return /(?:[-._](?:alpha|beta|rc|dev|preview|canary|nightly|ea)|(?:alpha|beta|rc|dev)(?:\d+|$))/i.test(version)
}

/** Node.js LTS release lines use even major versions >= 4 (e.g. 18.x, 20.x, 22.x). */
export function isNodeLts(version: string): boolean {
  if (isPrerelease(version)) return false
  const key = parseVersionKey(version)
  const major = key[0] ?? 0
  return major >= 4 && major % 2 === 0
}

/** Check if upgrading from `fromVersion` to `toVersion` crosses a major semver boundary. */
export function isMajorUpgrade(fromVersion: string | null, toVersion: string | null): boolean {
  if (!fromVersion || !toVersion) return false
  const fromMajor = parseVersionKey(fromVersion)[0] ?? 0
  const toMajor = parseVersionKey(toVersion)[0] ?? 0
  return toMajor > fromMajor
}

export function formatAvailableVersions(count: number): string {
  if (count <= 0) return ''
  return `${count} ${count === 1 ? 'version' : 'versions'} available`
}

export function getRuntimeStatus({
  rows,
  runtimeId,
  isChecking = false,
  isError = false,
}: RuntimeStatusOptions): RuntimeStatusResult {
  const managedRows = rows.filter((r) => r.kind === 'managed')
  const managedInstalled = managedRows.filter((r) => r.installed)
  const customInstalled = rows.filter((r) => r.kind === 'custom')
  const totalInstalled = managedInstalled.length + customInstalled.length

  const defaultRow = managedInstalled.find((r) => r.isDefault)
  const defaultVersion = defaultRow?.version ?? (managedInstalled[0]?.version || null)
  const hasDefaultBadge = totalInstalled > 1 ? !!defaultVersion : !!defaultRow?.isDefault

  const uninstalledManaged = managedRows.filter((r) => !r.installed)
  const installableCount = uninstalledManaged.length

  // Find latest available version across all known managed catalog versions (including pre-releases)
  const sortedCatalog = managedRows
    .map((r) => r.version)
    .sort(compareVersionsDesc)
  const latestAvailableVersion = sortedCatalog[0] ?? null

  // Find highest installed version
  const installedVersionStrings = [
    ...managedInstalled.map((r) => r.version),
    ...customInstalled.map((r) => r.version),
  ]
  const sortedInstalled = installedVersionStrings.sort(compareVersionsDesc)
  const highestInstalledVersion = sortedInstalled[0] ?? null

  // Find latest stable target for updates (never an alpha, beta, or RC as default target)
  // For Node.js, prefer LTS stable releases over odd-numbered current releases
  let targetUpdateVersion: string | null = null
  let hasUpdate = false

  if (totalInstalled > 0 && highestInstalledVersion) {
    if (runtimeId === 'node') {
      const allLts = managedRows
        .map((r) => r.version)
        .filter((v) => isNodeLts(v))
        .sort(compareVersionsDesc)
      const latestLts = allLts[0] ?? null
      if (latestLts && compareVersionsDesc(latestLts, highestInstalledVersion) < 0) {
        // A newer LTS version exists and is uninstalled
        targetUpdateVersion = latestLts
        hasUpdate = true
      }
    } else {
      const stableUninstalled = uninstalledManaged
        .map((r) => r.version)
        .filter((v) => !isPrerelease(v))
        .sort(compareVersionsDesc)
      const newestStable = stableUninstalled[0] ?? null
      if (newestStable && compareVersionsDesc(newestStable, highestInstalledVersion) < 0) {
        targetUpdateVersion = newestStable
        hasUpdate = true
      }
    }
  }

  const isMajor = hasUpdate && isMajorUpgrade(highestInstalledVersion, targetUpdateVersion)

  let statusKind: RuntimeStatusKind
  let statusLabel: string

  if (isChecking && rows.length === 0) {
    statusKind = 'checking'
    statusLabel = 'Checking…'
  } else if (isError && rows.length === 0) {
    statusKind = 'error'
    statusLabel = 'Could not check'
  } else if (totalInstalled === 0) {
    statusKind = 'not_installed'
    statusLabel = 'Not installed'
  } else if (hasUpdate && targetUpdateVersion) {
    statusKind = 'update_available'
    statusLabel = 'Update available'
  } else {
    statusKind = 'up_to_date'
    statusLabel = 'Up to date'
  }

  return {
    totalInstalled,
    managedInstalledCount: managedInstalled.length,
    customInstalledCount: customInstalled.length,
    defaultVersion,
    hasDefaultBadge,
    installableCount,
    highestInstalledVersion,
    latestAvailableVersion,
    targetUpdateVersion,
    hasUpdate,
    isMajorUpgrade: isMajor,
    statusKind,
    statusLabel,
  }
}
