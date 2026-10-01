import { open, save } from '@tauri-apps/plugin-dialog'
import { Archive, ExternalLink, FolderSearch, Import, Info, Plus, RotateCcw, Trash2 } from 'lucide-react'
import { Fragment, useEffect, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { ErrorCard } from '@/components/ErrorCard'
import { MigrateDialog } from '@/components/MigrateDialog'
import { ServiceMark, TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  type ConnectionInfo,
  type DbBackup,
  type DbTool,
  type Diagnostic,
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

type Tab = 'mariadb' | 'postgres' | 'mongodb' | 'redis' | 'memcached' | 'sqlite' | 'tools'
type DatabaseToolProps = {
  dbTools: DbTool[]
  externalTools: ExternalTool[]
  defaultTools: Record<string, string>
  setDefaultTool: (engine: string, id: string) => Promise<void>
}

/** §31–39, §102: SQL databases and users, MongoDB connection info, SQLite files, and external tools. */
export function DatabasesPage() {
  const [tab, setTab] = useState<Tab>('mariadb')
  const [migrating, setMigrating] = useState(false)
  const [services, setServices] = useState<ServiceStatus[]>([])
  const [dbTools, setDbTools] = useState<DbTool[]>([])
  const [externalTools, setExternalTools] = useState<ExternalTool[]>([])
  const [defaultTools, setDefaultTools] = useState<Record<string, string>>({})
  const { error, setError } = useAction()

  useEffect(() => {
    Promise.all([
      runCommand({ type: 'list_db_tools' }),
      runCommand({ type: 'list_external_tools' }),
      ...['mariadb', 'postgres', 'mongodb', 'redis', 'sqlite'].map((engine) => runCommand({ type: 'get_setting', key: `database.default_tool.${engine}` })),
    ]).then(([detected, external, ...settings]) => {
      if (detected.type === 'db_tools') setDbTools(detected.tools)
      if (external.type === 'external_tools') setExternalTools(external.tools)
      const defaults: Record<string, string> = {}
      const configured = new Set<string>()
      settings.forEach((setting, index) => {
        const engine = ['mariadb', 'postgres', 'mongodb', 'redis', 'sqlite'][index]
        if (setting.type === 'setting' && typeof setting.value === 'string') {
          defaults[engine] = setting.value
          configured.add(engine)
        }
      })
      const builtinTools = detected.type === 'db_tools' ? detected.tools : []
      const registeredTools = external.type === 'external_tools' ? external.tools : []
      for (const [engine, preferred] of Object.entries({ mariadb: 'heidisql', postgres: 'heidisql', mongodb: 'nosqlbooster', redis: 'tinyrdm', sqlite: 'dbbrowser' })) {
        if (configured.has(engine)) continue
        const available = builtinTools.some((tool) => tool.id === preferred && tool.found_path) || registeredTools.some((tool) => tool.id === preferred && tool.engines.includes(engine))
        if (available) defaults[engine] = preferred
        else if (engine === 'postgres' && builtinTools.some((tool) => tool.id === 'pgadmin' && tool.found_path)) defaults[engine] = 'pgadmin'
        else if (engine === 'sqlite' && (builtinTools.some((tool) => tool.id === 'heidisql' && tool.found_path) || registeredTools.some((tool) => tool.id === 'heidisql' && tool.engines.includes(engine)))) defaults[engine] = 'heidisql'
        else defaults[engine] = registeredTools.find((tool) => tool.engines.includes(engine))?.id ?? ''
      }
      setDefaultTools(defaults)
    }).catch((e) => setError(e as Diagnostic))
  }, [])

  async function setDefaultTool(engine: string, id: string) {
    setDefaultTools((current) => ({ ...current, [engine]: id }))
    await runCommand({ type: 'set_setting', key: `database.default_tool.${engine}`, value: id })
  }

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
          { id: 'memcached', label: 'Memcached', icon: <TechIcon id="memcached" /> },
          { id: 'sqlite', label: 'SQLite', icon: <TechIcon id="sqlite" /> },
          { id: 'tools', label: 'External tools', icon: <TechIcon id="tools" /> },
        ]}
        value={tab}
        onChange={setTab}
      />
      {(tab === 'mariadb' || tab === 'postgres') && <SqlEngine key={tab} engine={tab} service={services.find((s) => s.id === tab)} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'mongodb' && <Mongo service={services.find((s) => s.id === 'mongodb')} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'redis' && <Redis service={services.find((s) => s.id === 'redis')} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'memcached' && <Memcached service={services.find((s) => s.id === 'memcached')} />}
      {tab === 'sqlite' && <Sqlite dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
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
        {/* flex-wrap + min-w-0 + shrink-0 on the badge: a squeezed flex row used to
            wrap "● Running on port 3306" out of its pill and push the connection URI
            past the card edge. */}
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4">
          <div className="min-w-0 flex-1 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <ServiceMark
                id={service.id}
                state={!service.installed ? 'missing' : service.running ? (service.healthy === false ? 'unhealthy' : 'running') : 'stopped'}
              />
              <span className="font-medium">{name}</span>
              {!service.installed ? (
                <Badge variant="outline" className="shrink-0 whitespace-nowrap">not installed (Runtimes page)</Badge>
              ) : service.running ? (
                <Badge variant="success" className="shrink-0 whitespace-nowrap">● Running on port {service.port}</Badge>
              ) : (
                <Badge variant="secondary" className="shrink-0 whitespace-nowrap">Stopped</Badge>
              )}
            </div>
            {service.connection && <div className="mt-1 min-w-0 break-all font-mono text-xs text-muted-foreground">{service.connection}</div>}
          </div>
          {service.installed && (
            <Button
              size="sm"
              className="shrink-0"
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

function toolChoices(engine: string, dbTools: DbTool[], externalTools: ExternalTool[]) {
  const serves = (engines: string[]) => engines.length === 0 || engines.includes(engine)
  return [
    ...dbTools.filter((tool) => serves(tool.engines) && tool.found_path),
    ...externalTools.filter((tool) => serves(tool.engines)).map((tool) => ({ id: tool.id, name: tool.name, found_path: tool.executable, engines: tool.engines })),
  ]
}

function OpenDatabaseButton({ engine, database = null, path = null, dbTools, externalTools, defaultTools, setDefaultTool, showToolSelect = true }: DatabaseToolProps & { engine: string; database?: string | null; path?: string | null; showToolSelect?: boolean }) {
  const [note, setNote] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const choices = toolChoices(engine, dbTools, externalTools)
  const selectedTool = defaultTools[engine] ?? ''
  const toolName = choices.find((tool) => tool.id === selectedTool)?.name ?? 'tool'

  return (
    <span className="inline-flex flex-col items-end gap-1">
      <span className="inline-flex items-center gap-1">
        {showToolSelect && (
          <span className="inline-flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">Open with</span>
            <Select
              aria-label={`Open ${engine} databases with`}
              title={`Open ${engine} databases with`}
              className="h-7 w-32 shrink-0 text-xs"
              value={selectedTool}
              onChange={(event) => { void setDefaultTool(engine, event.target.value).catch((e) => setError(e as Diagnostic)) }}
            >
              <option value="">Automatic tool</option>
              {choices.map((tool) => <option key={tool.id} value={tool.id}>{tool.name}</option>)}
            </Select>
          </span>
        )}
        <Button
          size="sm"
          variant="secondary"
          className="size-7 shrink-0 p-0"
          aria-label={`Open ${database ?? path ?? 'database'} with ${toolName}`}
          title={`Open ${database ?? path ?? 'database'} with ${toolName}`}
          disabled={busy !== null}
          onClick={() => run('open', async () => {
          setNote(null)
          if (engine === 'mongodb' && selectedTool === 'nosqlbooster') {
            const connection = await runCommand({ type: 'get_connection_info', engine, database, path })
            if (connection.type === 'connection') {
              try {
                await navigator.clipboard.writeText(connection.info.uri)
              } catch {
                setNote(`Opened NoSQLBooster. Connect → From URI: ${connection.info.uri}`)
              }
            }
          }
          if (engine === 'redis' && selectedTool === 'tinyrdm') {
            const connection = await runCommand({ type: 'get_connection_info', engine, database, path })
            if (connection.type === 'connection') {
              try {
                await navigator.clipboard.writeText(connection.info.uri)
              } catch {
                setNote(`Opened tool. Connect to ${connection.info.uri} if it did not pick it up.`)
              }
            }
          }
          await runCommand({ type: 'open_database', engine, database, path, tool_id: selectedTool || null })
          if (engine === 'mongodb' && selectedTool === 'nosqlbooster') setNote((current) => current ?? 'MongoDB connection URI copied. In NoSQLBooster, choose Connect → From URI and paste.')
          if (engine === 'redis' && selectedTool === 'tinyrdm') setNote((current) => current ?? 'Redis URI copied (redis://127.0.0.1:6379). Paste it if the tool asks for a connection.')
        })}>
          {busy === 'open' ? <Spinner /> : <ExternalLink className="size-3.5" />}
        </Button>
      </span>
      {note && <span className="max-w-80 text-right text-xs text-muted-foreground">{note}</span>}
      {error && <span className="text-xs text-destructive">{error.problem}</span>}
    </span>
  )
}

function ConnectionDetails({ engine, database, path = null }: { engine: string; database: string | null; path?: string | null }) {
  const [info, setInfo] = useState<ConnectionInfo | null>(null)
  const [copied, setCopied] = useState<'uri' | null>(null)
  const { error, setError } = useAction()
  useEffect(() => {
    runCommand({ type: 'get_connection_info', engine, database, path })
      .then((r) => {
        if (r.type === 'connection') setInfo(r.info)
      })
      .catch((e) => setError(e as Diagnostic))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engine, database, path])

  if (error) {
    return <p className="px-3 py-2 text-xs text-destructive">{error.problem}{error.cause ? ` — ${error.cause}` : ''}{error.fix ? ` ${error.fix}` : ''}</p>
  }
  if (!info) {
    return <p className="px-3 py-2 text-xs text-muted-foreground">Reading connection details…</p>
  }
  const fields = [
    ['Engine', info.engine],
    ['Host', info.host],
    ['Port', info.port === null ? '—' : String(info.port)],
    ['User', info.user ?? '—'],
    ['Database', info.database ?? '—'],
    ['File', info.path],
  ].filter(([, value]) => value !== null && value !== '') as [string, string][]
  return (
    <div className="space-y-1.5 bg-muted/40 px-3 py-2 text-xs">
      <p className="font-medium text-foreground">Connection details</p>
      <dl className="grid grid-cols-[4.5rem_1fr] gap-x-2 gap-y-0.5">
        {fields.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="font-mono break-all">{value}</dd>
          </div>
        ))}
      </dl>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 font-mono break-all">{info.uri}</code>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 shrink-0 px-1.5 text-xs"
          title="Copy the connection URI"
          onClick={() => {
            void navigator.clipboard
              .writeText(info.uri)
              .then(() => setCopied('uri'))
              .catch(() => setCopied(null))
          }}
        >
          {copied === 'uri' ? 'Copied' : 'Copy URI'}
        </Button>
      </div>
    </div>
  )
}

const ENGINE_NAMES = { mariadb: 'MariaDB', postgres: 'PostgreSQL' } as const

function SqlEngine({ engine, service, ...toolProps }: { engine: 'mariadb' | 'postgres'; service?: ServiceStatus } & DatabaseToolProps) {
  const [dbs, setDbs] = useState<string[]>([])
  const [users, setUsers] = useState<DbUser[]>([])
  const [newDb, setNewDb] = useState('')
  const [newDbCreds, setNewDbCreds] = useState<'root' | 'custom'>('root')
  const [dbUser, setDbUser] = useState({ user: '', password: '' })
  const [user, setUser] = useState({ user: '', password: '', database: '' })
  const [openDb, setOpenDb] = useState<string | null>(null)
  const [backups, setBackups] = useState<DbBackup[]>([])
  const [note, setNote] = useState<string | null>(null)
  const [busyDb, setBusyDb] = useState<string | null>(null)
  const [busyBackup, setBusyBackup] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const running = !!service?.running
  const dbChoices = toolChoices(engine, toolProps.dbTools, toolProps.externalTools)

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
            <CardHeader className="flex-row flex-wrap items-center justify-between gap-2 space-y-0 pb-2">
              <CardTitle className="text-sm">Databases</CardTitle>
              {dbChoices.length > 0 && (
                <span className="inline-flex shrink-0 items-center gap-1.5">
                  <span className="text-xs text-muted-foreground">Open with</span>
                  <Select
                    aria-label={`Open ${ENGINE_NAMES[engine]} databases with`}
                    title={`Open ${ENGINE_NAMES[engine]} databases with`}
                    className="h-7 w-32 shrink-0 text-xs"
                    value={toolProps.defaultTools[engine] ?? ''}
                    onChange={(event) => void toolProps.setDefaultTool(engine, event.target.value).catch((e) => setError(e as Diagnostic))}
                  >
                    <option value="">Automatic tool</option>
                    {dbChoices.map((tool) => (
                      <option key={tool.id} value={tool.id}>{tool.name}</option>
                    ))}
                  </Select>
                </span>
              )}
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex gap-2">
                <Input value={newDb} onChange={(e) => setNewDb(e.target.value)} placeholder="new_database" />
                <Button
                  disabled={!newDb || busy !== null || (newDbCreds === 'custom' && (!dbUser.user || !dbUser.password))}
                  onClick={() =>
                    run('db', async () => {
                      await runCommand({
                        type: 'create_database',
                        engine,
                        name: newDb,
                        user: newDbCreds === 'custom' ? dbUser.user : null,
                        password: newDbCreds === 'custom' ? dbUser.password : null,
                      })
                      setNewDb('')
                      setDbUser({ user: '', password: '' })
                      setNote(
                        newDbCreds === 'custom'
                          ? `Database "${newDb}" and user "${dbUser.user}" created. The password is in the Windows credential store.`
                          : `Database "${newDb}" created (or already existed), owned by the ${engine === 'postgres' ? 'postgres' : 'root'} account.`,
                      )
                      await refresh()
                    })
                  }
                >
                  <Plus /> Create
                </Button>
              </div>
              <div className="flex flex-col gap-2">
                <Select
                  aria-label="Who owns the new database"
                  title="Who owns the new database"
                  className="h-8 w-full text-xs"
                  value={newDbCreds}
                  onChange={(e) => setNewDbCreds(e.target.value as 'root' | 'custom')}
                >
                  <option value="root">Sign in as {engine === 'postgres' ? 'postgres' : 'root'} (no password)</option>
                  <option value="custom">Create a user with a password I choose</option>
                </Select>
                {newDbCreds === 'custom' && (
                  <div className="flex flex-wrap gap-2">
                    <Input
                      aria-label="New database user name"
                      className="w-40"
                      value={dbUser.user}
                      onChange={(e) => setDbUser({ ...dbUser, user: e.target.value })}
                      placeholder="app_user"
                    />
                    <Input
                      aria-label="Password for the new user"
                      type="password"
                      autoComplete="new-password"
                      className="w-48"
                      value={dbUser.password}
                      onChange={(e) => setDbUser({ ...dbUser, password: e.target.value })}
                      placeholder="password"
                    />
                    <span className="self-center text-xs text-muted-foreground">
                      Owns the new database only. Kept in the Windows credential store, never on disk.
                    </span>
                  </div>
                )}
              </div>
              {dbs.length > 0 && (
                <div className="divide-y divide-border overflow-hidden rounded-lg border border-border">
                  {dbs.map((d) => (
                    <Fragment key={d}>
                      <div className="flex items-center justify-between gap-2 px-3 py-1">
                      <span className="truncate text-sm font-medium">{d}</span>
                      <span className="flex shrink-0 items-center gap-0.5">
                        <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" title="Back up this database" disabled={busy !== null} onClick={() => { setBusyDb(d); void run('backup', async () => { const r = await runCommand({ type: 'backup_database', engine, database: d }); if (r.type === 'text') setNote(`Backup saved to ${r.text}`); await refresh() }).finally(() => setBusyDb(null)) }}>
                          {busyDb === d ? <Spinner /> : <Archive className="size-3.5" />} Back up
                        </Button>
                        <Button
                          size="sm"
                          variant={openDb === d ? 'secondary' : 'ghost'}
                          className="h-7 w-7 shrink-0 p-0"
                          aria-label={`Connection details for ${d}`}
                          aria-expanded={openDb === d}
                          title="Show connection details (host, port, user, URI)"
                          onClick={() => setOpenDb(openDb === d ? null : d)}
                        >
                          <Info className="size-3.5" />
                        </Button>
                        <OpenDatabaseButton engine={engine} database={d} {...toolProps} showToolSelect={false} />
                      </span>
                      </div>
                      {openDb === d && <ConnectionDetails engine={engine} database={d} />}
                    </Fragment>
                  ))}
                </div>
              )}
              {dbs.length === 0 && <p className="text-sm text-muted-foreground">No databases yet.</p>}
            </CardContent>
          </Card>
          <Card className="lg:order-last lg:col-span-2">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Backups</CardTitle>
              <CardDescription>SQL dumps kept by OLS. Restoring first saves the current database as a new backup, so it can be undone.</CardDescription>
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
                                setBusyBackup(b.file)
                                void run('restore', async () => {
                                  const r = await runCommand({ type: 'restore_database', engine, database: b.database, file: b.file })
                                  setNote(r.type === 'text' && r.text ? `Restored. The previous data is saved at ${r.text}` : 'Restored.')
                                  await refresh()
                                }).finally(() => setBusyBackup(null))
                              }}
                            >
                              {busyBackup === b.file ? <Spinner /> : <RotateCcw className="size-3.5" />} Restore
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
          <Card className="@container">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Users</CardTitle>
              <CardDescription>Passwords are kept in the Windows credential store, never on disk.</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="grid grid-cols-1 gap-2 @lg:grid-cols-3">
                <Field label="User">
                  <Input value={user.user} onChange={(e) => setUser({ ...user, user: e.target.value })} placeholder="user" />
                </Field>
                <Field label="Password">
                  <Input type="password" value={user.password} onChange={(e) => setUser({ ...user, password: e.target.value })} placeholder="password" />
                </Field>
                <Field label="Database">
                  <Input value={user.database} onChange={(e) => setUser({ ...user, database: e.target.value })} placeholder="database" />
                </Field>
              </div>
              <div>
                <Button size="sm" className="w-full @lg:w-auto" disabled={!user.user || !user.password || !user.database || busy !== null} onClick={() => run('user', async () => { await runCommand({ type: 'create_db_user', engine, ...user }); setUser({ user: '', password: '', database: '' }); await refresh() })}>
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

function Mongo({ service, ...toolProps }: { service?: ServiceStatus } & DatabaseToolProps) {
  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name="MongoDB" />
      <Card>
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4 text-sm">
          <div className="min-w-0">
            Health: {service?.running ? (service.healthy ? <Badge variant="success" className="shrink-0 whitespace-nowrap">answering</Badge> : <Badge variant="warning" className="shrink-0 whitespace-nowrap">not answering</Badge>) : <Badge variant="secondary" className="shrink-0 whitespace-nowrap">stopped</Badge>}
            <div className="mt-1 text-xs text-muted-foreground">Logs are on the Logs page (source: MongoDB).</div>
          </div>
          <OpenDatabaseButton engine="mongodb" {...toolProps} />
        </CardContent>
      </Card>
    </div>
  )
}

