import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Bug, Check, Download, FolderSearch, Pause, Play, Puzzle, Settings2, Square, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { ScrollableList } from '@/components/ScrollableList'
import { Spinner } from '@/components/Spinner'
import { ErrorCard } from '@/components/ErrorCard'
import { PhpExtensionsDialog } from '@/components/PhpExtensionsDialog'
import { XdebugDialog } from '@/components/XdebugDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Select } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { TechIcon } from '@/components/TechIcon'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CatalogEntry, type CustomInstall, type Diagnostic, type EolItem, type RuntimeEvent, runCommand } from '@/core'
import { eolLabel, formatBytes, usePoll } from '@/lib/hooks'
import { createPersistentCache } from '@/lib/cache'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'
import {
  compareVersionsDesc,
  formatAvailableVersions,
  getRuntimeStatus,
  isMajorUpgrade,
  isPrerelease,
} from '@/lib/runtimeStatus'
import { cn } from '@/lib/utils'

/** Runtimes a user may already have somewhere else and want to point at. */
const LOCATABLE = ['php', 'node', 'python']
/** Database GUI tools belong in Databases > External tools and should never be listed as runtimes. */
const DB_GUI_TOOL_IDS = new Set([
  'heidisql',
  'pgadmin',
  'tinyrdm',
  'nosqlbooster',
  'dbbrowser',
  'dbeaver',
  'tableplus',
  'redisinsight',
  'datagrip',
  'navicat',
  'sqlitestudio',
  'letos',
  'adminer',
  'phpmyadmin',
])
const ONLINE_CATALOGS = new Set(['nginx', 'node', 'mariadb', 'php', 'apache', 'composer', 'mongodb', 'postgres', 'redis', 'memcached', 'mailpit', 'python'])
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

const runtimeCatalogCache = createPersistentCache<CatalogEntry[]>('runtimes_catalog', [])
const customInstallsCache = createPersistentCache<CustomInstall[]>('runtimes_custom', [])
let cachedCatalogStatus: Record<string, CatalogStatus> = {}
let cachedVerifiedKeys: Set<string> = new Set()
let cachedHasLoaded = false

function RuntimeProgress() {
  return (
    <div className="absolute inset-x-0 top-0 z-10" role="status" aria-label="Looking for runtimes on this computer…">
      <div className="h-0.5 overflow-hidden rounded-t-xl bg-muted">
        <div className="h-full w-1/3 animate-[ols-indeterminate_1.4s_ease-in-out_infinite] bg-primary" />
      </div>
      <span className="absolute left-3 top-1 rounded-b bg-card px-2 py-1 text-xs text-muted-foreground shadow-sm">Looking for runtimes on this computer…</span>
    </div>
  )
}

