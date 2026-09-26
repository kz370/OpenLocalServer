import { open } from '@tauri-apps/plugin-dialog'
import { ExternalLink, FolderSearch, Play } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CustomServices } from '@/components/CustomServices'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type DbTool, type Diagnostic, type PortStatus, type ServiceStatus, runCommand } from '@/core'
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
      setError(asDiagnostic(err))
      await refresh().catch(() => undefined)
    } finally {
      setBusy(null)
    }
  }

  async function createDatabase() {
    setError(null)
    setMessage(null)
    try {
      await runCommand({ type: 'create_database', engine: 'mariadb', name: dbName })
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

  const mariadb = services.find((s) => s.id === 'mariadb')

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Services</h1>
        <p className="text-sm text-muted-foreground">Mail testing and databases, managed like any other process.</p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

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
                      <span className="flex flex-wrap items-center gap-1.5">
                        <Badge variant="success">● Running</Badge>
                        {s.healthy === false && <Badge variant="warning">not answering</Badge>}
                        {s.port_status === 'in_use' && <Badge variant="warning">port in use</Badge>}
                      </span>
                    ) : (
                      <span className="flex flex-col items-start gap-1">
                        <span className="flex flex-wrap items-center gap-1.5">
                          <Badge variant="secondary">Stopped</Badge>
                          {s.port_status === 'in_use' && <Badge variant="warning">port in use</Badge>}
                        </span>
                        {s.installed && s.port !== null && s.port_status === 'in_use' && <PortHolder port={s.port} />}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-2">
                      {s.id === 'mailpit' && s.running && s.port !== null && (
                        <button
                          type="button"
                          className="inline-flex cursor-pointer items-center gap-1 text-xs text-primary hover:underline"
                          title={`Open Mailpit at http://127.0.0.1:${s.port}`}
                          onClick={() =>
                            runCommand({ type: 'open_url', url: `http://127.0.0.1:${s.port}` }).catch((e) => setError(e as Diagnostic))
                          }
                        >
                          Open <ExternalLink className="size-3" />
                        </button>
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

      {mariadb?.running && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Create a MariaDB database</CardTitle>
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
    </div>
  )
}

/** Who holds this port (best-effort via netstat). Propose, never kill blindly. */
function PortHolder({ port }: { port: number }) {
  const [holder, setHolder] = useState<PortStatus | null>(null)
  useEffect(() => {
    let alive = true
    const tick = () => {
      runCommand({ type: 'check_port', port })
        .then((r) => alive && r.type === 'port' && setHolder(r.status))
        .catch(() => undefined)
    }
    void tick()
    const t = setInterval(tick, 5000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [port])
  if (!holder || holder.status === 'free') return null
  // Stale PID race (holder exited between netstat and tasklist): port frees
  // on next check, so say retry instead of naming a ghost.
  if (holder.process_name === null) {
    return (
      <span className="max-w-64 text-xs text-muted-foreground" title="The holder exited already or can't be identified. Wait a few seconds, then press Start again.">
        holder gone — retry Start shortly
      </span>
    )
  }
  const pid = holder.pid !== null ? ` (PID ${holder.pid})` : ''
  return (
    <span className="max-w-64 text-xs text-muted-foreground" title="Stop that program safely (its own stop command or Windows Services), then Start here. Killing it by force can corrupt its data.">
      held by {holder.process_name}
      {pid} — stop it first, then Start
    </span>
  )
}