function Redis({ service, ...toolProps }: { service?: ServiceStatus } & DatabaseToolProps) {
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
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4 text-sm">
          <div className="min-w-0">
            Health: {service?.running ? (service.healthy ? <Badge variant="success" className="shrink-0 whitespace-nowrap">answering</Badge> : <Badge variant="warning" className="shrink-0 whitespace-nowrap">not answering</Badge>) : <Badge variant="secondary" className="shrink-0 whitespace-nowrap">stopped</Badge>}
            <div className="mt-1 text-xs text-muted-foreground">Logs are on the Logs page (source: Redis).</div>
          </div>
          <OpenDatabaseButton engine="redis" {...toolProps} />
        </CardContent>
      </Card>
      <Card>
        <CardContent className="flex flex-col gap-2 pt-4 text-sm">
          {info && <div className="min-w-0 rounded-lg bg-muted/40 p-3 font-mono text-xs break-all">host {info.host} · port {info.port}<br />{info.uri}</div>}
          <p className="text-xs text-muted-foreground">
            Runs the Windows build of Redis from the Runtimes page (the community redis-windows project). It listens on 127.0.0.1 only. Logs are on the Logs page (source: Redis).
          </p>
        </CardContent>
      </Card>
    </div>
  )
}

/** A key/value cache: no GUI tool ships for it, so the panel is status + connection only. */
function Memcached({ service }: { service?: ServiceStatus }) {
  const [info, setInfo] = useState<ConnectionInfo | null>(null)
  const { error, setError } = useAction()
  useEffect(() => {
    runCommand({ type: 'get_connection_info', engine: 'memcached', database: null, path: null })
      .then((r) => r.type === 'connection' && setInfo(r.info))
      .catch((e) => setError(e))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name="Memcached" />
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4 text-sm">
          <div className="min-w-0">
            Health:{' '}
            {service?.running ? (
              service.healthy ? <Badge variant="success" className="shrink-0 whitespace-nowrap">answering</Badge> : <Badge variant="warning" className="shrink-0 whitespace-nowrap">not answering</Badge>
            ) : (
              <Badge variant="secondary" className="shrink-0 whitespace-nowrap">stopped</Badge>
            )}
            <div className="mt-1 text-xs text-muted-foreground">Logs are on the Logs page (source: Memcached).</div>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardContent className="flex flex-col gap-2 pt-4 text-sm">
          {info && (
            <div className="min-w-0 rounded-lg bg-muted/40 p-3 font-mono text-xs break-all">
              host {info.host} · port {info.port}
              <br />
              {info.uri}
            </div>
          )}
          <p className="text-xs text-muted-foreground">
            Runs the Windows build of Memcached from the Runtimes page (the community native port). It listens on 127.0.0.1 only and keeps
            everything in RAM, so its memory cap lives in Settings → Resources.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}

function Sqlite(toolProps: DatabaseToolProps) {
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
                <OpenDatabaseButton engine="sqlite" path={d.path} {...toolProps} />
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

/** Every engine a registered tool may claim — mirrors `KNOWN_ENGINES` in `dbtools.rs`. */
const TOOL_ENGINES = [
  { id: 'mariadb', label: 'MariaDB' },
  { id: 'postgres', label: 'PostgreSQL' },
  { id: 'mongodb', label: 'MongoDB' },
  { id: 'redis', label: 'Redis' },
  { id: 'memcached', label: 'Memcached' },
  { id: 'sqlite', label: 'SQLite' },
] as const

function Tools() {
  const [tools, setTools] = useState<ExternalTool[]>([])
  const [draft, setDraft] = useState({ id: '', name: '', engines: [] as string[], executable: '', args: '{uri}' })
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'list_external_tools' }).then((r) => r.type === 'external_tools' && setTools(r.tools))
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Register a database explorer</CardTitle>
          <CardDescription>
            Any program that opens a database — an explorer of your own, or a vendor one — as long as it is an .exe. It is then
            listed in “Open with” on every engine tab you tick, and chosen per engine by the dropdown there. Arguments can use{' '}
            {'{host} {port} {user} {database} {path} {uri}'}. Without a registered tool, MariaDB opens in HeidiSQL if it's
            installed; SQLite opens in DB Browser for SQLite, then HeidiSQL.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 sm:grid-cols-2">
          <Field label="Id" hint="Lower-case name, no spaces. Saving with an existing id replaces that tool.">
            <Input value={draft.id} onChange={(e) => setDraft({ ...draft, id: e.target.value })} placeholder="custom_db_explorer" />
          </Field>
          <Field label="Name">
            <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="My DB Explorer" />
          </Field>
          {/* Chips, not a comma-separated text box: a free-text engine list accepted
              "MariaDB" or "mysql", which then matched no engine and left the tool
              invisible on every page — a silent failure that read as a broken button. */}
          <Field
            className="sm:col-span-2"
            label="Engines it opens"
            hint="Tick the tabs it should appear on. None ticked offers it everywhere — the safe default, since a tool that can open nothing is never picked."
          >
            <div className="flex flex-wrap gap-x-4 gap-y-2">
              {TOOL_ENGINES.map((e) => (
                <label key={e.id} className="inline-flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={draft.engines.includes(e.id)}
                    onChange={(on) => setDraft({ ...draft, engines: on ? [...draft.engines, e.id] : draft.engines.filter((x) => x !== e.id) })}
                    label={`${e.label} — open with ${draft.name || 'this tool'}`}
                  />
                  {e.label}
                </label>
              ))}
            </div>
          </Field>
          <Field label="Arguments (space separated)" hint="Each space-separated word is one argument.">
            <Input value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} />
          </Field>
          <div className="sm:col-span-2">
            <Field label="Program" hint="The .exe to run. It must exist at that path.">
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
                    engines: draft.engines,
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
            <div key={t.id} className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2 text-sm">
              <div className="min-w-0">
                <div className="font-medium">{t.name}</div>
                <div className="min-w-0 font-mono text-xs break-all text-muted-foreground">
                  {t.engines.length === 0 ? 'every engine' : t.engines.join(', ')} · {t.executable}
                </div>
              </div>
              <Button size="sm" variant="ghost" className="shrink-0" title="Remove this tool" onClick={() => confirmThen(`Remove ${t.name} from the tool list? The program itself is not uninstalled.`, () => run('rm', async () => { const r = await runCommand({ type: 'remove_external_tool', id: t.id }); if (r.type === 'external_tools') setTools(r.tools) }))}>
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
