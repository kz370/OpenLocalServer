import { runCommand } from '@/core'

/** Cached default TLD (domains.default_tld setting, default: 'local'). */
let cachedDefaultTld: string | null = null

/** Returns the user-configured default TLD (e.g. 'local', 'test', 'localhost', or a custom value). */
export async function getDefaultTld(): Promise<string> {
  if (cachedDefaultTld !== null) return cachedDefaultTld
  try {
    const r = await runCommand({ type: 'get_setting', key: 'domains.default_tld' })
    const val = r.type === 'setting' && typeof r.value === 'string' ? r.value.trim().replace(/^\./, '') : ''
    cachedDefaultTld = val || 'local'
  } catch {
    cachedDefaultTld = 'local'
  }
  return cachedDefaultTld
}

/** Invalidates the cached TLD so the next call to getDefaultTld() re-fetches from the backend. */
export function invalidateDefaultTldCache(): void {
  cachedDefaultTld = null
}

/** Cached default sites directory (<install>\sites or quickapps.projects_dir). */
let cachedSitesDir: string | null = null

/**
 * Returns the default sites folder in the installation directory
 * (or user override from `quickapps.projects_dir`).
 */
export async function getDefaultSitesDir(): Promise<string> {
  if (cachedSitesDir) return cachedSitesDir
  try {
    const override = await runCommand({ type: 'get_setting', key: 'quickapps.projects_dir' })
    const custom = override.type === 'setting' && typeof override.value === 'string' ? override.value.trim() : ''
    if (custom) {
      cachedSitesDir = custom
      return custom
    }
    const sites = await runCommand({ type: 'get_setting', key: 'paths.sites_dir' })
    if (sites.type === 'setting' && typeof sites.value === 'string' && sites.value.trim()) {
      cachedSitesDir = sites.value.trim()
      return cachedSitesDir
    }
  } catch {
    // Keep fallback below when backend unreachable
  }
  return ''
}

/**
 * Lowercase, alphanumerics and single dashes — e.g. "My Shop_2" -> "my-shop-2".
 * Matches ols-core slugify function (§48).
 */
export function slugify(name: string): string {
  let out = ''
  let lastDash = true
  for (const c of name) {
    if (/[a-zA-Z0-9]/.test(c)) {
      out += c.toLowerCase()
      lastDash = false
    } else if (!lastDash) {
      out += '-'
      lastDash = true
    }
  }
  return out.replace(/-+$/, '')
}

/**
 * Derives a folder name from a domain or project name.
 * e.g. "shop.test" -> "shop", "my-app.local" -> "my-app", "api.shop.test" -> "api.shop".
 * Cleans illegal Windows path characters and protocol prefixes.
 */
export function domainToFolderName(domain: string): string {
  let clean = domain.trim().toLowerCase()
  clean = clean.replace(/^https?:\/\//, '').replace(/\/.*$/, '')
  // Strip trailing .test, .local, .localhost, or partially typed extensions like .t, .te, .tes
  clean = clean.replace(/\.((test|local|localhost|dev\.test|t|te|tes|l|lo|loc|loca)$|$)/i, '')
  clean = clean.replace(/[\\/:*?"<>|]/g, '-').replace(/\s+/g, '-').replace(/^-+|-+$/g, '')
  return clean
}

/**
 * Derives a domain name from a folder or project name.
 * e.g. "shop" -> "shop.local", "my-site" -> "my-site.local".
 * Pass `tld` (e.g. from `getDefaultTld()`) to use a specific TLD; defaults to 'local'.
 */
export function folderNameToDomain(name: string, tld = 'local'): string {
  const clean = name.trim().toLowerCase().replace(/[\\/:*?"<>|]/g, '-').replace(/\s+/g, '-').replace(/^-+|-+$/g, '')
  if (!clean) return ''
  if (/\.(test|local|localhost)$/i.test(clean)) return clean
  return `${clean}.${tld}`
}

/**
 * Combines a parent directory and a folder name into a valid path using Windows backslashes.
 */
export function buildSitePath(parentDir: string, folderName: string): string {
  if (!parentDir) return folderName
  const sep = parentDir.includes('/') && !parentDir.includes('\\') ? '/' : '\\'
  const trimmedParent = parentDir.replace(/[\\/]+$/, '')
  return folderName ? `${trimmedParent}${sep}${folderName}` : trimmedParent
}
