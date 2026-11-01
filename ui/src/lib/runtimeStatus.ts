export interface RuntimeVersionRow {
  key: string
  version: string
  kind: 'managed' | 'custom'
  installed: boolean
  isDefault?: boolean
}

export interface RuntimeStatusOptions {
  rows: RuntimeVersionRow[]
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
  installableCount: number
  highestInstalledVersion: string | null
  latestAvailableVersion: string | null
  hasUpdate: boolean
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

export function formatAvailableVersions(count: number): string {
  if (count <= 0) return ''
  return `${count} ${count === 1 ? 'version' : 'versions'} available`
}

export function getRuntimeStatus({
  rows,
  isChecking = false,
  isError = false,
}: RuntimeStatusOptions): RuntimeStatusResult {
  const managedRows = rows.filter((r) => r.kind === 'managed')
  const managedInstalled = managedRows.filter((r) => r.installed)
  const customInstalled = rows.filter((r) => r.kind === 'custom')
  const totalInstalled = managedInstalled.length + customInstalled.length

  const defaultRow = managedInstalled.find((r) => r.isDefault)
  const defaultVersion = defaultRow?.version ?? (managedInstalled[0]?.version || null)

  const uninstalledManaged = managedRows.filter((r) => !r.installed)
  const installableCount = uninstalledManaged.length

  // Find latest available version across all known managed catalog versions
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

  // Check if a newer version is available compared to any installed version
  let hasUpdate = false
  if (totalInstalled > 0 && latestAvailableVersion && highestInstalledVersion) {
    hasUpdate = compareVersionsDesc(latestAvailableVersion, highestInstalledVersion) < 0
  }

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
  } else if (hasUpdate && latestAvailableVersion) {
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
    installableCount,
    highestInstalledVersion,
    latestAvailableVersion,
    hasUpdate,
    statusKind,
    statusLabel,
  }
}

