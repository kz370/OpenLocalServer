import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Bug, ChevronDown, Download, FolderSearch, Puzzle, Trash2 } from 'lucide-react'
import { Fragment, useEffect, useMemo, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { PhpExtensionsDialog } from '@/components/PhpExtensionsDialog'
import { XdebugDialog } from '@/components/XdebugDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { TechIcon } from '@/components/TechIcon'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CatalogEntry, type CustomInstall, type Diagnostic, type RuntimeEvent, runCommand } from '@/core'
import { formatBytes } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'

/** Runtimes a user may already have somewhere else and want to point at. */
const LOCATABLE = ['php', 'node', 'python']

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

  const [customInstalls, setCustomInstalls] = useState<CustomInstall[]>([])
  const [customId, setCustomId] = useState('php')
  const [customLabel, setCustomLabel] = useState('')
  const [extVersion, setExtVersion] = useState<string | null>(null)
  const [xdebugVersion, setXdebugVersion] = useState<string | null>(null)

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') setCatalog(res.entries)
    const custom = await runCommand({ type: 'list_custom_installs' })
    if (custom.type === 'custom_installs') setCustomInstalls(custom.entries)
    setLoading(false)
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
    void refresh().catch(() => setLoading(false))
    const unlisten = listen<RuntimeEvent>('runtime-event', (event) => {
      const e = event.payload
      setProgress((prev) => ({ ...prev, [`${e.id}@${e.version}`]: e }))
      if (e.kind === 'installed' || e.kind === 'failed') void refresh()
    })
    return () => {
      void unlisten.then((f) => f())
    }
  }, [])

  const install = (entry: CatalogEntry) =>
    guarded(async () => {
      await runCommand({ type: 'install_runtime', id: entry.id, version: entry.version })
    })

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

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Runtimes</h1>
        <p className="text-sm text-muted-foreground">
          Multiple versions are grouped by runtime. Downloads are verified by SHA-256 and cached (§20–21, §127).
        </p>
      </div>

      <div className="rounded-xl border border-border p-4">
        <div>
          <h2 className="text-sm font-semibold">Use versions you already have</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">Add all versions from a folder, or register one executable.</p>
        </div>
        <div className="mt-4 grid gap-3 md:grid-cols-[minmax(220px,0.85fr)_minmax(0,1.5fr)]">
          <div className="flex flex-col justify-center gap-2">
            <Button className="h-10 w-full justify-center" onClick={scanPhpFolder} title="Adds every PHP version found in the folder you pick">
              <FolderSearch /> Add folder of PHP versions
            </Button>
            <p className="text-center text-xs text-muted-foreground">Finds every PHP install in the selected folder.</p>
          </div>
          <div className="min-w-0 rounded-lg border border-border/70 bg-background/40 p-3">
            <div className="mb-2.5">
              <p className="text-xs font-medium">Or add one executable</p>
              <p className="mt-0.5 text-xs text-muted-foreground">Choose its runtime and version, then locate the file.</p>
            </div>
            <div className="flex min-w-0 flex-wrap gap-2 sm:flex-nowrap">
              <div className="relative w-28 shrink-0">
                <select
                  aria-label="Executable runtime"
                  value={customId}
                  onChange={(e) => setCustomId(e.target.value)}
                  className="h-10 w-full appearance-none rounded-lg border border-transparent bg-input/60 py-1 pl-3 pr-10 text-sm focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
                >
                  {LOCATABLE.map((id) => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
                </select>
                <ChevronDown aria-hidden="true" className="pointer-events-none absolute right-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              </div>
              <Input
                aria-label="Executable version"
                value={customLabel}
                onChange={(e) => setCustomLabel(e.target.value)}
                placeholder="Version (e.g. 8.1.2)"
                className="h-10 min-w-32 flex-1"
              />
              <Button size="default" variant="secondary" className="h-10 shrink-0" onClick={addCustomInstall}>
                <FolderSearch /> Locate
              </Button>
            </div>
          </div>
        </div>
      </div>

      {notice && <p className="text-sm text-muted-foreground">{notice}</p>}

      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-44">Runtime</TableHead>
            <TableHead className="w-32">Version</TableHead>
            <TableHead className="w-32">Status</TableHead>
            <TableHead>Details</TableHead>
            <TableHead className="w-56 text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading &&
            Array.from({ length: 6 }, (_, i) => (
              <TableRow key={i} className="hover:bg-transparent">
                {[0, 1, 2, 3, 4].map((c) => (
                  <TableCell key={c}>
                    <div className="h-4 animate-pulse rounded bg-muted" style={{ width: c === 3 ? '70%' : '60%' }} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          {loading && (
            <TableRow className="hover:bg-transparent">
              <TableCell colSpan={5} className="text-center text-xs text-muted-foreground">
                <Spinner className="mr-1.5 inline size-3.5 align-text-bottom" /> Looking for runtimes on this computer…
              </TableCell>
            </TableRow>
          )}
          {!loading &&
            groups.map((g) => {
              const installed = g.rows.filter((r) => r.kind === 'custom' || r.entry?.installed).length
              const grouped = g.rows.length > 1
              return (
                <Fragment key={g.id}>
                  {grouped && (
                    <TableRow className="border-b border-border bg-muted/40 hover:bg-muted/40">
                      <TableCell colSpan={5} className="py-1.5">
                        <span className="flex items-center gap-2 font-medium">
                          <TechIcon id={g.id} className="size-4" />
                          {g.name}
                          <span className="text-xs font-normal text-muted-foreground">
                            {installed}/{g.rows.length} installed
                          </span>
                        </span>
                      </TableCell>
                    </TableRow>
                  )}
                  {g.rows.map((row) => {
                    const live = progress[row.key]
                    const installing = live && live.kind === 'progress'
                    return (
                      <TableRow key={row.key}>
                        <TableCell className="w-44 py-1.5">
                          {!grouped && (
                            <span className="flex items-center gap-2 font-medium">
                              <TechIcon id={g.id} className="size-4" />
                              {g.name}
                            </span>
                          )}
                        </TableCell>
                        <TableCell className="w-32 py-1.5">{row.version}</TableCell>
                        <TableCell className="w-32 py-1.5">
                          {row.kind === 'custom' ? (
                            <Badge variant="secondary">Yours</Badge>
                          ) : row.entry?.installed ? (
                            <Badge variant="success">Installed</Badge>
                          ) : installing ? (
                            <Badge variant="secondary" className="capitalize">
                              {live.kind === 'progress' ? live.state : ''}
                            </Badge>
                          ) : live?.kind === 'failed' ? (
                            <Badge variant="destructive">Failed</Badge>
                          ) : (
                            <Badge variant="outline">Not installed</Badge>
                          )}
                        </TableCell>
                        <TableCell className="max-w-0 truncate py-1.5 text-xs text-muted-foreground">
                          {row.kind === 'custom' && row.custom?.path}
                          {live?.kind === 'progress' &&
                            `${formatBytes(live.downloaded)}${live.total ? ` / ${formatBytes(live.total)}` : ''}`}
                          {live?.kind === 'failed' && live.message}
                          {row.kind === 'managed' && !row.entry?.installed && !live && row.entry?.system &&
                            `Found on PATH: ${row.entry.system.version}`}
                        </TableCell>
                        <TableCell className="w-56 whitespace-nowrap py-1.5 text-right">
                          {g.id === 'php' && (row.kind === 'custom' ? !!row.custom?.label : row.entry?.installed) && (
                            <Button size="sm" variant="ghost" className="h-7" title="Extensions" onClick={() => setExtVersion(row.version)}>
                              <Puzzle className="size-3.5" /> Extensions
                            </Button>
                          )}
                          {g.id === 'php' && (row.kind === 'custom' ? !!row.custom?.label : row.entry?.installed) && (
                            <Button size="sm" variant="ghost" className="h-7" title="Xdebug" onClick={() => setXdebugVersion(row.version)}>
                              <Bug className="size-3.5" /> Xdebug
                            </Button>
                          )}
                          {row.kind === 'managed' && !row.entry?.installed && !installing && row.entry && (
                            <Button size="sm" variant="secondary" className="h-7" onClick={() => install(row.entry!)}>
                              <Download /> Install
                            </Button>
                          )}
                          {row.kind === 'custom' && row.custom && (
                            <Button size="sm" variant="ghost" className="h-7" title="Remove from list" onClick={() => removeCustomInstall(row.custom!)}>
                              <Trash2 className="size-3.5" />
                            </Button>
                          )}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </Fragment>
              )
            })}
        </TableBody>
      </Table>
      {!loading && groups.length === 0 && <p className="text-sm text-muted-foreground">No runtimes in the catalog for this platform yet.</p>}

      <PhpExtensionsDialog key={extVersion ?? ''} version={extVersion} onClose={() => setExtVersion(null)} />
      <XdebugDialog key={'x' + (xdebugVersion ?? '')} version={xdebugVersion} onClose={() => setXdebugVersion(null)} />

      {error && (
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
