import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Bug, Check, ChevronDown, Download, FolderSearch, Puzzle, Settings2, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { PhpExtensionsDialog } from '@/components/PhpExtensionsDialog'
import { XdebugDialog } from '@/components/XdebugDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { TechIcon } from '@/components/TechIcon'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CatalogEntry, type CustomInstall, type Diagnostic, type RuntimeEvent, runCommand } from '@/core'
import { formatBytes } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'

/** Runtimes a user may already have somewhere else and want to point at. */
const LOCATABLE = ['php', 'node', 'python']
const ONLINE_CATALOGS = new Set(['nginx', 'node', 'mariadb', 'php', 'apache', 'composer', 'mongodb', 'postgres', 'redis'])
const ONLINE_CATALOG_CACHE_KEY = 'ols.runtime-catalogs'
const ONLINE_CATALOG_CACHE_TTL = 24 * 60 * 60 * 1000

interface CachedCatalog {
  fetchedAt: number
  versions: Array<{ name: string; version: string }>
}

type CatalogStatus = 'checking' | 'ready' | 'error'

function readCatalogCache(): Record<string, CachedCatalog> {
  try {
    const raw = localStorage.getItem(ONLINE_CATALOG_CACHE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, CachedCatalog>
    return Object.fromEntries(Object.entries(parsed).filter(([, value]) =>
      Number.isFinite(value?.fetchedAt) && value.fetchedAt <= Date.now() && Array.isArray(value.versions),
    ))
  } catch {
    return {}
  }
}

function versionKey(v: string): number[] {
  return v.split('.').map((p) => parseInt(p, 10) || 0)
}

function compareVersionsDesc(a: string, b: string): number {
  const ka = versionKey(a)
  const kb = versionKey(b)
  for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
    const d = (kb[i] ?? 0) - (ka[i] ?? 0)
    if (d !== 0) return d
  }
  return 0
}

interface Row {
  key: string
  version: string
  kind: 'managed' | 'custom'
  entry?: CatalogEntry
  custom?: CustomInstall
}

interface Group {
  id: string
  name: string
  rows: Row[]
}

