import { open } from '@tauri-apps/plugin-dialog'
import { ExternalLink, FolderSearch, Play } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CustomServices } from '@/components/CustomServices'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type DbTool, type Diagnostic, type ServiceStatus, runCommand } from '@/core'
import { waitForService } from '@/lib/wait'
import { confirmAction } from '@/lib/confirm'

export function ServicesPage() {
  const [services, setServices] = useState<ServiceStatus[]>([])
  const [dbTools, setDbTools] = useState<DbTool[]>([])
  const [dbName, setDbName] = useState('my_app')
  const [error, setError] = useState<Diagnostic | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)

  async function refresh() {
    const res = await runCommand({ type: 'list_services' })
    if (res.type === 'services') setServices(res.services)
    const tools = await runCommand({ type: 'list_db_tools' })
    if (tools.type === 'db_tools') setDbTools(tools.tools)
  }

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, 3000)
    return () => clearInterval(interval)
  }, [])

  async function toggle(service: ServiceStatus) {
    if (service.running && !(await confirmAction(`Stop ${service.name}? Anything connected to it will be disconnected.`))) return
    setError(null)
    setBusy(service.id)
    try {
      await runCommand({ type: service.running ? 'stop_service' : 'start_service', id: service.id })
      await waitForService(service.id, service.running ? 'stopped' : 'running')
      await refresh()
    } catch (err) {
      setError(err as Diagnostic)
    } finally {
      setBusy(null)
    }
  }

  async function createDatabase() {
    setError(null)
    setMessage(null)
    try {
      await runCommand({ type: 'create_mysql_database', name: dbName })
      setMessage(`Database "${dbName}" created (or already existed).`)
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function openTool(tool: DbTool) {
    setError(null)
    try {
      await runCommand({ type: 'open_db_tool', id: tool.id })
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function locateTool(tool: DbTool) {
    const picked = await open({
      multiple: false,
      directory: false,
      title: `Locate ${tool.name}`,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    })
    if (!picked || Array.isArray(picked)) return
    setError(null)
    try {
      await runCommand({ type: 'set_custom_install', id: tool.id, label: '', path: picked })
      await refresh()
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  const mysql = services.find((s) => s.id === 'mysql')

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Services</h1>
        <p className="text-sm text-muted-foreground">Mail testing and databases, managed like any other process.</p>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Services</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Port</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Action</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {services.map((s) => (
                <TableRow key={s.id}>
                  <TableCell className="font-medium">{s.name}</TableCell>
                  <TableCell className="text-muted-foreground">{s.installed ? s.port : '—'}</TableCell>
                  <TableCell>
                    {!s.installed ? (
                      <Badge variant="outline">not installed</Badge>
                    ) : s.running ? (
                      <Badge variant="success">● Running</Badge>
                    ) : (
                      <Badge variant="secondary">Stopped</Badge>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-2">
                      {s.id === 'mailpit' && s.running && (
                        <a
                          href={`http://127.0.0.1:${s.port}`}
                          target="_blank"
                          rel="noreferrer"
                          className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                        >
                          Open <ExternalLink className="size-3" />
                        </a>
                      )}
                      {s.installed && (
                        <Button size="sm" variant="secondary" disabled={busy === s.id} onClick={() => toggle(s)}>
                          {busy === s.id ? (
                            <>
                              <Spinner /> {s.running ? 'Stopping…' : 'Starting…'}
                            </>
                          ) : s.running ? (
                            <>
                              <StopIcon /> Stop
                            </>
                          ) : (
                            <>
                              <Play /> Start
                            </>
                          )}
                        </Button>
                      )}
                    </div>
                  </TableCell>
                </TableRow>
              ))}
              {services.length === 0 && (
                <TableRow>
                  <TableCell colSpan={4} className="text-center text-sm text-muted-foreground">
                    No services registered.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <CustomServices onChanged={refresh} />

      {mysql?.running && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Create a MySQL database</CardTitle>
          </CardHeader>
          <CardContent className="flex items-center gap-2">
            <Input value={dbName} onChange={(e) => setDbName(e.target.value)} className="w-56" />
            <Button size="sm" variant="secondary" onClick={createDatabase}>
              Create
            </Button>
            {message && <span className="text-sm text-success">{message}</span>}
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Database GUI tools</CardTitle>
          <CardDescription>Detected on your system first — nothing downloads without you asking.</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Tool</TableHead>
                <TableHead>Path</TableHead>
                <TableHead className="text-right">Action</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {dbTools.map((t) => (
                <TableRow key={t.id}>
                  <TableCell className="font-medium">{t.name}</TableCell>
                  <TableCell className="max-w-xs truncate text-xs text-muted-foreground">
                    {t.found_path ?? '—'}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-2">
                      {!t.found_path && <Badge variant="outline">not found</Badge>}
                      {t.found_path && (
                        <Button size="sm" variant="secondary" onClick={() => openTool(t)}>
                          Open
                        </Button>
                      )}
                      <Button size="sm" variant="outline" onClick={() => locateTool(t)} title="Point at a specific install">
                        <FolderSearch className="size-3.5" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
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
