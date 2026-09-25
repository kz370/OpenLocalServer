import { listen } from '@tauri-apps/api/event'
import { Download } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type CatalogEntry, type Diagnostic, type RuntimeEvent, runCommand } from '@/core'

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function RuntimesPage() {
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [progress, setProgress] = useState<Record<string, RuntimeEvent | undefined>>({})
  const [error, setError] = useState<Diagnostic | null>(null)

  async function refresh() {
    const res = await runCommand({ type: 'list_runtime_catalog' })
    if (res.type === 'runtime_catalog') setCatalog(res.entries)
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

      <div className="grid gap-4 sm:grid-cols-2">
        {catalog.map((entry) => {
          const key = `${entry.id}@${entry.version}`
          const live = progress[key]
          const isInstalling = live && live.kind === 'progress'

          return (
            <Card key={key}>
              <CardHeader className="flex-row items-center justify-between space-y-0">
                <CardTitle>
                  {entry.name} <span className="text-muted-foreground font-normal">v{entry.version}</span>
                </CardTitle>
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
              </CardHeader>
              <CardContent className="flex flex-col gap-3">
                <CardDescription>
                  {live?.kind === 'progress' &&
                    `${formatBytes(live.downloaded)}${live.total ? ` / ${formatBytes(live.total)}` : ''}`}
                  {live?.kind === 'failed' && live.message}
                  {!live && !entry.installed && 'Windows x64 build from the vendor’s official distribution.'}
                  {!live && entry.installed && 'Ready to use in any project.'}
                </CardDescription>
                {entry.system && !entry.installed && (
                  <p className="text-xs text-muted-foreground">
                    Found on your system: <span className="text-foreground">{entry.system.version}</span> at{' '}
                    <code className="rounded bg-muted px-1 py-0.5">{entry.system.path}</code>
                    <br />
                    Not managed by OpenLocalServer — install below for per-project version control.
                  </p>
                )}
                {live?.kind === 'progress' && live.total && (
                  <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                    <div
                      className="h-full bg-primary transition-all"
                      style={{ width: `${Math.min(100, (live.downloaded / live.total) * 100)}%` }}
                    />
                  </div>
                )}
                {!entry.installed && !isInstalling && (
                  <Button size="sm" onClick={() => install(entry)} className="w-fit">
                    <Download /> Install
                  </Button>
                )}
              </CardContent>
            </Card>
          )
        })}
        {catalog.length === 0 && (
          <p className="text-sm text-muted-foreground">No runtimes in the catalog for this platform yet.</p>
        )}
      </div>

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