export function RuntimesPage() {
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [progress, setProgress] = useState<Record<string, RuntimeEvent | undefined>>({})
  const [error, setError] = useState<Diagnostic | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [catalogRefreshing, setCatalogRefreshing] = useState(false)
  const [catalogStatus, setCatalogStatus] = useState<Record<string, CatalogStatus>>({})
  const [cachedCatalogIds, setCachedCatalogIds] = useState<Set<string>>(new Set())
  // Versions the backend actually listed (builtin + online). localStorage cache
  // entries are display-only until a refresh confirms them — installing a stale
  // cached version fails with "not in the current online or built-in version list".
  const [verifiedKeys, setVerifiedKeys] = useState<Set<string>>(new Set())
  // Install failure for the open dialog. Kept in dedicated state (not derived
  // from the progress map) so a catalog refresh can't wipe it mid-read.
  const [installError, setInstallError] = useState<{ id: string; version: string; message: string } | null>(null)

  const [customInstalls, setCustomInstalls] = useState<CustomInstall[]>([])
  const [manageId, setManageId] = useState<string | null>(null)
  const [installChoices, setInstallChoices] = useState<Record<string, string>>({})
  const [versionSearch, setVersionSearch] = useState('')
  const [customId, setCustomId] = useState('php')
  const [customLabel, setCustomLabel] = useState('')
  const [extVersion, setExtVersion] = useState<string | null>(null)
  const [xdebugVersion, setXdebugVersion] = useState<string | null>(null)
  // Render guard: coalesces progress bursts so a flooded event stream can't
  // lock the page. Backend already throttles emits; this caps re-renders ~4/s.
  const lastProgressPaint = useRef(0)

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') {
      setCatalog(res.entries)
      setVerifiedKeys(new Set(res.entries.map((e) => `${e.id}@${e.version}`)))
    }
    const custom = await runCommand({ type: 'list_custom_installs' })
    if (custom.type === 'custom_installs') setCustomInstalls(custom.entries)
    setLoading(false)
  }

  async function refreshOnlineCatalog(id: string) {
    setCatalogStatus((prev) => ({ ...prev, [id]: 'checking' }))
    try {
      const res = await runCommand({ type: 'refresh_runtime_catalog', id })
      if (res.type !== 'runtime_catalog') return
      const entries = res.entries.filter((entry) => entry.id === id)
      setCatalog((prev) => [...prev.filter((entry) => entry.id !== id), ...entries])
      setVerifiedKeys((prev) => new Set([...prev, ...entries.map((e) => `${e.id}@${e.version}`)]))
      setCatalogStatus((prev) => ({ ...prev, [id]: 'ready' }))
      setCachedCatalogIds((prev) => new Set([...prev, id]))
      try {
        const cache = readCatalogCache()
        cache[id] = { fetchedAt: Date.now(), versions: entries.map(({ name, version }) => ({ name, version })) }
        localStorage.setItem(ONLINE_CATALOG_CACHE_KEY, JSON.stringify(cache))
      } catch {
        // The current runtime list remains usable when browser storage is unavailable.
      }
      return entries
    } catch (err) {
      setCatalogStatus((prev) => ({ ...prev, [id]: 'error' }))
      throw err
    }
  }

  async function guarded(fn: () => Promise<void>) {
    setError(null)
    setNotice(null)
    try {
      await fn()
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  const addCustomInstall = () =>
    guarded(async () => {
      const picked = await open({
        multiple: false,
        directory: false,
        title: `Locate ${customId}${customLabel ? ` ${customLabel}` : ''}`,
        filters: [{ name: 'Executable', extensions: ['exe'] }],
      })
      if (!picked || Array.isArray(picked)) return
      await runCommand({ type: 'set_custom_install', id: customId, label: customLabel, path: picked })
      setCustomLabel('')
      await refresh()
    })

  const scanPhpFolder = () =>
    guarded(async () => {
      const picked = await open({ directory: true, multiple: false, title: 'Folder containing your PHP versions' })
      if (!picked || Array.isArray(picked)) return
      const res = await runCommand({ type: 'scan_php_folder', dir: picked })
      if (res.type === 'php_scan') {
        setNotice(
          res.found.length === 0
            ? 'No PHP installs found there. A PHP folder needs both php.exe and php-cgi.exe.'
            : `Added ${res.found.length} PHP version${res.found.length === 1 ? '' : 's'}: ${res.found.map((f) => f.version).join(', ')}`,
        )
      }
      await refresh()
    })

  const removeCustomInstall = (entry: CustomInstall) =>
    guarded(async () => {
      const name = `${entry.id.toUpperCase()} ${entry.label || ''}`.trim()
      if (!(await confirmAction(`Remove ${name} from the list?\n\nThe files at ${entry.path} are not deleted.`))) return
      await runCommand({ type: 'remove_custom_install', id: entry.id, label: entry.label })
      await refresh()
    })

  useEffect(() => {
    let active = true
    void (async () => {
      try {
        await refresh()
        if (!active) return
        const cached = readCatalogCache()
        setCachedCatalogIds(new Set(Object.keys(cached)))
        const cachedEntries = Object.entries(cached).flatMap(([id, value]) =>
          value.versions.map(({ name, version }) => ({ id, name, version, installed: false, is_default: false, system: null })),
        )
        setCatalog((prev) => {
          const known = new Set(prev.map((entry) => `${entry.id}@${entry.version}`))
          return [...prev, ...cachedEntries.filter((entry) => !known.has(`${entry.id}@${entry.version}`))]
        })
        const staleIds = [...ONLINE_CATALOGS].filter((id) =>
          !cached[id] || Date.now() - cached[id].fetchedAt >= ONLINE_CATALOG_CACHE_TTL,
        )
        setCatalogStatus(Object.fromEntries([...ONLINE_CATALOGS].map((id) => [
          id,
          staleIds.includes(id) ? 'checking' as const : 'ready' as const,
        ])))
        await Promise.all(staleIds.map(async (id) => {
          try { await refreshOnlineCatalog(id) } catch { /* Keep cached or built-in versions visible offline. */ }
        }))
      } catch {
        setLoading(false)
      }
    })()
    const unlisten = listen<RuntimeEvent>('runtime-event', (event) => {
      const e = event.payload
      // Terminal states always paint. Progress paints at most every 250ms.
      if (e.kind === 'progress') {
        const now = Date.now()
        if (now - lastProgressPaint.current < 250) return
        lastProgressPaint.current = now
      }
      setProgress((prev) => ({ ...prev, [`${e.id}@${e.version}`]: e }))
      if (e.kind === 'failed') setInstallError({ id: e.id, version: e.version, message: e.message })
      if (e.kind === 'installed') setInstallError(null)
      if (e.kind === 'installed' || e.kind === 'failed') void refresh()
    })
    return () => {
      active = false
      void unlisten.then((f) => f())
    }
  }, [])

  const install = (entry: CatalogEntry) =>
    guarded(async () => {
      setInstallError(null)
      await runCommand({ type: 'install_runtime', id: entry.id, version: entry.version })
    })

  const chooseDefault = (entry: CatalogEntry) =>
    guarded(async () => {
      await runCommand({ type: 'set_setting', key: `runtime.${entry.id}.global`, value: entry.version })
      await refresh()
      setNotice(entry.id === 'mariadb'
        ? `MariaDB ${entry.version} is now the default. Each MariaDB series uses a separate data folder, so its databases are separate.`
        : ['nginx', 'apache', 'caddy'].includes(entry.id)
          ? `${entry.name} ${entry.version} will be used when you next start the web server.`
          : entry.id === 'php'
            ? `PHP ${entry.version} is now the default. Site/project pins still take precedence; apply web config to update unpinned PHP sites.`
            : ['node', 'python'].includes(entry.id)
              ? `${entry.name} ${entry.version} is now the default. A project's pinned version still takes precedence.`
            : `${entry.name} ${entry.version} will be used the next time its service starts.`)
    })

  const removeRuntime = (entry: CatalogEntry) =>
    guarded(async () => {
      if (!(await confirmAction(`Remove the managed ${entry.name} ${entry.version} files? Projects or sites pinned to this version will need another version selected or this version reinstalled. Project files and MariaDB data stay in place.`, 'Remove version'))) return
      await runCommand({ type: 'remove_runtime', id: entry.id, version: entry.version })
      await refresh()
    })

  const openVersions = (group: Group) => {
    setError(null)
    setNotice(null)
    setInstallError(null)
    setManageId(group.id)
    setVersionSearch('')
    setInstallChoices((prev) => ({ ...prev, [group.id]: group.rows.find((r) => r.kind === 'managed' && !r.entry?.installed)?.version ?? '' }))
    if (!ONLINE_CATALOGS.has(group.id)) {
      setCatalogRefreshing(false)
      return
    }
    // Always verify against the vendor list on open: the localStorage cache
    // survives daemon restarts but the backend's online list does not, so a
    // cached-only choice would fail validation at install time.
    if (catalogStatus[group.id] === 'checking') {
      setCatalogRefreshing(false)
      return
    }
    setCatalogRefreshing(true)
    void refreshOnlineCatalog(group.id)
      .then((entries) => {
        if (!entries) return
        const newest = entries
          .filter((entry) => !entry.installed)
          .sort((a, b) => compareVersionsDesc(a.version, b.version))[0]
        if (newest) setInstallChoices((prev) => ({ ...prev, [group.id]: newest.version }))
      })
      .catch((err) => setError(err as Diagnostic))
      .finally(() => setCatalogRefreshing(false))
  }

  const groups = useMemo<Group[]>(() => {
    const byId = new Map<string, Group>()
    for (const entry of catalog) {
      const g = byId.get(entry.id) ?? { id: entry.id, name: entry.name, rows: [] }
      g.rows.push({ key: `${entry.id}@${entry.version}`, version: entry.version, kind: 'managed', entry })
      byId.set(entry.id, g)
    }
    for (const c of customInstalls) {
      const g = byId.get(c.id) ?? { id: c.id, name: c.id.toUpperCase(), rows: [] }
      g.rows.push({ key: `custom:${c.id}:${c.label}`, version: c.label || 'custom', kind: 'custom', custom: c })
      byId.set(c.id, g)
    }
    const list = [...byId.values()]
    for (const g of list) g.rows.sort((a, b) => compareVersionsDesc(a.version, b.version))
    // Groups with something installed first, then alphabetical.
    const has = (g: Group) => g.rows.some((r) => r.kind === 'custom' || r.entry?.installed)
    return list.sort((a, b) => Number(has(b)) - Number(has(a)) || a.name.localeCompare(b.name))
  }, [catalog, customInstalls])

  const managedGroup = groups.find((g) => g.id === manageId) ?? null
  // Only backend-confirmed versions are installable. Cached-only entries render
  // in counts but can't install until a refresh verifies them.
  const verifiedManaged = managedGroup?.rows.filter((r) => r.kind === 'managed' && verifiedKeys.has(r.key)) ?? []
  const installable = verifiedManaged.filter((r) => !r.entry?.installed)
  const unverifiedCount = managedGroup?.rows.filter((r) => r.kind === 'managed' && !r.entry?.installed && !verifiedKeys.has(r.key)).length ?? 0
  const filteredOptions = verifiedManaged.filter((row) => row.version.toLowerCase().includes(versionSearch.trim().toLowerCase()))
  const optionsByMajor = new Map<string, Row[]>()
  for (const row of filteredOptions) {
    const major = row.version.match(/^\d+/)?.[0] ?? 'Other'
    const key = major === 'Other' ? major : `${major}.x`
    optionsByMajor.set(key, [...(optionsByMajor.get(key) ?? []), row])
  }
  const dialogChecking = !!managedGroup && (catalogRefreshing || catalogStatus[managedGroup.id] === 'checking')
  const dialogRefreshFailed = !!managedGroup && !dialogChecking && installable.length === 0 && unverifiedCount === 0 && catalogStatus[managedGroup.id] === 'error'
  const filteredInstallable = installable.filter((row) => row.version.toLowerCase().includes(versionSearch.trim().toLowerCase()))
  const rememberedChoice = installChoices[manageId ?? '']
  const selectedInstall = rememberedChoice && installable.some((r) => r.version === rememberedChoice) ? rememberedChoice : installable[0]?.version ?? ''
  const selectedInstallVisible = filteredInstallable.some((r) => r.version === selectedInstall)
  const selectedInstallEvent = progress[`${manageId}@${selectedInstall}`]
  const installingChoice = selectedInstallEvent?.kind === 'progress'
  const installedRows = managedGroup?.rows.filter((r) => r.kind === 'custom' || r.entry?.installed) ?? []
  const managedInstalledCount = managedGroup?.rows.filter((r) => r.kind === 'managed' && r.entry?.installed).length ?? 0

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-1">
        <h1 className="text-xl font-semibold tracking-tight">Runtimes</h1>
        <p className="max-w-2xl text-[13px] leading-relaxed text-muted-foreground">
            Install versions side by side, then choose a default for new projects and services. Vendor SHA-256 checksums are verified when published.
        </p>
      </div>

      <div className="rounded-xl border border-border/60 bg-card/40 p-4">
        <div className="flex flex-col gap-0.5">
          <h2 className="text-sm font-semibold leading-none">Add a runtime</h2>
          <p className="text-xs text-muted-foreground">Use an existing installation or register a custom executable.</p>
        </div>
        <div className="mt-3.5 grid gap-3 lg:grid-cols-2">
          <div className="flex min-w-0 flex-col justify-between gap-3 rounded-lg border border-border/60 bg-background/40 p-3.5 sm:flex-row sm:items-center">
            <div className="min-w-0">
              <p className="text-[13px] font-medium leading-none">Add existing versions</p>
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">Scan a folder for installed runtimes.</p>
            </div>
            <Button size="sm" variant="secondary" className="h-8 shrink-0 text-xs" onClick={scanPhpFolder} title="Adds every PHP version found in the folder you pick">
              <FolderSearch /> Add folder
            </Button>
          </div>
          <div className="flex min-w-0 flex-col justify-center gap-2.5 rounded-lg border border-border/60 bg-background/40 p-3.5">
            <div className="min-w-0">
              <p className="text-[13px] font-medium leading-none">Add one executable</p>
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">Choose its runtime and version, then locate the file.</p>
            </div>
            <div className="flex min-w-0 flex-wrap items-center gap-2 sm:flex-nowrap">
              <div className="relative w-24 shrink-0">
                <select
                  aria-label="Executable runtime"
                  value={customId}
                  onChange={(e) => setCustomId(e.target.value)}
                  className="h-8 w-full appearance-none rounded-md border border-transparent bg-input/60 py-1 pl-2.5 pr-8 text-[13px] focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
                >
                  {LOCATABLE.map((id) => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
                </select>
                <ChevronDown aria-hidden="true" className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              </div>
              <Input
                aria-label="Executable version"
                value={customLabel}
                onChange={(e) => setCustomLabel(e.target.value)}
                placeholder="Version (e.g. 8.1.2)"
                className="h-8 min-w-28 flex-1 text-[13px]"
              />
              <Button size="sm" variant="secondary" className="h-8 shrink-0 text-xs" onClick={addCustomInstall}>
                <FolderSearch /> Locate
              </Button>
            </div>
          </div>
        </div>
      </div>

      {notice && !managedGroup && <p className="text-sm text-muted-foreground">{notice}</p>}

      <Table wrapperClassName="rounded-xl border-border/60">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-8 text-[11px] font-medium uppercase tracking-wider">Runtime</TableHead>
            <TableHead className="h-8 text-[11px] font-medium uppercase tracking-wider">Installed</TableHead>
            <TableHead className="h-8 text-[11px] font-medium uppercase tracking-wider">Status</TableHead>
            <TableHead className="h-8 text-right text-[11px] font-medium uppercase tracking-wider">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading &&
            Array.from({ length: 6 }, (_, i) => (
              <TableRow key={i} className="hover:bg-transparent">
                {[0, 1, 2, 3].map((c) => (
                  <TableCell key={c}>
                    <div className="h-4 animate-pulse rounded bg-muted" style={{ width: '60%' }} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          {loading && (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={4} className="text-center text-xs text-muted-foreground">
                <Spinner className="mr-1.5 inline size-3.5 align-text-bottom" /> Looking for runtimes on this computer…
              </TableCell>
            </TableRow>
          )}
          {!loading && groups.map((g) => {
            const managedInstalled = g.rows.filter((r) => r.kind === 'managed' && r.entry?.installed)
            const customInstalled = g.rows.filter((r) => r.kind === 'custom')
            const defaultVersion = managedInstalled.find((r) => r.entry?.is_default)?.version
            const availableCount = g.rows.filter((r) => r.kind === 'managed' && !r.entry?.installed).length
            const isChecking = ONLINE_CATALOGS.has(g.id) && catalogStatus[g.id] === 'checking'
            const isError = ONLINE_CATALOGS.has(g.id) && catalogStatus[g.id] === 'error' && !cachedCatalogIds.has(g.id)
            const hasUpdate = !isError && availableCount > 0
            return (
              <TableRow key={g.id} className="hover:bg-muted/30">
                <TableCell className="py-2.5">
                  <span className="flex items-center gap-2 text-[13px] font-medium"><TechIcon id={g.id} className="size-4 opacity-90" />{g.name}</span>
                </TableCell>
                <TableCell className="py-2.5">
                  {managedInstalled.length + customInstalled.length === 0 ? <span className="text-xs text-muted-foreground">None</span> : (
                    <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
                      {defaultVersion && <>
                        <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">{defaultVersion}</code>
                        <Badge variant="secondary" className="px-1.5 text-[11px] font-normal">Default</Badge>
                      </>}
                      <span className="text-xs text-muted-foreground">{managedInstalled.length + customInstalled.length} installed</span>
                    </span>
                  )}
                </TableCell>
                <TableCell className="py-2.5">
                  <span className="flex items-center gap-1.5 text-xs">
                    {isChecking && !cachedCatalogIds.has(g.id)
                      ? <><span className="size-1.5 animate-pulse rounded-full bg-muted-foreground" /><span className="text-muted-foreground">Checking…</span></>
                      : isError
                        ? <><span className="size-1.5 rounded-full bg-muted-foreground/60" /><span className="text-muted-foreground">Could not check</span></>
                        : hasUpdate
                          ? <><span className="size-1.5 rounded-full bg-sky-400" /><span className="text-sky-400"> {availableCount} available</span>{isChecking && <span className="text-muted-foreground">· refreshing</span>}</>
                          : <><span className="size-1.5 rounded-full bg-emerald-500/80" /><span className="text-muted-foreground">Up to date</span></>}
                  </span>
                </TableCell>
                <TableCell className="py-2.5 text-right">
                  <Button size="sm" variant="secondary" className="h-7 px-2.5 text-xs font-normal" onClick={() => openVersions(g)}>
                    <Settings2 className="size-3.5 opacity-70" /> Manage
                  </Button>
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
      {!loading && groups.length === 0 && <p className="text-sm text-muted-foreground">No runtimes in the catalog for this platform yet.</p>}

      <Dialog
        open={!!managedGroup}
        onClose={() => { setManageId(null); setInstallError(null); setVersionSearch('') }}
        wide
        title={`${managedGroup?.name ?? 'Runtime'} versions`}
        description="Version lists refresh in the background and are checked again when needed. Search by version, choose one to install, or manage versions already on this computer."
      >
        {managedGroup && <div className="flex flex-col gap-4">
          <section className="rounded-lg border border-border/60 bg-background/40 p-3.5">
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <h3 className="text-[13px] font-semibold leading-none">Install a version</h3>
            </div>
              <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">Vendor SHA-256 checksums are verified when published. {managedGroup.name} archives come directly from the vendor over HTTPS.</p>
            <div className="mt-3 grid gap-2 lg:grid-cols-[minmax(0,1fr)_170px_auto]">
              <Input
                aria-label={`Search ${managedGroup.name} versions`}
                value={versionSearch}
                onChange={(e) => {
                  const query = e.target.value
                  setVersionSearch(query)
                  const firstMatch = installable.find((row) => row.version.toLowerCase().includes(query.trim().toLowerCase()))
                  if (firstMatch) setInstallChoices((prev) => ({ ...prev, [managedGroup!.id]: firstMatch.version }))
                }}
                placeholder="Search versions…"
                className="h-9 text-[13px]"
              />
              <div className="relative min-w-0">
                <select
                  aria-label={`Available ${managedGroup.name} versions`}
                  value={selectedInstall}
                  disabled={filteredOptions.length === 0 || dialogChecking}
                  onChange={(e) => setInstallChoices((prev) => ({ ...prev, [managedGroup.id]: e.target.value }))}
                  className="h-9 w-full min-w-0 appearance-none rounded-md border border-border/60 bg-background py-1 pl-2.5 pr-8 text-[13px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
                >
                  {filteredOptions.length === 0 && dialogChecking && <option value="">Checking vendor list…</option>}
                  {filteredOptions.length === 0 && !dialogChecking && unverifiedCount > 0 && <option value="">Verifying cached versions…</option>}
                  {filteredOptions.length === 0 && !dialogChecking && unverifiedCount === 0 && installable.length === 0 && <option value="">All catalog versions are installed</option>}
                  {filteredOptions.length === 0 && !dialogChecking && versionSearch.trim() !== '' && verifiedManaged.length > 0 && <option value="">No versions match your search</option>}
                  {[...optionsByMajor].map(([major, rows]) => (
                    <optgroup key={major} label={major === 'Other' ? major : `Major ${major}`}>
                      {rows.map((row) => {
                        const installed = !!row.entry?.installed
                        return <option key={row.key} value={row.version} disabled={installed}>{row.version}{installed ? ' · Installed' : progress[row.key]?.kind === 'progress' ? ' · Installing' : ''}</option>
                      })}
                    </optgroup>
                  ))}
                </select>
                <ChevronDown aria-hidden="true" className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              </div>
              <Button
                className="h-9 shrink-0 px-3.5 text-[13px]"
                disabled={!selectedInstall || !selectedInstallVisible || installingChoice || installable.length === 0 || dialogChecking}
                onClick={() => { const row = installable.find((r) => r.version === selectedInstall); if (row?.entry) void install(row.entry) }}
              >
                {installingChoice ? <Spinner /> : <Download />} {installingChoice ? 'Installing…' : 'Download and install'}
              </Button>
            </div>
            {dialogChecking && <p className="mt-2 text-xs text-muted-foreground">Checking the vendor’s online version list…</p>}
            {dialogRefreshFailed && (
              <p className="mt-2 text-xs text-muted-foreground">
                Last check failed —{" "}
                <button
                  className="underline underline-offset-2 hover:text-foreground"
                  onClick={() => {
                    setCatalogRefreshing(true)
                    void refreshOnlineCatalog(managedGroup.id)
                      .then((entries) => {
                        if (!entries) return
                        const newest = entries
                          .filter((entry) => !entry.installed)
                          .sort((a, b) => compareVersionsDesc(a.version, b.version))[0]
                        if (newest) setInstallChoices((prev) => ({ ...prev, [managedGroup!.id]: newest.version }))
                      })
                      .catch((err) => setError(err as Diagnostic))
                      .finally(() => setCatalogRefreshing(false))
                  }}
                >
                  retry
                </button>
              </p>
            )}
            {selectedInstall && progress[`${managedGroup.id}@${selectedInstall}`]?.kind === 'progress' && (() => {
              const live = progress[`${managedGroup.id}@${selectedInstall}`]
              return live?.kind === 'progress' ? <p className="mt-2 text-xs text-muted-foreground">{live.state}: {formatBytes(live.downloaded)}{live.total ? ` / ${formatBytes(live.total)}` : ''}</p> : null
            })()}
            {installError && installError.id === managedGroup.id && (
              <p className="mt-2 text-sm text-destructive">{installError.version}: {installError.message}</p>
            )}
          </section>

          <section>
            <div className="mb-2 flex items-center justify-between gap-3">
              <div className="flex flex-col gap-0.5">
                <h3 className="text-[13px] font-semibold leading-none">Installed versions</h3>
                <p className="text-xs text-muted-foreground">The default is used unless a project or site pins another version.</p>
              </div>
              <Badge variant="secondary" className="shrink-0 rounded-full px-2 text-[11px] font-normal text-muted-foreground">{installedRows.length} versions</Badge>
            </div>
            <div className="overflow-hidden rounded-lg border border-border/60">
              {installedRows.length === 0 ? <p className="px-3 py-4 text-center text-[13px] text-muted-foreground">No versions installed.</p> : installedRows.map((row) => {
                const isDefault = !!row.entry?.is_default
                return (
                <div key={row.key} className={`flex flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border/60 px-3 py-2 last:border-b-0 ${isDefault ? 'bg-primary/[0.06]' : ''}`}>
                  <div className="flex min-w-0 items-center gap-2">
                    {isDefault && <span aria-hidden="true" className="h-8 w-0.5 shrink-0 rounded-full bg-primary/70" />}
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-1.5 text-[13px] font-medium tabular-nums">
                        <span className="font-mono">{row.version}</span>
                        {isDefault && <Badge variant="secondary" className="px-1.5 text-[11px] font-normal">Default</Badge>}
                        {row.kind === 'custom' && <Badge variant="outline" className="px-1.5 text-[11px] font-normal text-muted-foreground">Yours</Badge>}
                      </div>
                      {row.custom?.path
                        ? <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">{row.custom.path}</p>
                        : row.entry?.installed && <p className="mt-0.5 text-[11px] text-muted-foreground">Managed install</p>}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-0.5">
                    {managedGroup.id === 'php' && (row.entry?.installed || row.kind === 'custom') && <>
                      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => setExtVersion(row.version)}><Puzzle className="size-3.5 opacity-70" /> Extensions</Button>
                      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => setXdebugVersion(row.version)}><Bug className="size-3.5 opacity-70" /> Xdebug</Button>
                    </>}
                    {row.kind === 'managed' && row.entry && !row.entry.is_default && <Button size="sm" variant="secondary" className="h-7 px-2.5 text-xs font-normal" onClick={() => void chooseDefault(row.entry!)} title="Make this the default version for new projects and services"><Check className="size-3.5 opacity-70" /> Set default</Button>}
                    {row.kind === 'managed' && row.entry && <Button size="sm" variant="ghost" className="h-7 w-7 px-0 text-muted-foreground hover:text-foreground" onClick={() => void removeRuntime(row.entry!)} title={row.entry.is_default && managedInstalledCount > 1 ? 'Choose another default first' : 'Remove version'} disabled={row.entry.is_default && managedInstalledCount > 1}><Trash2 className="size-3.5 opacity-70" /></Button>}
                    {row.kind === 'custom' && row.custom && <Button size="sm" variant="ghost" className="h-7 w-7 px-0 text-muted-foreground hover:text-foreground" title="Remove from list" onClick={() => void removeCustomInstall(row.custom!)}><Trash2 className="size-3.5 opacity-70" /></Button>}
                  </div>
                </div>
                )
              })}
            </div>
          </section>
          {notice && <p className="text-sm text-muted-foreground">{notice}</p>}
          {error && <Card className="border-destructive/40 bg-destructive/5"><CardContent className="pt-4"><p className="font-medium text-destructive">{error.problem}</p><p className="mt-1 text-sm">{error.cause}</p></CardContent></Card>}
        </div>}
      </Dialog>

      <PhpExtensionsDialog key={extVersion ?? ''} version={extVersion} onClose={() => setExtVersion(null)} />
      <XdebugDialog key={'x' + (xdebugVersion ?? '')} version={xdebugVersion} onClose={() => setXdebugVersion(null)} />

      {error && !managedGroup && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardHeader>
            <CardTitle className="text-destructive">{error.problem}</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm">{error.cause}</p>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
