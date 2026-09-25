import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { Download, FolderSearch, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CatalogEntry, type CustomInstall, type Diagnostic, type RuntimeEvent, runCommand } from '@/core'

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const RUNTIME_IDS = ['php', 'node', 'python']

export function RuntimesPage() {
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [progress, setProgress] = useState<Record<string, RuntimeEvent | undefined>>({})
  const [error, setError] = useState<Diagnostic | null>(null)

  const [customInstalls, setCustomInstalls] = useState<CustomInstall[]>([])
  const [customId, setCustomId] = useState('php')
  const [customLabel, setCustomLabel] = useState('')

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') setCatalog(res.entries)
    const custom = await runCommand({ type: 'list_custom_installs' })
    if (custom.type === 'custom_installs') setCustomInstalls(custom.entries)
  }

  async function addCustomInstall() {
    const picked = await open({
      multiple: false,
      directory: false,
      title: `Locate ${customId}${customLabel ? ` ${customLabel}` : ''}`,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    })
    if (!picked || Array.isArray(picked)) return
    setError(null)
    try {
      await runCommand({ type: 'set_custom_install', id: customId, label: customLabel, path: picked })
      setCustomLabel('')
      await refresh()
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function removeCustomInstall(entry: CustomInstall) {
    await runCommand({ type: 'remove_custom_install', id: entry.id, label: entry.label })
    await refresh()
  }

  useEffect(() => {
    refresh()
    const unlisten = listen<RuntimeEvent>('runtime-event', (event) => {
      const e = event.payload
      setProgress((prev) => ({ ...prev, [`${e.id}@${e.version}`]: e }))
      if (e.kind === 'installed' || e.kind === 'failed') refresh()
    })
    return () => {
      unlisten.then((f) => f())
    }
  }, [])

  async function install(entry: CatalogEntry) {
    setError(null)
    try {
      await runCommand({ type: 'install_runtime', id: entry.id, version: entry.version })
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Runtimes</h1>
        <p className="text-sm text-muted-foreground">
          Downloaded once, verified by SHA-256, cached for every project (§20–21, §127).
        </p>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Catalog</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Version</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Detail</TableHead>
                <TableHead className="text-right">Action</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {catalog.map((entry) => {
                const key = `${entry.id}@${entry.version}`
                const live = progress[key]
                const isInstalling = live && live.kind === 'progress'

                return (
                  <TableRow key={key}>
                    <TableCell className="font-medium">{entry.name}</TableCell>
                    <TableCell className="text-muted-foreground">{entry.version}</TableCell>
                    <TableCell>
                      {entry.installed ? (
                        <Badge variant="success">Installed</Badge>
                      ) : isInstalling ? (
                        <Badge variant="secondary" className="capitalize">
                          {live.kind === 'progress' ? live.state : ''}
                        </Badge>
                      ) : live?.kind === 'failed' ? (
                        <Badge variant="destructive">Failed</Badge>
                      ) : (
                        <Badge variant="outline">Not installed</Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {live?.kind === 'progress' &&
                        `${formatBytes(live.downloaded)}${live.total ? ` / ${formatBytes(live.total)}` : ''}`}
                      {live?.kind === 'failed' && live.message}
                    </TableCell>
                    <TableCell className="text-right">
                      {!entry.installed && !isInstalling && (
                        <Button size="sm" variant="secondary" onClick={() => install(entry)}>
                          <Download /> Install
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                )
              })}
              {catalog.length === 0 && (
                <TableRow>
                  <TableCell colSpan={5} className="text-center text-sm text-muted-foreground">
                    No runtimes in the catalog for this platform yet.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Custom locations</CardTitle>
          <CardDescription>
            Already have PHP, Node, or Python installed elsewhere? Point a project at it instead of downloading.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <select
              value={customId}
              onChange={(e) => setCustomId(e.target.value)}
              className="h-9 rounded-lg border border-transparent bg-input/60 px-3 text-sm"
            >
              {RUNTIME_IDS.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
            <Input
              value={customLabel}
              onChange={(e) => setCustomLabel(e.target.value)}
              placeholder="version label, e.g. 8.1"
              className="w-40"
            />
            <Button size="sm" variant="secondary" onClick={addCustomInstall}>
              <FolderSearch /> Locate executable
            </Button>
          </div>

          {customInstalls.length > 0 && (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Runtime</TableHead>
                  <TableHead>Version</TableHead>
                  <TableHead>Path</TableHead>
                  <TableHead className="text-right">Action</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {customInstalls.map((c) => (
                  <TableRow key={`${c.id}-${c.label}`}>
                    <TableCell className="font-medium uppercase">{c.id}</TableCell>
                    <TableCell>{c.label || '—'}</TableCell>
                    <TableCell className="max-w-xs truncate text-xs text-muted-foreground">{c.path}</TableCell>
                    <TableCell className="text-right">
                      <Button size="sm" variant="ghost" onClick={() => removeCustomInstall(c)}>
                        <Trash2 className="size-3.5" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

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
