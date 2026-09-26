import { open, save } from '@tauri-apps/plugin-dialog'
import { Archive, ExternalLink, FolderSearch, Import, Plus, RotateCcw, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { ErrorCard } from '@/components/ErrorCard'
import { MigrateDialog } from '@/components/MigrateDialog'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  type ConnectionInfo,
  type DbBackup,
  type DbUser,
  type ExternalTool,
  type Project,
  type ServiceStatus,
  type SqliteInfo,
  runCommand,
} from '@/core'
import { formatBytes, useAction, usePoll } from '@/lib/hooks'
import { waitForService } from '@/lib/wait'
import { confirmAction, confirmThen } from '@/lib/confirm'

type Tab = 'mariadb' | 'postgres' | 'mongodb' | 'redis' | 'sqlite' | 'tools'

/** §31–39, §102: SQL databases and users, MongoDB connection info, SQLite files, and external tools. */
export function DatabasesPage() {
  const [tab, setTab] = useState<Tab>('mariadb')
  const [migrating, setMigrating] = useState(false)
  const [services, setServices] = useState<ServiceStatus[]>([])
  const { error, setError } = useAction()

  usePoll(async () => {
    const r = await runCommand({ type: 'list_services' })
    if (r.type === 'services') setServices(r.services)
  }, 4000)

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Databases</h1>
          <p className="text-sm text-muted-foreground">Create databases and users, see connection details, and open them in your own tool.</p>
        </div>
        <Button variant="secondary" onClick={() => setMigrating(true)}>
          <Import /> Import from Laragon / XAMPP / Wamp
        </Button>
      </div>
      <MigrateDialog open={migrating} onClose={() => setMigrating(false)} />
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Tabs
        tabs={[
          { id: 'mariadb', label: 'MariaDB', icon: <TechIcon id="mariadb" /> },
          { id: 'postgres', label: 'PostgreSQL', icon: <TechIcon id="postgres" /> },
          { id: 'mongodb', label: 'MongoDB', icon: <TechIcon id="mongodb" /> },
          { id: 'redis', label: 'Redis', icon: <TechIcon id="redis" /> },
          { id: 'sqlite', label: 'SQLite', icon: <TechIcon id="sqlite" /> },
          { id: 'tools', label: 'External tools', icon: <TechIcon id="tools" /> },
        ]}
        value={tab}
        onChange={setTab}
      />
      {(tab === 'mariadb' || tab === 'postgres') && <SqlEngine key={tab} engine={tab} service={services.find((s) => s.id === tab)} />}
      {tab === 'mongodb' && <Mongo service={services.find((s) => s.id === 'mongodb')} />}
      {tab === 'redis' && <Redis service={services.find((s) => s.id === 'redis')} />}
      {tab === 'sqlite' && <Sqlite />}
      {tab === 'tools' && <Tools />}
    </div>
  )
}

function ServiceBanner({ service, name }: { service?: ServiceStatus; name: string }) {
  const [busy, setBusy] = useState(false)
  const { error, setError, run } = useAction()
  if (!service) return null
  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardContent className="flex items-center justify-between gap-3 pt-4">
          <div className="text-sm">
            <span className="font-medium">{name}</span>{' '}
            {!service.installed ? <Badge variant="outline">not installed (Runtimes page)</Badge> : service.running ? <Badge variant="success">● Running on port {service.port}</Badge> : <Badge variant="secondary">Stopped</Badge>}
            {service.connection && <div className="mt-1 font-mono text-xs text-muted-foreground">{service.connection}</div>}
          </div>
          {service.installed && (
            <Button
              size="sm"
              disabled={busy}
              onClick={async () => {
                if (service.running && !(await confirmAction(`Stop ${service.name}? Anything connected to it will be disconnected.`))) return
                setBusy(true)
                void run('svc', async () => {
                  await runCommand({ type: service.running ? 'stop_service' : 'start_service', id: service.id })
                  await waitForService(service.id, service.running ? 'stopped' : 'running')
                }).finally(() => setBusy(false))
              }}
            >
              {busy && <Spinner />}
              {busy ? (service.running ? 'Stopping…' : 'Starting…') : service.running ? 'Stop' : 'Start'}
            </Button>
          )}
        </CardContent>
      </Card>
    </>
  )
}