export function RuntimesPage() {
  const [catalog, setCatalog] = useState<CatalogEntry[]>(runtimeCatalogCache.get)
  const [progress, setProgress] = useState<Record<string, RuntimeEvent | undefined>>({})
  const [error, setError] = useState<Diagnostic | null>(null)
  const [loading, setLoading] = useState(() => !cachedHasLoaded && runtimeCatalogCache.get().length === 0)
  const [catalogRefreshing, setCatalogRefreshing] = useState(false)
  const [catalogStatus, setCatalogStatus] = useState<Record<string, CatalogStatus>>(cachedCatalogStatus)
  const [cachedCatalogIds, setCachedCatalogIds] = useState<Set<string>>(new Set())
  const [verifiedKeys, setVerifiedKeys] = useState<Set<string>>(cachedVerifiedKeys)
  const [filter, setFilter] = useState<'all' | 'updates' | 'installed'>('all')
  const [eolItems, setEolItems] = useState<EolItem[]>([])

  const [installError, setInstallError] = useState<{ id: string; version: string; message: string } | null>(null)
  const [pending, setPending] = useState<Set<string>>(new Set())

  const [customInstalls, setCustomInstalls] = useState<CustomInstall[]>(customInstallsCache.get)
  const [manageId, setManageId] = useState<string | null>(null)
  const [installChoices, setInstallChoices] = useState<Record<string, string>>({})
  const [versionSearch, setVersionSearch] = useState('')
  const [showPrerelease, setShowPrerelease] = useState(false)
  const [customId, setCustomId] = useState('php')
  const [customLabel, setCustomLabel] = useState('')
  const [extVersion, setExtVersion] = useState<string | null>(null)
  const [xdebugVersion, setXdebugVersion] = useState<string | null>(null)
  const lastProgressPaint = useRef(0)

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') {
      const cached = readCatalogCache()
      setCachedCatalogIds(new Set(Object.keys(cached)))
      const cachedEntries = Object.entries(cached).flatMap(([id, value]) =>
        value.versions.map(({ name, version }) => ({
          id,
          name,
          version,
          installed: false,
          is_default: false,
          system: null,
        })),
      )
      const installedSet = new Set(
        res.entries
          .filter((e) => e.installed)
          .map((e) => `${e.id}@${e.version}`),
      )
      const defaultMap = new Map(
        res.entries
          .filter((e) => e.is_default)
          .map((e) => [e.id, e.version]),
      )
      setCatalog((prev) => {
        const byKey = new Map<string, CatalogEntry>()
        for (const entry of cachedEntries) {
          byKey.set(`${entry.id}@${entry.version}`, entry)
        }
        for (const entry of prev) {
          const key = `${entry.id}@${entry.version}`
          const isInstalled = installedSet.has(key)
          const isDefault = defaultMap.get(entry.id) === entry.version
          byKey.set(key, {
            ...entry,
            installed: isInstalled,
            is_default: isDefault,
          })
        }
        for (const entry of res.entries) {
          byKey.set(`${entry.id}@${entry.version}`, entry)
        }
        const next = Array.from(byKey.values())
        runtimeCatalogCache.set(next)
        return next
      })
      const nextKeys = new Set([...verifiedKeys, ...res.entries.map((e) => `${e.id}@${e.version}`)])
      cachedVerifiedKeys = nextKeys
      setVerifiedKeys(nextKeys)
    }
    const custom = await runCommand({ type: 'list_custom_installs' })
    if (custom.type === 'custom_installs') {
      setCustomInstalls(custom.entries)
      customInstallsCache.set(custom.entries)
    }
    cachedHasLoaded = true
    setLoading(false)
  }

  async function refreshOnlineCatalog(id: string) {
    setCatalogStatus((prev) => ({ ...prev, [id]: 'checking' }))
    try {
      const res = await runCommand({ type: 'refresh_runtime_catalog', id })
      if (res.type !== 'runtime_catalog') return
      const entries = res.entries.filter((entry) => entry.id === id)
      setCatalog((prev) => {
        const next = [...prev.filter((entry) => entry.id !== id), ...entries]
        runtimeCatalogCache.set(next)
        return next
      })
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
        if (res.found.length === 0) {
          notify.warning('No PHP installs found there. A PHP folder needs both php.exe and php-cgi.exe.')
        } else {
          notify.success(`Added ${res.found.length} PHP version${res.found.length === 1 ? '' : 's'}: ${res.found.map((f) => f.version).join(', ')}`)
        }
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

  // §169: end-of-life badges, read once when the page opens (offline data).
  useEffect(() => {
    void runCommand({ type: 'get_eol_report' })
      .then((r) => {
        if (r.type === 'eol_report') setEolItems(r.items)
      })
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    let active = true
    void (async () => {
      try {
        await refresh()
        if (!active) return
        const cached = readCatalogCache()
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
      const key = `${e.id}@${e.version}`
      setPending((prev) => (prev.has(key) ? new Set([...prev].filter((k) => k !== key)) : prev))
      if (e.kind === 'progress' && e.state === 'downloading') {
        const now = Date.now()
        if (now - lastProgressPaint.current < 250) return
        lastProgressPaint.current = now
      }
      setProgress((prev) => ({ ...prev, [key]: e }))
      if (e.kind === 'failed') setInstallError({ id: e.id, version: e.version, message: e.message })
      if (e.kind === 'installed') {
        setInstallError(null)
        setCatalog((prev) => {
          const next = prev.map((item) =>
            item.id === e.id && item.version === e.version ? { ...item, installed: true } : item
          )
          runtimeCatalogCache.set(next)
          return next
        })
      }
      if (e.kind === 'installed' || e.kind === 'failed' || e.kind === 'cancelled') void refresh()
    })
    return () => {
      active = false
      void unlisten.then((f) => f())
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  usePoll(() => refresh().catch(() => undefined), 3000)

  const install = (entry: CatalogEntry) =>
    guarded(async () => {
      const key = `${entry.id}@${entry.version}`
      setInstallError(null)
      setPending((prev) => new Set([...prev, key]))
      try {
        await runCommand({ type: 'install_runtime', id: entry.id, version: entry.version })
      } catch (err) {
        setPending((prev) => new Set([...prev].filter((k) => k !== key)))
        throw err
      }
    })

  const handleUpdate = async (group: Group, version: string, isMajor: boolean) => {
    if (isMajor) {
      const ok = await confirmAction(
        `Upgrading ${group.name} to ${version} is a major version upgrade.\n\nMajor upgrades may introduce breaking changes, configuration deprecations, or data format incompatibilities with existing databases and applications.\n\nAre you sure you want to proceed with installing ${group.name} ${version}?`,
        `Major Version Upgrade: ${group.name} ${version}`,
        `Upgrade to ${version}`,
      )
      if (!ok) return
    }
    const row = group.rows.find((r) => r.version === version && r.kind === 'managed')
    const entry: CatalogEntry = row?.entry ?? {
      id: group.id,
      name: group.name,
      version,
      installed: false,
      is_default: false,
      system: null,
    }
    void install(entry)
  }

  const setPaused = (entry: CatalogEntry, paused: boolean) =>
    guarded(async () => {
      await runCommand({ type: 'pause_runtime', id: entry.id, version: entry.version, paused })
    })

  const stopInstall = (entry: CatalogEntry) =>
    guarded(async () => {
      if (!(await confirmAction(`Stop downloading ${entry.name} ${entry.version}? The partial download is deleted and nothing is installed.`, 'Stop download'))) return
      await runCommand({ type: 'cancel_runtime', id: entry.id, version: entry.version })
    })

  const chooseDefault = (entry: CatalogEntry) =>
    guarded(async () => {
      await runCommand({ type: 'set_setting', key: `runtime.${entry.id}.global`, value: entry.version })
      await refresh()
      const msg = entry.id === 'mariadb'
        ? `MariaDB ${entry.version} is now the default. Older series keep their own data; use Databases → Import databases to migrate it.`
        : ['nginx', 'apache', 'caddy'].includes(entry.id)
          ? `${entry.name} ${entry.version} will be used when you next start the web server.`
          : entry.id === 'php'
            ? `PHP ${entry.version} is now the default. Site/project pins still take precedence; apply web config to update unpinned PHP sites.`
            : ['node', 'python'].includes(entry.id)
              ? `${entry.name} ${entry.version} is now the default. A project's pinned version still takes precedence.`
              : `${entry.name} ${entry.version} will be used the next time its service starts.`
      notify.info(msg)
    })

  const removeRuntime = (entry: CatalogEntry) =>
    guarded(async () => {
      const migrationNote = entry.id === 'mariadb'
        ? ' If you plan to migrate its databases to another MariaDB series, do that first; removing this runtime also removes the binaries needed to read its data.'
        : ''
      if (!(await confirmAction(`Remove the managed ${entry.name} ${entry.version} files? Projects or sites pinned to this version will need another version selected or this version reinstalled. Project files and MariaDB data stay in place.${migrationNote}`, 'Remove version', 'Remove version'))) return
      await runCommand({ type: 'remove_runtime', id: entry.id, version: entry.version })
      setCatalog((prev) => {
        const next = prev.map((e) =>
          e.id === entry.id && e.version === entry.version
            ? { ...e, installed: false, is_default: false }
            : e,
        )
        runtimeCatalogCache.set(next)
        return next
      })
      await refresh()
      notify.success(`Removed ${entry.name} ${entry.version}.`)
    })

  const openVersions = (group: Group, preselectedVersion?: string) => {
    setError(null)
    setInstallError(null)
    setManageId(group.id)
    setVersionSearch('')
    setShowPrerelease(preselectedVersion ? isPrerelease(preselectedVersion) : false)
    const defaultChoice = preselectedVersion ?? group.rows.find((r) => r.kind === 'managed' && !r.entry?.installed && !isPrerelease(r.version))?.version ?? group.rows.find((r) => r.kind === 'managed' && !r.entry?.installed)?.version ?? ''
    setInstallChoices((prev) => ({ ...prev, [group.id]: defaultChoice }))
    if (!ONLINE_CATALOGS.has(group.id)) {
      setCatalogRefreshing(false)
      return
    }
    if (catalogStatus[group.id] === 'checking') {
      setCatalogRefreshing(false)
      return
    }
    setCatalogRefreshing(true)
    void refreshOnlineCatalog(group.id)
      .then((entries) => {
        if (!entries) return
        if (preselectedVersion && entries.some((entry) => entry.version === preselectedVersion && !entry.installed)) {
          setInstallChoices((prev) => ({ ...prev, [group.id]: preselectedVersion }))
          return
        }
        const newest = entries
          .filter((entry) => !entry.installed && !isPrerelease(entry.version))
          .sort((a, b) => compareVersionsDesc(a.version, b.version))[0] ?? entries
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
      if (DB_GUI_TOOL_IDS.has(entry.id.toLowerCase())) continue
      const g = byId.get(entry.id) ?? { id: entry.id, name: entry.name, rows: [] }
      const key = `${entry.id}@${entry.version}`
      const existing = g.rows.find((r) => r.key === key)
      if (!existing) {
        g.rows.push({ key, version: entry.version, kind: 'managed', entry })
      } else {
        existing.entry = entry
      }
      byId.set(entry.id, g)
    }
    for (const c of customInstalls) {
      if (DB_GUI_TOOL_IDS.has(c.id.toLowerCase())) continue
      const g = byId.get(c.id) ?? { id: c.id, name: c.id.toUpperCase(), rows: [] }
      const key = `custom:${c.id}:${c.label}`
      if (!g.rows.some((r) => r.key === key)) {
        g.rows.push({ key, version: c.label || 'custom', kind: 'custom', custom: c })
      }
      byId.set(c.id, g)
    }
    const list = [...byId.values()]
    for (const g of list) g.rows.sort((a, b) => compareVersionsDesc(a.version, b.version))
    const has = (g: Group) => g.rows.some((r) => r.kind === 'custom' || r.entry?.installed)
    return list.sort((a, b) => Number(has(b)) - Number(has(a)) || a.name.localeCompare(b.name))
  }, [catalog, customInstalls])

  const groupStatuses = useMemo(() => {
    const map = new Map<string, ReturnType<typeof getRuntimeStatus>>()
    for (const g of groups) {
      const isChecking = ONLINE_CATALOGS.has(g.id) && catalogStatus[g.id] === 'checking'
      const isError = ONLINE_CATALOGS.has(g.id) && catalogStatus[g.id] === 'error' && !cachedCatalogIds.has(g.id)
      const st = getRuntimeStatus({
        rows: g.rows.map((r) => ({
          key: r.key,
          version: r.version,
          kind: r.kind,
          installed: r.kind === 'custom' || !!r.entry?.installed,
          isDefault: !!r.entry?.is_default,
        })),
        runtimeId: g.id,
        isChecking,
        isError,
      })
      map.set(g.id, st)
    }
    return map
  }, [groups, catalogStatus, cachedCatalogIds])

  const updatesAvailableCount = useMemo(() => {
    let count = 0
    for (const status of groupStatuses.values()) {
      if (status.statusKind === 'update_available' && status.targetUpdateVersion) count++
    }
    return count
  }, [groupStatuses])

  const installedRuntimesCount = useMemo(() => {
    let count = 0
    for (const status of groupStatuses.values()) {
      if (status.totalInstalled > 0) count++
    }
    return count
  }, [groupStatuses])

  const filteredGroups = useMemo(() => {
    if (filter === 'updates') {
      return groups.filter((g) => {
        const s = groupStatuses.get(g.id)
        return s?.statusKind === 'update_available' && s?.targetUpdateVersion
      })
    }
    if (filter === 'installed') {
      return groups.filter((g) => {
        const s = groupStatuses.get(g.id)
        return s && s.totalInstalled > 0
      })
    }
    return groups
  }, [groups, filter, groupStatuses])

  const handleUpdateAll = async () => {
    const updatable = groups.filter((g) => {
      const s = groupStatuses.get(g.id)
      return s?.statusKind === 'update_available' && s?.targetUpdateVersion
    })
    if (updatable.length === 0) return

    const majorUpgrades = updatable.filter((g) => {
      const s = groupStatuses.get(g.id)
      return s?.isMajorUpgrade
    })

    let confirmMsg = `Update all ${updatable.length} runtimes to their latest stable version?`
    if (majorUpgrades.length > 0) {
      confirmMsg += `\n\nNotice: ${majorUpgrades.length} runtime(s) have major upgrades which may contain breaking changes:\n`
      for (const g of majorUpgrades) {
        const s = groupStatuses.get(g.id)
        confirmMsg += `• ${g.name} → ${s?.targetUpdateVersion}\n`
      }
    }

    const ok = await confirmAction(confirmMsg, 'Update all runtimes', 'Update all')
    if (!ok) return

    for (const g of updatable) {
      const s = groupStatuses.get(g.id)
      if (s?.targetUpdateVersion) {
        const row = g.rows.find((r) => r.version === s.targetUpdateVersion && r.kind === 'managed')
        const entry: CatalogEntry = row?.entry ?? {
          id: g.id,
          name: g.name,
          version: s.targetUpdateVersion,
          installed: false,
          is_default: false,
          system: null,
        }
        void install(entry)
      }
    }
  }

  const managedGroup = groups.find((g) => g.id === manageId) ?? null
  const managedStatus = useMemo(() => {
    if (!managedGroup) return null
    return groupStatuses.get(managedGroup.id) ?? null
  }, [managedGroup, groupStatuses])

  const verifiedManaged = managedGroup?.rows.filter((r) => r.kind === 'managed' && verifiedKeys.has(r.key)) ?? []
  const visibleManaged = verifiedManaged.filter((r) => showPrerelease || !isPrerelease(r.version))
  const installable = visibleManaged.filter((r) => !r.entry?.installed)
  const unverifiedCount = managedGroup?.rows.filter((r) => r.kind === 'managed' && !r.entry?.installed && !verifiedKeys.has(r.key)).length ?? 0
  const filteredOptions = visibleManaged.filter((row) => row.version.toLowerCase().includes(versionSearch.trim().toLowerCase()))
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
  const selectedInstallRow = installable.find((r) => r.version === selectedInstall)
  const selectedInstallPending = !!manageId && pending.has(`${manageId}@${selectedInstall}`)
  const isSelectedPrerelease = isPrerelease(selectedInstall)
  const installingChoice = selectedInstallEvent?.kind === 'progress' || selectedInstallPending
  const installedRows = managedGroup?.rows.filter((r) => r.kind === 'custom' || r.entry?.installed) ?? []
  const eolFor = (version: string) => {
    const item = eolItems.find((i) => i.runtime === managedGroup?.id && i.version === version)
    return item ? eolLabel(item.status) : null
  }
  const managedInstalledCount = managedGroup?.rows.filter((r) => r.kind === 'managed' && r.entry?.installed).length ?? 0
  const activeDownloads = [...new Set([...Object.keys(progress), ...pending])].flatMap((key) => {
    const [id, version] = [key.slice(0, key.indexOf('@')), key.slice(key.indexOf('@') + 1)]
    const event = progress[key]
    if (event && !(event.kind === 'progress' && (event.state === 'downloading' || event.state === 'paused'))) return []
    if (!event && !pending.has(key)) return []
    if (managedGroup && id !== managedGroup.id) return []
    const entry = catalog.find((c) => c.id === id && c.version === version)
    const live: RuntimeEvent = event ?? { kind: 'progress', id, version, state: 'downloading', downloaded: 0, total: null }
    return entry ? [{ key, entry, live }] : []
  })

  return (
    <PageLayout
      mode="list"
      header={
      <div className="flex flex-col gap-1">
        <h1 className="text-xl font-semibold tracking-tight">Runtimes</h1>
        <p className="max-w-2xl text-[13px] leading-relaxed text-muted-foreground">
          Languages, databases, and servers managed by OpenLocalServer. Every version runs from its own directory with vendor SHA-256 integrity verification.
        </p>
      </div>
      }
      toolbar={
      <div className="flex flex-col gap-3">
      <div className="@container rounded-xl border border-border/60 bg-card/40 px-3 py-2.5">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <div className="min-w-48 flex-1">
            <h2 className="text-sm font-semibold leading-none">Add a runtime</h2>
            <p className="mt-1 text-xs text-muted-foreground">Scan an existing install or register one executable.</p>
          </div>
          <Button size="sm" variant="outline" className="h-8 shrink-0 text-sm" onClick={scanPhpFolder} title="Scan a folder for installed runtimes. Adds every PHP version found in the folder you pick">
            <FolderSearch /> Scan folder
          </Button>
          <div className="hidden h-6 w-px shrink-0 bg-border @2xl:block" role="separator" aria-orientation="vertical" />
          <div className="flex w-full min-w-0 items-center gap-2 @2xl:w-auto">
            <span className="hidden shrink-0 text-xs text-muted-foreground @4xl:inline">Single executable</span>
            <Select
              aria-label="Executable runtime"
              value={customId}
              onChange={(e) => setCustomId(e.target.value)}
              className="w-[92px] shrink-0"
              triggerClassName="h-8 text-sm px-2.5"
            >
              {LOCATABLE.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </Select>
            <Input
              aria-label="Executable version"
              value={customLabel}
              onChange={(e) => setCustomLabel(e.target.value)}
              placeholder="Version (e.g. 8.1.2)"
              className="h-8 min-w-28 flex-1 text-sm @2xl:w-44 @2xl:flex-none"
            />
            <Button size="sm" variant="secondary" className="h-8 shrink-0 text-sm" onClick={addCustomInstall}>
              <FolderSearch /> Locate
            </Button>
          </div>
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
        <div className="flex items-center gap-1 rounded-lg border border-border/60 bg-muted/40 p-1">
          <button
            type="button"
            onClick={() => setFilter('all')}
            className={cn(
              "rounded-md px-2.5 py-1 text-xs font-medium transition-colors cursor-pointer",
              filter === 'all'
                ? "bg-background text-foreground shadow-xs"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            All ({groups.length})
          </button>
          <button
            type="button"
            onClick={() => setFilter('updates')}
            className={cn(
              "rounded-md px-2.5 py-1 text-xs font-medium transition-colors flex items-center gap-1.5 cursor-pointer",
              filter === 'updates'
                ? "bg-background text-foreground shadow-xs"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            {updatesAvailableCount > 0 && <span className="size-1.5 rounded-full bg-sky-500 shrink-0" />}
            Updates available ({updatesAvailableCount})
          </button>
          <button
            type="button"
            onClick={() => setFilter('installed')}
            className={cn(
              "rounded-md px-2.5 py-1 text-xs font-medium transition-colors cursor-pointer",
              filter === 'installed'
                ? "bg-background text-foreground shadow-xs"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            Installed ({installedRuntimesCount})
          </button>
        </div>

        {updatesAvailableCount > 0 && (
          <Button
            size="sm"
            variant="secondary"
            className="h-8 text-xs font-medium gap-1.5"
            onClick={handleUpdateAll}
          >
            <Download className="size-3.5" />
            Update all ({updatesAvailableCount})
          </Button>
        )}
      </div>
      </div>
      }
    >
      {installError && (
        <ErrorCard
          error={{
            problem: `${installError.version} could not be installed.`,
            cause: installError.message,
            fix: null,
          }}
          onDismiss={() => setInstallError(null)}
          overlay
        />
      )}

      <ScrollableList resetKey={filter} overlay={loading && <RuntimeProgress />}>
      <Table className="table-fixed" wrapperClassName={cn('overflow-visible rounded-none border-0 bg-transparent transition-opacity', loading && 'opacity-90')}>
        <TableHeader sticky>
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-9 w-[22%] text-[11px] font-medium uppercase tracking-wider">Runtime</TableHead>
            <TableHead className="h-9 w-[28%] text-[11px] font-medium uppercase tracking-wider">Installed</TableHead>
            <TableHead className="h-9 w-[22%] text-[11px] font-medium uppercase tracking-wider">Status</TableHead>
            <TableHead className="h-9 text-right text-[11px] font-medium uppercase tracking-wider">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading && catalog.length === 0 &&
            Array.from({ length: 6 }, (_, i) => (
              <TableRow key={i} className="h-14 hover:bg-transparent">
                {[0, 1, 2, 3].map((c) => (
                  <TableCell key={c}>
                    <div className="h-4 animate-pulse rounded bg-muted" style={{ width: c === 3 ? '72%' : '60%' }} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          {(!loading || catalog.length > 0) && filteredGroups.map((g) => {
            const isChecking = ONLINE_CATALOGS.has(g.id) && catalogStatus[g.id] === 'checking'
            const status = groupStatuses.get(g.id) ?? getRuntimeStatus({
              rows: g.rows.map((r) => ({
                key: r.key,
                version: r.version,
                kind: r.kind,
                installed: r.kind === 'custom' || !!r.entry?.installed,
                isDefault: !!r.entry?.is_default,
              })),
              runtimeId: g.id,
            })

            return (
              <TableRow key={g.id} className="h-14 transition-colors hover:bg-muted/40">
                <TableCell className="py-3 align-middle">
                  <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
                    <TechIcon id={g.id} className="size-4 shrink-0 opacity-90" />
                    <span className="truncate">{g.name}</span>
                  </span>
                </TableCell>
                <TableCell className="py-3 align-middle">
                  {status.totalInstalled === 0 ? (
                    <div className="flex flex-col gap-0.5">
                      <span className="flex items-center gap-1.5 text-xs">
                        <span className="font-mono text-muted-foreground">—</span>
                        <span className="text-slate-600 dark:text-slate-400 font-medium">0 installed</span>
                      </span>
                      {status.installableCount > 0 ? (
                        <span className="text-xs text-slate-600 dark:text-slate-400">
                          {formatAvailableVersions(status.installableCount)}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">—</span>
                      )}
                    </div>
                  ) : (
                    <div className="flex flex-col gap-0.5">
                      <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                        {status.defaultVersion ? (
                          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs font-medium text-foreground">
                            {status.defaultVersion}
                          </code>
                        ) : (
                          <span className="font-mono text-muted-foreground">—</span>
                        )}
                        {status.hasDefaultBadge && (
                          <Badge variant="secondary" className="px-1.5 py-0 text-[11px] font-normal">
                            Default
                          </Badge>
                        )}
                        <span className="text-xs text-slate-600 dark:text-slate-400 font-medium">
                          {status.totalInstalled} installed
                        </span>
                      </span>
                      {status.installableCount > 0 ? (
                        <span className="text-xs text-slate-600 dark:text-slate-400">
                          {formatAvailableVersions(status.installableCount)}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground">—</span>
                      )}
                    </div>
                  )}
                </TableCell>
                <TableCell className="py-3 align-middle">
                  <div className="flex flex-col gap-0.5">
                    {status.statusKind === 'checking' && (
                      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <Spinner className="size-3 shrink-0" />
                        <span>Checking…</span>
                      </span>
                    )}
                    {status.statusKind === 'error' && (
                      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <span className="size-2 rounded-full bg-muted-foreground/60 shrink-0" />
                        <span>Could not check</span>
                      </span>
                    )}
                    {status.statusKind === 'not_installed' && (
                      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <span className="size-2 rounded-full bg-muted-foreground/40 shrink-0" />
                        <span>Not installed</span>
                        {isChecking && <span className="text-[11px] text-muted-foreground">· checking</span>}
                      </span>
                    )}
                    {status.statusKind === 'up_to_date' && (
                      <span className="flex items-center gap-1.5 text-xs">
                        <span className="size-2 rounded-full bg-emerald-500 shrink-0" />
                        <span className="font-medium text-slate-700 dark:text-slate-300">Up to date</span>
                        {isChecking && <span className="text-[11px] text-muted-foreground">· checking</span>}
                      </span>
                    )}
                    {status.statusKind === 'update_available' && status.targetUpdateVersion && (() => {
                      const updateVersion = status.targetUpdateVersion
                      const isMajor = status.isMajorUpgrade
                      const updateKey = `${g.id}@${updateVersion}`
                      const updateEvt = progress[updateKey]
                      const isUpdating = pending.has(updateKey) || updateEvt?.kind === 'progress'
                      return (
                        <>
                          <span className="flex items-center gap-1.5 text-xs">
                            <span className={cn("size-2 rounded-full shrink-0", isUpdating ? "animate-pulse bg-sky-500" : "bg-sky-500 dark:bg-sky-400")} />
                            <span className="font-medium text-foreground">{isUpdating ? 'Updating…' : 'Update available'}</span>
                            {isChecking && <span className="text-[11px] text-muted-foreground">· checking</span>}
                          </span>
                          <span className="flex items-center gap-1.5 pl-3.5 text-xs text-slate-600 dark:text-slate-400">
                            {isUpdating && updateEvt?.kind === 'progress' && updateEvt.state === 'downloading' ? (
                              <span>{formatBytes(updateEvt.downloaded)}{updateEvt.total ? ` / ${formatBytes(updateEvt.total)}` : ''}</span>
                            ) : isUpdating ? (
                              <span>Starting {updateVersion}…</span>
                            ) : (
                              <>
                                <span>Latest: {updateVersion}</span>
                                {isMajor && (
                                  <Badge variant="warning" className="px-1.5 py-0 text-[10px] font-semibold uppercase tracking-wider leading-none">
                                    Major
                                  </Badge>
                                )}
                              </>
                            )}
                          </span>
                        </>
                      )
                    })()}
                  </div>
                </TableCell>
                <TableCell className="py-3 text-right align-middle">
                  <div className="flex flex-wrap items-center justify-end gap-2">
                    {status.statusKind === 'update_available' && status.targetUpdateVersion && (() => {
                      const updateVersion = status.targetUpdateVersion
                      const isMajor = status.isMajorUpgrade
                      const updateKey = `${g.id}@${updateVersion}`
                      const updateEvt = progress[updateKey]
                      const isUpdating = pending.has(updateKey) || updateEvt?.kind === 'progress'
                      return (
                        <Button
                          size="sm"
                          disabled={isUpdating}
                          className={cn(
                            "h-8 px-3 text-xs font-medium shrink-0",
                            isMajor && "border border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300 hover:bg-amber-500/20 hover:text-amber-800 dark:hover:text-amber-200 shadow-sm"
                          )}
                          onClick={() => void handleUpdate(g, updateVersion, isMajor)}
                          title={`Download and install ${g.name} ${updateVersion}`}
                        >
                          {isUpdating ? <Spinner className="size-3.5" /> : <Download className="size-3.5" />}
                          <span>{isUpdating ? 'Updating…' : `Update to ${updateVersion}`}</span>
                          {isMajor && !isUpdating && (
                            <Badge variant="warning" className="ml-1 px-1 py-0 text-[9px] font-semibold uppercase leading-none">
                              Major
                            </Badge>
                          )}
                        </Button>
                      )
                    })()}
                    <Button
                      size="sm"
                      variant="secondary"
                      className="h-8 min-w-[90px] px-3 text-xs font-normal shrink-0"
                      onClick={() => openVersions(g)}
                    >
                      <Settings2 className="size-3.5 opacity-70" /> Manage
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            )
          })}
          {(!loading || catalog.length > 0) && filteredGroups.length === 0 && (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={4} className="py-6 text-center text-xs text-muted-foreground">
                {filter === 'updates'
                  ? 'All runtimes are up to date. No updates available.'
                  : filter === 'installed'
                    ? 'No runtimes are currently installed.'
                    : 'No runtimes in the catalog for this platform yet.'}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
      </ScrollableList>

      <Dialog
        open={!!managedGroup}
        onClose={() => { setManageId(null); setInstallError(null); setVersionSearch(''); setShowPrerelease(false) }}
        wide
        title={`${managedGroup?.name ?? 'Runtime'} versions`}
        description="Version lists refresh in the background and are checked again when needed. Search by version, choose one to install, or manage versions already on this computer."
      >
        {managedGroup && <div className="flex flex-col gap-4">
          <section className="rounded-lg border border-border/60 bg-background/40 p-3.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-[13px] font-semibold leading-none">Install a version</h3>
              <label className="flex items-center gap-1.5 cursor-pointer text-xs font-medium text-slate-700 dark:text-slate-300 select-none">
                <input
                  type="checkbox"
                  checked={showPrerelease}
                  onChange={(e) => setShowPrerelease(e.target.checked)}
                  className="size-3.5 rounded border-border accent-primary cursor-pointer"
                />
                <span>Show pre-release versions</span>
              </label>
            </div>
            <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
              Vendor SHA-256 checksums are verified when published. {managedGroup.name} archives come directly from the vendor over HTTPS.
              {installable.length > 0 && ` (${formatAvailableVersions(installable.length)} to install)`}
            </p>
            <div className="mt-3 grid gap-2 lg:grid-cols-[minmax(0,1fr)_190px_auto]">
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
              <Select
                aria-label={`Available ${managedGroup.name} versions`}
                value={selectedInstall}
                disabled={filteredOptions.length === 0 || dialogChecking}
                onChange={(e) => setInstallChoices((prev) => ({ ...prev, [managedGroup.id]: e.target.value }))}
                className="min-w-0 flex-1"
                triggerClassName="h-9 text-[13px] px-2.5"
              >
                {filteredOptions.length === 0 && dialogChecking && <option value="">Checking vendor list…</option>}
                {filteredOptions.length === 0 && !dialogChecking && unverifiedCount > 0 && <option value="">Verifying cached versions…</option>}
                {filteredOptions.length === 0 && !dialogChecking && unverifiedCount === 0 && installable.length === 0 && <option value="">All catalog versions are installed</option>}
                {filteredOptions.length === 0 && !dialogChecking && versionSearch.trim() !== '' && verifiedManaged.length > 0 && <option value="">No versions match your search</option>}
                {[...optionsByMajor].map(([major, rows]) => (
                  <optgroup key={major} label={major === 'Other' ? major : `Major ${major}`}>
                    {rows.map((row) => {
                      const installed = !!row.entry?.installed
                      const isDefault = !!row.entry?.is_default
                      const isPre = isPrerelease(row.version)
                      return (
                        <option key={row.key} value={row.version} disabled={installed}>
                          {row.version}
                          {isPre ? ' · Pre-release' : ''}
                          {installed
                            ? isDefault
                              ? ' · Default'
                              : ' · Installed'
                            : progress[row.key]?.kind === 'progress'
                              ? ' · Installing'
                              : pending.has(row.key)
                                ? ' · Starting…'
                                : ''}
                        </option>
                      )
                    })}
                  </optgroup>
                ))}
              </Select>
              <div className="flex items-center gap-2">
                {isSelectedPrerelease && (
                  <Badge variant="outline" className="h-6 px-2 text-[11px] font-medium border-amber-500/40 text-amber-600 dark:text-amber-400">
                    Pre-release
                  </Badge>
                )}
                <Button
                  variant={isSelectedPrerelease ? 'secondary' : 'default'}
                  className={cn(
                    "h-9 shrink-0 px-3.5 text-[13px]",
                    isSelectedPrerelease && "border border-amber-500/40 text-amber-600 dark:text-amber-400 hover:bg-accent"
                  )}
                  disabled={!selectedInstall || !selectedInstallVisible || installingChoice || installable.length === 0 || dialogChecking}
                  onClick={async () => {
                    const row = installable.find((r) => r.version === selectedInstall)
                    if (!row?.entry) return
                    const isMajor = isMajorUpgrade(managedStatus?.highestInstalledVersion ?? null, selectedInstall)
                    if (isMajor) {
                      const ok = await confirmAction(
                        `Installing ${managedGroup.name} ${selectedInstall} is a major version upgrade.\n\nMajor upgrades may introduce breaking changes, configuration deprecations, or data format incompatibilities with existing databases and applications.\n\nAre you sure you want to proceed with installing ${managedGroup.name} ${selectedInstall}?`,
                        `Major Version Upgrade: ${managedGroup.name} ${selectedInstall}`,
                        `Install ${selectedInstall}`,
                      )
                      if (!ok) return
                    }
                    void install(row.entry)
                  }}
                >
                  {installingChoice ? <Spinner /> : <Download />}
                  {installingChoice ? 'Installing…' : isSelectedPrerelease ? 'Install pre-release' : 'Download and install'}
                </Button>
              </div>
            </div>
            {installingChoice && selectedInstallRow?.entry && selectedInstallEvent?.kind === 'progress' && (() => {
              const live = selectedInstallEvent
              const row = selectedInstallRow.entry!
              const paused = live.state === 'paused'
              const holding = paused || live.state === 'downloading'
              return (
                <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5">
                  <p className="text-xs text-muted-foreground">
                    {live.state}: {formatBytes(live.downloaded)}{live.total ? ` / ${formatBytes(live.total)}` : ''}
                    {paused ? ' — held; the download resumes where it stopped' : ''}
                  </p>
                  {holding && (
                    <>
                      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => void setPaused(row, !paused)}>
                        {paused ? <Play className="size-3.5 opacity-70" /> : <Pause className="size-3.5 opacity-70" />} {paused ? 'Resume' : 'Pause'}
                      </Button>
                      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => void stopInstall(row)}>
                        <Square className="size-3 opacity-70" /> Stop
                      </Button>
                    </>
                  )}
                </div>
              )
            })()}
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
                          .filter((entry) => !entry.installed && !isPrerelease(entry.version))
                          .sort((a, b) => compareVersionsDesc(a.version, b.version))[0] ?? entries
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
            {installError && installError.id === managedGroup.id && (
              <p className="mt-2 text-sm text-destructive">{installError.version}: {installError.message}</p>
            )}
          </section>

          {activeDownloads.length > 0 && (
            <section>
              <div className="mb-2 flex items-center justify-between gap-3">
                <div className="flex flex-col gap-0.5">
                  <h3 className="text-[13px] font-semibold leading-none">Downloads in progress</h3>
                  <p className="text-xs text-muted-foreground">Pausing holds the transfer; stopping deletes the partial download and installs nothing.</p>
                </div>
              </div>
              <div className="overflow-hidden rounded-lg border-border/60 border">
                {activeDownloads.map(({ key, entry, live }) => {
                  const paused = live.state === 'paused'
                  return (
                    <div key={key} className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border/60 px-3 py-2 last:border-b-0">
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-1.5 text-[13px] font-medium tabular-nums">
                          <span className="font-mono">{entry.version}</span>
                          {paused && <Badge variant="secondary" className="px-1.5 text-[11px] font-normal">Paused</Badge>}
                          {isPrerelease(entry.version) && (
                            <Badge variant="outline" className="px-1.5 text-[11px] font-normal border-amber-500/40 text-amber-600 dark:text-amber-400">
                              Pre-release
                            </Badge>
                          )}
                        </div>
                        <p className="mt-0.5 text-[11px] text-muted-foreground">
                          {live.state}: {formatBytes(live.downloaded)}{live.total ? ` / ${formatBytes(live.total)}` : ''}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-0.5">
                        <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => void setPaused(entry, !paused)}>
                          {paused ? <Play className="size-3.5 opacity-70" /> : <Pause className="size-3.5 opacity-70" />} {paused ? 'Resume' : 'Pause'}
                        </Button>
                        <Button size="sm" variant="ghost" className="h-7 px-2 text-xs font-normal text-muted-foreground hover:text-foreground" onClick={() => void stopInstall(entry)}>
                          <Square className="size-3 opacity-70" /> Stop
                        </Button>
                      </div>
                    </div>
                  )
                })}
              </div>
            </section>
          )}

          <section>
            <div className="mb-2 flex items-center justify-between gap-3">
              <div className="flex flex-col gap-0.5">
                <h3 className="text-[13px] font-semibold leading-none">Installed versions</h3>
                <p className="text-xs text-muted-foreground">The default is used unless a project or site pins another version.</p>
              </div>
              <Badge variant="secondary" className="shrink-0 rounded-full px-2 text-[11px] font-normal text-muted-foreground">{installedRows.length} {installedRows.length === 1 ? 'version' : 'versions'}</Badge>
            </div>
            <div className="overflow-hidden rounded-lg border border-border/60">
              {installedRows.length === 0 ? <p className="px-3 py-4 text-center text-[13px] text-muted-foreground">No versions installed.</p> : installedRows.map((row) => {
                const isDefault = !!row.entry?.is_default
                const isPre = isPrerelease(row.version)
                return (
                  <div key={row.key} className={`flex flex-wrap items-center justify-between gap-x-3 gap-y-2 border-b border-border/60 px-3 py-2 last:border-b-0 ${isDefault ? 'bg-primary/[0.06]' : ''}`}>
                    <div className="flex min-w-0 items-center gap-2">
                      {isDefault && <span aria-hidden="true" className="h-8 w-0.5 shrink-0 rounded-full bg-primary/70" />}
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-1.5 text-[13px] font-medium tabular-nums">
                          <span className="font-mono">{row.version}</span>
                          {isDefault ? (
                            <Badge variant="secondary" className="px-1.5 text-[11px] font-normal">Default</Badge>
                          ) : row.kind === 'managed' ? (
                            <Badge variant="outline" className="px-1.5 text-[11px] font-normal text-muted-foreground">Installed</Badge>
                          ) : null}
                          {isPre && (
                            <Badge variant="outline" className="px-1.5 text-[11px] font-normal border-amber-500/40 text-amber-600 dark:text-amber-400">
                              Pre-release
                            </Badge>
                          )}
                          {row.kind === 'custom' && <Badge variant="outline" className="px-1.5 text-[11px] font-normal text-muted-foreground">Yours</Badge>}
                          {(() => {
                            const eol = eolFor(row.version)
                            return eol ? <Badge variant={eol.variant} className="px-1.5 text-[11px] font-normal">{eol.variant === 'destructive' ? 'Ended' : 'Security fixes only'}</Badge> : null
                          })()}
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
    </PageLayout>
  )
}
