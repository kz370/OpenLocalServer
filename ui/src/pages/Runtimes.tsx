import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Download, FolderSearch, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { type CatalogEntry, type CustomInstall, type Diagnostic, type RuntimeEvent, runCommand } from '@/core'
import { formatBytes } from '@/lib/hooks'

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

  const [customInstalls, setCustomInstalls] = useState<CustomInstall[]>([])
  const [customId, setCustomId] = useState('php')
  const [customLabel, setCustomLabel] = useState('')

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') setCatalog(res.entries)
    const custom = await runCommand({ type: 'list_custom_installs' })
    if (custom.type === 'custom_installs') setCustomInstalls(custom.entries)
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
      if (!window.confirm(`Remove ${name} from the list?\n\nThe files at ${entry.path} are not deleted.`)) return
      await runCommand({ type: 'remove_custom_install', id: entry.id, label: entry.label })
      await refresh()
    })

  useEffect(() => {
    void refresh()
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
          Versions are grouped by runtime. Downloads are verified by SHA-256 and cached (§20–21, §127).
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border px-3 py-2">
        <span className="text-sm font-medium">Use versions you already have</span>
        <Button size="sm" onClick={scanPhpFolder} title="Adds every PHP version found in the folder you pick">
          <FolderSearch /> Add folder of PHP versions
        </Button>
        <span className="px-1 text-xs text-muted-foreground">or one executable:</span>
        <select
          value={customId}
          onChange={(e) => setCustomId(e.target.value)}
          className="h-8 rounded-lg border border-transparent bg-input/60 px-2 text-sm"
        >
          {LOCATABLE.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </select>
        <Input
          value={customLabel}
          onChange={(e) => setCustomLabel(e.target.value)}
          placeholder="version, e.g. 8.1.2"
          className="h-8 w-36"
        />
        <Button size="sm" variant="secondary" onClick={addCustomInstall}>
          <FolderSearch /> Locate
        </Button>
      </div>

      {notice && <p className="text-sm text-muted-foreground">{notice}</p>}

      <Table>
        <TableBody>
          {groups.map((g) => {
            const installed = g.rows.filter((r) => r.kind === 'custom' || r.entry?.installed).length
            return g.rows.map((row, i) => {
              const live = progress[row.key]
              const installing = live && live.kind === 'progress'
              const last = i === g.rows.length - 1
              return (
                <TableRow key={row.key} className={last ? 'border-b-2' : 'border-b-0'}>
                  <TableCell className="w-44 py-1 align-top">
                    {i === 0 && (
                      <span className="font-medium">
                        {g.name}
                        {g.rows.length > 1 && (
                          <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                            {installed}/{g.rows.length}
                          </span>
                        )}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="w-32 py-1">{row.version}</TableCell>
                  <TableCell className="w-32 py-1">
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
                  <TableCell className="max-w-0 truncate py-1 text-xs text-muted-foreground">
                    {row.kind === 'custom' && row.custom?.path}
                    {live?.kind === 'progress' &&
                      `${formatBytes(live.downloaded)}${live.total ? ` / ${formatBytes(live.total)}` : ''}`}
                    {live?.kind === 'failed' && live.message}
                    {row.kind === 'managed' && !row.entry?.installed && !live && row.entry?.system &&
                      `Found on PATH: ${row.entry.system.version}`}
                  </TableCell>
                  <TableCell className="w-28 py-1 text-right">
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
            })
          })}
        </TableBody>
      </Table>
      {groups.length === 0 && <p className="text-sm text-muted-foreground">No runtimes in the catalog for this platform yet.</p>}

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