const ENGINE_NAMES = { mariadb: 'MariaDB', postgres: 'PostgreSQL' } as const

function SqlEngine({ engine, service }: { engine: 'mariadb' | 'postgres'; service?: ServiceStatus }) {
  const [dbs, setDbs] = useState<string[]>([])
  const [users, setUsers] = useState<DbUser[]>([])
  const [newDb, setNewDb] = useState('')
  const [user, setUser] = useState({ user: '', password: '', database: '' })
  const [info, setInfo] = useState<ConnectionInfo | null>(null)
  const [backups, setBackups] = useState<DbBackup[]>([])
  const [note, setNote] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const running = !!service?.running

  async function refresh() {
    if (!running) return
    const [d, u] = await Promise.all([runCommand({ type: 'list_databases', engine }), runCommand({ type: 'list_db_users', engine })])
    if (d.type === 'names') setDbs(d.names)
    if (u.type === 'db_users') setUsers(u.users)
    const b = await runCommand({ type: 'list_db_backups', engine, database: null })
    if (b.type === 'db_backups') setBackups(b.backups)
  }
  useEffect(() => {
    refresh().catch((e) => setError(e))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running])

  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name={ENGINE_NAMES[engine]} />
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {running && (
        <div className="grid items-start gap-4 lg:grid-cols-2">
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Databases</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex gap-2">
                <Input value={newDb} onChange={(e) => setNewDb(e.target.value)} placeholder="new_database" />
                <Button disabled={!newDb || busy !== null} onClick={() => run('db', async () => { await runCommand({ type: 'create_database', engine, name: newDb }); setNewDb(''); await refresh() })}>
                  <Plus /> Create
                </Button>
              </div>
              {dbs.length > 0 && (
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead>Database</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {dbs.map((d) => (
                      <TableRow key={d}>
                        <TableCell className="py-1.5 font-medium">{d}</TableCell>
                        <TableCell className="py-1.5 text-right">
                          <span className="inline-flex flex-wrap justify-end gap-1">
                            <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => run('backup', async () => { const r = await runCommand({ type: 'backup_database', engine, database: d }); if (r.type === 'text') setNote(`Backup saved to ${r.text}`); await refresh() })}>
                              {busy === 'backup' ? <Spinner /> : <Archive className="size-3.5" />} Back up
                            </Button>
                            <Button size="sm" variant="ghost" onClick={() => run('info', async () => { const r = await runCommand({ type: 'get_connection_info', engine, database: d, path: null }); if (r.type === 'connection') setInfo(r.info) })}>Connection</Button>
                            <Button size="sm" variant="secondary" onClick={() => run('open', () => runCommand({ type: 'open_database', engine, database: d, path: null, tool_id: null }))}>
                              <ExternalLink className="size-3.5" /> Open in tool
                            </Button>
                          </span>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
              {dbs.length === 0 && <p className="text-sm text-muted-foreground">No databases yet.</p>}
              {info && (
                <div className="rounded-lg bg-muted/40 p-3 font-mono text-xs">
                  host {info.host} · port {info.port} · user {info.user}
                  <br />
                  {info.uri}
                </div>
              )}
            </CardContent>
          </Card>
          <Card className="lg:order-last lg:col-span-2">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Backups</CardTitle>
              <CardDescription>SQL dumps kept by OpenLocalServer. Restoring first saves the current database as a new backup, so it can be undone.</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {note && <p className="text-xs text-success">{note}</p>}
              {backups.length > 0 && (
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead>Database</TableHead>
                      <TableHead>Created</TableHead>
                      <TableHead>Size</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {backups.map((b) => (
                      <TableRow key={b.file}>
                        <TableCell className="py-1.5 font-medium">{b.database}</TableCell>
                        <TableCell className="py-1.5 text-muted-foreground">{new Date(b.created * 1000).toLocaleString()}</TableCell>
                        <TableCell className="py-1.5 text-muted-foreground">{formatBytes(b.size)}</TableCell>
                        <TableCell className="py-1.5 text-right">
                          <span className="inline-flex flex-wrap justify-end gap-1">
                            <Button
                              size="sm"
                              variant="secondary"
                              disabled={busy !== null}
                              onClick={async () => {
                                if (!(await confirmAction(`Restore "${b.database}" from this backup? The tables in it are replaced. The current data is backed up first.`))) return
                                void run('restore', async () => {
                                  const r = await runCommand({ type: 'restore_database', engine, database: b.database, file: b.file })
                                  setNote(r.type === 'text' && r.text ? `Restored. The previous data is saved at ${r.text}` : 'Restored.')
                                  await refresh()
                                })
                              }}
                            >
                              {busy === 'restore' ? <Spinner /> : <RotateCcw className="size-3.5" />} Restore
                            </Button>
                            <Button size="sm" variant="ghost" title="Delete this backup" disabled={busy !== null} onClick={() => confirmThen('Delete this backup file?', () => run('delete', async () => { await runCommand({ type: 'delete_db_backup', engine, file: b.file }); await refresh() }))}>
                              <Trash2 className="size-3.5" />
                            </Button>
                          </span>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
              {backups.length === 0 && <p className="text-sm text-muted-foreground">No backups yet. Use “Back up” next to a database.</p>}
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Users</CardTitle>
              <CardDescription>Passwords are kept in the Windows credential store, never on disk.</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid gap-2 sm:grid-cols-3">
                <Input value={user.user} onChange={(e) => setUser({ ...user, user: e.target.value })} placeholder="user" />
                <Input type="password" value={user.password} onChange={(e) => setUser({ ...user, password: e.target.value })} placeholder="password" />
                <Input value={user.database} onChange={(e) => setUser({ ...user, database: e.target.value })} placeholder="database" />
              </div>
              <div>
                <Button size="sm" disabled={!user.user || !user.password || !user.database || busy !== null} onClick={() => run('user', async () => { await runCommand({ type: 'create_db_user', engine, ...user }); setUser({ user: '', password: '', database: '' }); await refresh() })}>
                  Create user with full access to that database
                </Button>
              </div>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>User</TableHead>
                    <TableHead>Host</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {users.map((u) => (
                    <TableRow key={`${u.user}@${u.host}`}>
                      <TableCell>{u.user}</TableCell>
                      <TableCell className="text-muted-foreground">{u.host}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  )
}

function Mongo({ service }: { service?: ServiceStatus }) {
  const { run, error, setError } = useAction()
  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name="MongoDB" />
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardContent className="flex items-center justify-between gap-3 pt-4 text-sm">
          <div>
            Health: {service?.running ? (service.healthy ? <Badge variant="success">answering</Badge> : <Badge variant="warning">not answering</Badge>) : <Badge variant="secondary">stopped</Badge>}
            <div className="mt-1 text-xs text-muted-foreground">Logs are on the Logs page (source: MongoDB).</div>
          </div>
          <Button size="sm" variant="secondary" onClick={() => run('open', () => runCommand({ type: 'open_database', engine: 'mongodb', database: null, path: null, tool_id: null }))}>
            <ExternalLink /> Open in tool
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}

function Redis({ service }: { service?: ServiceStatus }) {
  const [info, setInfo] = useState<ConnectionInfo | null>(null)
  const { error, setError } = useAction()
  useEffect(() => {
    runCommand({ type: 'get_connection_info', engine: 'redis', database: null, path: null })
      .then((r) => r.type === 'connection' && setInfo(r.info))
      .catch((e) => setError(e))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name="Redis" />
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardContent className="flex flex-col gap-2 pt-4 text-sm">
          <div>
            Health: {service?.running ? (service.healthy ? <Badge variant="success">answering</Badge> : <Badge variant="warning">not answering</Badge>) : <Badge variant="secondary">stopped</Badge>}
          </div>
          {info && <div className="rounded-lg bg-muted/40 p-3 font-mono text-xs">host {info.host} · port {info.port}<br />{info.uri}</div>}
          <p className="text-xs text-muted-foreground">
            Runs the Windows build of Redis from the Runtimes page (the community redis-windows project). It listens on 127.0.0.1 only. Logs are on the Logs page (source: Redis).
          </p>
        </CardContent>
      </Card>
    </div>
  )
}

function Sqlite() {
  const [dbs, setDbs] = useState<SqliteInfo[]>([])
  const [projects, setProjects] = useState<Project[]>([])
  const [projectId, setProjectId] = useState('')
  const [msg, setMsg] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  async function refresh() {
    const r = await runCommand({ type: 'list_sqlite' })
    if (r.type === 'sqlite_list') setDbs(r.databases)
  }
  useEffect(() => {
    void refresh()
    runCommand({ type: 'list_projects' }).then((r) => r.type === 'projects' && setProjects(r.projects))
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {msg && <p className="rounded-lg border border-border bg-muted/40 p-3 text-sm">{msg}</p>}
      <Card>
        <CardHeader className="flex-row flex-wrap items-center justify-between gap-2 space-y-0 pb-2">
          <CardTitle className="text-sm">SQLite databases</CardTitle>
          <div className="flex flex-wrap items-center gap-2">
            <Select value={projectId} onChange={(e) => setProjectId(e.target.value)} className="w-48">
              <option value="">— project —</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </Select>
            <Button size="sm" variant="secondary" disabled={!projectId || busy !== null} onClick={() => run('detect', async () => { const r = await runCommand({ type: 'detect_sqlite', project_id: projectId }); if (r.type === 'sqlite_list') setMsg(`Found ${r.databases.length} database file(s) in the project.`); await refresh() })}>
              Detect in project
            </Button>
            <Button size="sm" variant="secondary" onClick={() => run('add', async () => { const p = await open({ filters: [{ name: 'SQLite', extensions: ['sqlite', 'sqlite3', 'db'] }] }); if (p && !Array.isArray(p)) { await runCommand({ type: 'associate_sqlite', path: p, project_id: projectId || null }); await refresh() } })}>
              <FolderSearch /> Add existing
            </Button>
            <Button size="sm" onClick={() => run('create', async () => { const p = await save({ title: 'New SQLite database', defaultPath: 'database.sqlite' }); if (p) { await runCommand({ type: 'create_sqlite', path: p, project_id: projectId || null }); await refresh() } })}>
              <Plus /> Create
            </Button>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {dbs.map((d) => (
            <div key={d.path} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2">
              <div className="min-w-0">
                <div className="text-sm font-medium">
                  {d.name} {!d.exists && <Badge variant="destructive">missing</Badge>}
                  {d.project_id && <span className="ml-2 text-xs text-muted-foreground">{projects.find((p) => p.id === d.project_id)?.name}</span>}
                </div>
                <div className="truncate font-mono text-xs text-muted-foreground">{d.path} · {formatBytes(d.size_bytes)} · {d.backups.length} backup(s)</div>
              </div>
              <div className="flex gap-1">
                <Button size="sm" variant="ghost" onClick={() => run('check', async () => { const r = await runCommand({ type: 'check_sqlite', path: d.path }); if (r.type === 'integrity') setMsg(r.result.ok ? `${d.name}: integrity check passed.` : `${d.name}: ${r.result.detail}`) })}>Integrity</Button>
                <Button size="sm" variant="ghost" onClick={() => run('backup', async () => { const r = await runCommand({ type: 'backup_sqlite', path: d.path }); if (r.type === 'text') setMsg(`Backed up to ${r.text}`); await refresh() })}>Backup</Button>
                <Button size="sm" variant="ghost" disabled={d.backups.length === 0} onClick={() => run('restore', async () => { if (!(await confirmAction('Restore the newest backup? The current file is backed up first.'))) return; const r = await runCommand({ type: 'restore_sqlite', path: d.path, backup: d.backups[0] }); if (r.type === 'text') setMsg(`Restored. Previous file saved as ${r.text || '(none)'}`); await refresh() })}>Restore</Button>
                <Button size="sm" variant="secondary" onClick={() => run('open', () => runCommand({ type: 'open_database', engine: 'sqlite', database: null, path: d.path, tool_id: null }))}>
                  <ExternalLink className="size-3.5" /> Open
                </Button>
                <Button size="sm" variant="ghost" title="Forget (the file stays on disk)" onClick={() => confirmThen(`Forget ${d.name}? The file stays on disk.`, () => run('forget', async () => { await runCommand({ type: 'forget_sqlite', path: d.path }); await refresh() }))}>
                  <Trash2 className="size-3.5" />
                </Button>
              </div>
            </div>
          ))}
          {dbs.length === 0 && <p className="text-sm text-muted-foreground">No SQLite databases registered yet.</p>}
        </CardContent>
      </Card>
    </div>
  )
}

function Tools() {
  const [tools, setTools] = useState<ExternalTool[]>([])
  const [draft, setDraft] = useState({ id: '', name: '', engines: 'mongodb', executable: '', args: '{uri}' })
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'list_external_tools' }).then((r) => r.type === 'external_tools' && setTools(r.tools))
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Register a tool</CardTitle>
          <CardDescription>
            Used by "Open in tool". Arguments can use {'{host} {port} {user} {database} {path} {uri}'}. Without a registered tool, MariaDB and SQLite open in HeidiSQL if it's installed.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-2">
          <Field label="Id">
            <Input value={draft.id} onChange={(e) => setDraft({ ...draft, id: e.target.value })} placeholder="compass" />
          </Field>
          <Field label="Name">
            <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="MongoDB Compass" />
          </Field>
          <Field label="Engines" hint="Comma separated: mariadb, sqlite, mongodb, postgres">
            <Input value={draft.engines} onChange={(e) => setDraft({ ...draft, engines: e.target.value })} />
          </Field>
          <Field label="Arguments (space separated)">
            <Input value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} />
          </Field>
          <div className="sm:col-span-2">
            <Field label="Program">
              <div className="flex gap-2">
                <Input value={draft.executable} onChange={(e) => setDraft({ ...draft, executable: e.target.value })} />
                <Button variant="secondary" onClick={async () => { const p = await open({ filters: [{ name: 'Program', extensions: ['exe'] }] }); if (p && !Array.isArray(p)) setDraft({ ...draft, executable: p }) }}>
                  <FolderSearch /> Browse
                </Button>
              </div>
            </Field>
          </div>
          <div>
            <Button
              disabled={busy !== null || !draft.id || !draft.name || !draft.executable}
              onClick={() =>
                run('save', async () => {
                  const tool: ExternalTool = {
                    id: draft.id,
                    name: draft.name,
                    engines: draft.engines.split(',').map((s) => s.trim()).filter(Boolean),
                    executable: draft.executable,
                    args: draft.args.split(/\s+/).filter(Boolean),
                  }
                  const r = await runCommand({ type: 'save_external_tool', tool })
                  if (r.type === 'external_tools') setTools(r.tools)
                })
              }
            >
              Save tool
            </Button>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardContent className="flex flex-col gap-2 pt-4">
          {tools.map((t) => (
            <div key={t.id} className="flex items-center justify-between rounded-lg border border-border px-3 py-2 text-sm">
              <div>
                <div className="font-medium">{t.name}</div>
                <div className="font-mono text-xs text-muted-foreground">{t.engines.join(', ')} · {t.executable}</div>
              </div>
              <Button size="sm" variant="ghost" onClick={() => confirmThen(`Remove ${t.name} from the tool list? The program itself is not uninstalled.`, () => run('rm', async () => { const r = await runCommand({ type: 'remove_external_tool', id: t.id }); if (r.type === 'external_tools') setTools(r.tools) }))}>
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          ))}
          {tools.length === 0 && <p className="text-sm text-muted-foreground">No tools registered.</p>}
        </CardContent>
      </Card>
    </div>
  )
}
