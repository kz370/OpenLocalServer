import { open, save } from '@tauri-apps/plugin-dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'
import {
  AlertCircle,
  Archive,
  Download,
  ExternalLink,
  FileUp,
  FolderOpen,
  FolderSearch,
  Import,
  Info,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  Sparkles,
  Trash2,
  Upload,
  Wrench,
} from 'lucide-react'
import { Fragment, useEffect, useState } from 'react'

import { PageLayout } from '@/components/PageLayout'
import { Spinner } from '@/components/Spinner'
import { ErrorCard } from '@/components/ErrorCard'
import { MigrateDialog } from '@/components/MigrateDialog'
import { ServiceMark, TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { ToolArgsEditor } from '@/components/ToolArgsEditor'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  type ConnectionInfo,
  type DbBackup,
  type DbTool,
  type Diagnostic,
  type DbUser,
  type ToolArgs,
  type ExternalTool,
  type Project,
  type ServiceStatus,
  type SqliteInfo,
  runCommand,
} from '@/core'
import { formatBytes, useAction, usePoll } from '@/lib/hooks'
import { createPersistentCache } from '@/lib/cache'
import { getServiceLifecycleState, useServiceStatuses } from '@/lib/serviceStatus'
import { waitForService } from '@/lib/wait'
import { confirmAction, confirmThen } from '@/lib/confirm'
import { notify } from '@/lib/notify'
import presetsCatalog from '@/data/presets.json'
import {
  argsAreEmpty,
  argsSections,
  cleanArgs,
  exportToolsJson,
  slugify,
  summarizeArgs,
  validateImportJson,
  type ToolDefinition,
} from '@/lib/externalTools'

type Tab = 'mariadb' | 'postgres' | 'mongodb' | 'redis' | 'memcached' | 'sqlite' | 'tools'
type DatabaseToolProps = {
  dbTools: DbTool[]
  externalTools: ExternalTool[]
  defaultTools: Record<string, string>
  setDefaultTool: (engine: string, id: string) => Promise<void>
}

/** §31–39, §102: SQL databases and users, MongoDB connection info, SQLite files, and external tools. */
const dbServicesCache = createPersistentCache<ServiceStatus[]>('databases_services', [])
const dbToolsCache = createPersistentCache<DbTool[]>('databases_tools', [])
const externalToolsCache = createPersistentCache<ExternalTool[]>('databases_external_tools', [])
const defaultToolsCache = createPersistentCache<Record<string, string>>('databases_default_tools', {})
const sqlDbsCache = createPersistentCache<Record<string, string[]>>('databases_sql_dbs', {})
const sqlUsersCache = createPersistentCache<Record<string, DbUser[]>>('databases_sql_users', {})
const sqlBackupsCache = createPersistentCache<Record<string, DbBackup[]>>('databases_sql_backups', {})

export function DatabasesPage() {
  const [tab, setTab] = useState<Tab>('mariadb')
  const [migrating, setMigrating] = useState(false)
  const [services, setServices] = useState<ServiceStatus[]>(dbServicesCache.get)
  const [dbTools, setDbTools] = useState<DbTool[]>(dbToolsCache.get)
  const [externalTools, setExternalTools] = useState<ExternalTool[]>(externalToolsCache.get)
  const [defaultTools, setDefaultTools] = useState<Record<string, string>>(defaultToolsCache.get)
  const { error, setError } = useAction()

  useEffect(() => {
    Promise.all([
      runCommand({ type: 'list_db_tools' }),
      runCommand({ type: 'list_external_tools' }),
      ...['mariadb', 'postgres', 'mongodb', 'redis', 'sqlite'].map((engine) => runCommand({ type: 'get_setting', key: `database.default_tool.${engine}` })),
    ]).then(([detected, external, ...settings]) => {
      if (detected.type === 'db_tools') {
        setDbTools(detected.tools)
        dbToolsCache.set(detected.tools)
      }
      if (external.type === 'external_tools') {
        setExternalTools(external.tools)
        externalToolsCache.set(external.tools)
      }
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
      defaultToolsCache.set(defaults)
    }).catch((e) => setError(e as Diagnostic))
  }, [])

  async function setDefaultTool(engine: string, id: string) {
    setDefaultTools((current) => {
      const next = { ...current, [engine]: id }
      defaultToolsCache.set(next)
      return next
    })
    await runCommand({ type: 'set_setting', key: `database.default_tool.${engine}`, value: id })
  }

  usePoll(async () => {
    const r = await runCommand({ type: 'list_services' })
    if (r.type === 'services') {
      setServices(r.services)
      dbServicesCache.set(r.services)
    }
  }, 4000)

  return (
    <PageLayout
      mode="scroll"
      resetKey={tab}
      header={
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Databases</h1>
          <p className="text-sm text-muted-foreground">Create databases and users, see connection details, and open them in your own tool.</p>
        </div>
        <Button variant="secondary" onClick={() => setMigrating(true)}>
          <Import /> Import databases
        </Button>
      </div>
      }
      toolbar={
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
      }
    >
      <MigrateDialog open={migrating} onClose={() => setMigrating(false)} />
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-col gap-6">
      {(tab === 'mariadb' || tab === 'postgres') && <SqlEngine key={tab} engine={tab} service={services.find((s) => s.id === tab)} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'mongodb' && <Mongo service={services.find((s) => s.id === 'mongodb')} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'redis' && <Redis service={services.find((s) => s.id === 'redis')} dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'memcached' && <Memcached service={services.find((s) => s.id === 'memcached')} />}
      {tab === 'sqlite' && <Sqlite dbTools={dbTools} externalTools={externalTools} defaultTools={defaultTools} setDefaultTool={setDefaultTool} />}
      {tab === 'tools' && (
        <Tools
          dbTools={dbTools}
          externalTools={externalTools}
          onToolsUpdated={(tools) => {
            setExternalTools(tools)
            externalToolsCache.set(tools)
          }}
        />
      )}
      </div>
    </PageLayout>
  )
}

function ServiceBanner({ service, name }: { service?: ServiceStatus; name: string }) {
  const { error, setError, run } = useAction()
  const serviceStatuses = useServiceStatuses()
  if (!service) return null
  const state = getServiceLifecycleState(service, serviceStatuses)
  const isStarting = state === 'starting' || state === 'restarting'
  const isStopping = state === 'stopping'
  const isPending = isStarting || isStopping
  const isRunning = state === 'running' || (isStopping && service.running)
  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <Card>
        {/* flex-wrap + min-w-0 + shrink-0 on the badge: a squeezed flex row used to
            wrap "● Running on port 3306" out of its pill and push the connection URI
            past the card edge. */}
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4">
          <div className="min-w-0 flex-1 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <ServiceMark
                id={service.id}
                state={
                  !service.installed
                    ? 'missing'
                    : isStarting
                      ? 'starting'
                      : isStopping
                        ? 'stopping'
                        : state === 'error'
                          ? 'unhealthy'
                          : isRunning
                            ? service.healthy === false
                              ? 'unhealthy'
                              : 'running'
                            : 'stopped'
                }
              />
              <span className="font-medium">{name}</span>
              {!service.installed ? (
                <Badge variant="outline" className="shrink-0 whitespace-nowrap">not installed (Runtimes page)</Badge>
              ) : isStarting ? (
                <Badge variant="secondary" className="shrink-0 whitespace-nowrap">{state === 'restarting' ? 'Restarting…' : 'Starting…'}</Badge>
              ) : isStopping ? (
                <Badge variant="secondary" className="shrink-0 whitespace-nowrap">Stopping…</Badge>
              ) : isRunning ? (
                <Badge variant="success" className="shrink-0 whitespace-nowrap">● Running on port {service.port}</Badge>
              ) : state === 'error' ? (
                <Badge variant="destructive" className="shrink-0 whitespace-nowrap">Error</Badge>
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
              disabled={isPending}
              onClick={async () => {
                if (isRunning && !(await confirmAction(`Stop ${service.name}? Anything connected to it will be disconnected.`))) return
                await run('svc', async () => {
                  await runCommand({ type: isRunning ? 'stop_service' : 'start_service', id: service.id })
                  await waitForService(service.id, isRunning ? 'stopped' : 'running')
                })
              }}
            >
              {isPending && <Spinner />}
              {isPending ? (isStarting ? 'Starting…' : 'Stopping…') : isRunning ? 'Stop' : 'Start'}
            </Button>
          )}
        </CardContent>
      </Card>
    </>
  )
}

function toolChoices(engine: string, dbTools: DbTool[], externalTools: ExternalTool[]) {
  const serves = (engines: string[]) => engines.length === 0 || engines.includes(engine)
  const map = new Map<
    string,
    { id: string; name: string; found_path: string; engines: string[]; available: boolean; launchMode: 'args' | 'clipboard' }
  >()

  // Registered external tools
  for (const tool of externalTools) {
    if (serves(tool.engines)) {
      map.set(tool.id, {
        id: tool.id,
        name: tool.name,
        found_path: tool.executable,
        engines: tool.engines,
        available: tool.available !== false && Boolean(tool.executable),
        launchMode: tool.launchMode ?? 'args',
      })
    }
  }

  // Built-in detected tools (if not already registered)
  for (const tool of dbTools) {
    if (serves(tool.engines) && !map.has(tool.id)) {
      map.set(tool.id, {
        id: tool.id,
        name: tool.name,
        found_path: tool.found_path ?? '',
        engines: tool.engines,
        available: Boolean(tool.found_path),
        launchMode: ['nosqlbooster', 'tinyrdm', 'pgadmin'].includes(tool.id) ? 'clipboard' : 'args',
      })
    }
  }

  return Array.from(map.values())
}

function OpenDatabaseButton({ engine, database = null, path = null, dbTools, externalTools, defaultTools, setDefaultTool, showToolSelect = true }: DatabaseToolProps & { engine: string; database?: string | null; path?: string | null; showToolSelect?: boolean }) {
  const { busy, setError, run } = useAction()
  const choices = toolChoices(engine, dbTools, externalTools)
  const selectedTool = defaultTools[engine] ?? ''
  const selectedChoice = choices.find((tool) => tool.id === selectedTool)
  const toolName = selectedChoice?.name ?? 'tool'
  const isAvailable = selectedChoice ? selectedChoice.available : true

  return (
    <span className="inline-flex shrink-0 items-center gap-1">
      {showToolSelect && (
        <span className="inline-flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">Open with</span>
          <Select
            aria-label={`Open ${engine} databases with`}
            title={`Open ${engine} databases with`}
            className="h-7 w-44 sm:w-48 shrink-0 text-xs"
            value={selectedTool}
            onChange={(event) => { void setDefaultTool(engine, event.target.value).catch((e) => setError(e as Diagnostic)) }}
          >
            <option value="">Automatic tool</option>
            {choices.map((tool) => (
              <option
                key={tool.id}
                value={tool.id}
                disabled={!tool.available}
                title={!tool.available ? `${tool.name} (Executable not found)` : undefined}
              >
                {tool.name} {!tool.available ? '(Executable not found)' : ''}
              </option>
            ))}
          </Select>
        </span>
      )}
      <Button
        size="sm"
        variant="secondary"
        className="size-7 shrink-0 p-0"
        aria-label={`Open ${database ?? path ?? 'database'} with ${toolName}`}
        title={!isAvailable ? `${toolName} executable not found. Fix path under External tools.` : `Open ${database ?? path ?? 'database'} with ${toolName}`}
        disabled={busy !== null || !isAvailable}
        onClick={() => run('open', async () => {
          if (selectedChoice && !selectedChoice.available) {
            const diag: Diagnostic = {
              problem: `Executable not found for ${selectedChoice.name}.`,
              cause: selectedChoice.found_path ? `The file "${selectedChoice.found_path}" does not exist on disk.` : 'No executable path has been chosen.',
              fix: 'Open the External tools tab and click "Fix path" to locate the executable.',
            }
            setError(diag)
            notify.error(diag.problem, { details: diag.fix ?? undefined })
            return
          }

          const isClipboard = selectedChoice?.launchMode === 'clipboard'
            || (engine === 'mongodb' && (selectedTool === 'nosqlbooster' || !selectedChoice?.found_path))
            || (engine === 'redis' && (selectedTool === 'tinyrdm' || !selectedChoice?.found_path))
          if (isClipboard) {
            const connection = await runCommand({ type: 'get_connection_info', engine, database, path })
            if (connection.type === 'connection') {
              try {
                await navigator.clipboard.writeText(connection.info.uri)
                const sanitized = connection.info.uri.replace(/:[^:@/]+@/, ':***@')
                notify.success(`Connection URI copied to clipboard (${sanitized}). Paste it into ${toolName}.`)
              } catch {
                notify.info(`Opened ${toolName}. Connect using ${connection.info.uri.replace(/:[^:@/]+@/, ':***@')}`)
              }
            }
          }
          await runCommand({ type: 'open_database', engine, database, path, tool_id: selectedTool || null })
        })}>
        {busy === 'open' ? <Spinner /> : <ExternalLink className="size-3.5" />}
      </Button>
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
    return <p className="min-h-36 px-3 py-2 text-xs text-destructive">{error.problem}{error.cause ? ` — ${error.cause}` : ''}{error.fix ? ` ${error.fix}` : ''}</p>
  }
  if (!info) {
    return (
      <div className="min-h-36 space-y-2 bg-muted/40 px-3 py-2" role="status" aria-label="Reading connection details…">
        <p className="text-xs text-muted-foreground">Reading connection details…</p>
        {Array.from({ length: 6 }, (_, index) => (
          <div key={index} className="flex items-center gap-2">
            <span className="h-3 w-16 animate-pulse rounded bg-muted" />
            <span className="h-3 w-2/3 animate-pulse rounded bg-muted" />
          </div>
        ))}
      </div>
    )
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
    <div className="min-h-36 space-y-1.5 bg-muted/40 px-3 py-2 text-xs">
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
  const [dbs, setDbs] = useState<string[]>(() => sqlDbsCache.get()[engine] ?? [])
  const [users, setUsers] = useState<DbUser[]>(() => sqlUsersCache.get()[engine] ?? [])
  const [newDb, setNewDb] = useState('')
  const [newDbCreds, setNewDbCreds] = useState<'root' | 'custom'>('root')
  const [dbUser, setDbUser] = useState({ user: '', password: '' })
  const [user, setUser] = useState({ user: '', password: '', database: '' })
  const [openDb, setOpenDb] = useState<string | null>(null)
  const [backups, setBackups] = useState<DbBackup[]>(() => sqlBackupsCache.get()[engine] ?? [])
  const [busyDb, setBusyDb] = useState<string | null>(null)
  const [busyBackup, setBusyBackup] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const running = !!service?.running
  const dbChoices = toolChoices(engine, toolProps.dbTools, toolProps.externalTools)

  async function refresh() {
    if (!running) return
    const [d, u] = await Promise.all([runCommand({ type: 'list_databases', engine }), runCommand({ type: 'list_db_users', engine })])
    if (d.type === 'names') {
      setDbs(d.names)
      sqlDbsCache.set((all) => ({ ...all, [engine]: d.names }))
    }
    if (u.type === 'db_users') {
      setUsers(u.users)
      sqlUsersCache.set((all) => ({ ...all, [engine]: u.users }))
    }
    const b = await runCommand({ type: 'list_db_backups', engine, database: null })
    if (b.type === 'db_backups') {
      setBackups(b.backups)
      sqlBackupsCache.set((all) => ({ ...all, [engine]: b.backups }))
    }
  }
  useEffect(() => {
    refresh().catch((e) => setError(e))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running])

  usePoll(async () => {
    if (running) await refresh().catch(() => undefined)
  }, 4000)

  return (
    <div className="flex flex-col gap-4">
      <ServiceBanner service={service} name={ENGINE_NAMES[engine]} />
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      {running && (
        <div className="grid items-start gap-4 lg:grid-cols-2">
          <Card>
            <CardHeader className="flex-row flex-wrap items-center justify-between gap-2 space-y-0 pb-5">
              <CardTitle className="text-sm">Databases</CardTitle>
              {dbChoices.length > 0 && (
                <span className="inline-flex shrink-0 items-center gap-1.5">
                  <span className="text-xs text-muted-foreground">Open with</span>
                  <Select
                    aria-label={`Open ${ENGINE_NAMES[engine]} databases with`}
                    title={`Open ${ENGINE_NAMES[engine]} databases with`}
                    className="h-7 w-44 sm:w-48 shrink-0 text-xs"
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
                      const dbName = newDb.trim()
                      if (dbName) {
                        setDbs((prev) => {
                          const next = prev.includes(dbName) ? prev : [...prev, dbName]
                          sqlDbsCache.set((all) => ({ ...all, [engine]: next }))
                          return next
                        })
                      }
                      await runCommand({
                        type: 'create_database',
                        engine,
                        name: newDb,
                        user: newDbCreds === 'custom' ? dbUser.user : null,
                        password: newDbCreds === 'custom' ? dbUser.password : null,
                      })
                      setNewDb('')
                      setDbUser({ user: '', password: '' })
                      notify.success(
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
                      <div className="flex items-center justify-between gap-2 px-3 py-1.5 min-w-0">
                        <span className="truncate text-sm font-medium min-w-0 flex-1">{d}</span>
                        <span className="flex shrink-0 items-center gap-1">
                          <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" title="Back up this database" disabled={busy !== null} onClick={() => { setBusyDb(d); void run('backup', async () => { const r = await runCommand({ type: 'backup_database', engine, database: d }); if (r.type === 'text') notify.success(`Backup saved to ${r.text}`); await refresh() }).finally(() => setBusyDb(null)) }}>
                            {busyDb === d ? <Spinner /> : <Archive className="size-3.5" />} Back up
                          </Button>
                          <Button
                            size="sm"
                            variant={openDb === d ? 'secondary' : 'ghost'}
                            className="size-7 shrink-0 p-0"
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
                                  notify.success(r.type === 'text' && r.text ? `Restored. The previous data is saved at ${r.text}` : 'Restored.')
                                  await refresh()
                                }).finally(() => setBusyBackup(null))
                              }}
                            >
                              {busyBackup === b.file ? <Spinner /> : <RotateCcw className="size-3.5" />} Restore
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              title="Delete this backup"
                              disabled={busy !== null}
                              onClick={() =>
                                confirmThen('Delete this backup file?', () =>
                                  run('delete', async () => {
                                    setBackups((prev) => {
                                      const next = prev.filter((x) => x.file !== b.file)
                                      sqlBackupsCache.set((all) => ({ ...all, [engine]: next }))
                                      return next
                                    })
                                    await runCommand({ type: 'delete_db_backup', engine, file: b.file })
                                    await refresh()
                                  }),
                                )
                              }
                            >
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
                <Button
                  size="sm"
                  className="w-full @lg:w-auto"
                  disabled={!user.user || !user.password || !user.database || busy !== null}
                  onClick={() =>
                    run('user', async () => {
                      const newUser = user.user.trim()
                      if (newUser) {
                        setUsers((prev) => {
                          const next = [...prev, { user: newUser, host: '%', databases: [user.database] }]
                          sqlUsersCache.set((all) => ({ ...all, [engine]: next }))
                          return next
                        })
                      }
                      await runCommand({ type: 'create_db_user', engine, ...user })
                      setUser({ user: '', password: '', database: '' })
                      await refresh()
                    })
                  }
                >
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
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
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
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
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
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      {msg && <p className="rounded-lg border border-border bg-muted/40 p-3 text-sm">{msg}</p>}
      <Card>
        <CardHeader className="flex-row flex-wrap items-center justify-between gap-2 space-y-0 pb-5">
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
              <div className="flex shrink-0 items-center gap-1">
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

interface ToolsProps {
  dbTools: DbTool[]
  externalTools: ExternalTool[]
  onToolsUpdated: (tools: ExternalTool[]) => void
}

function Tools({ dbTools, externalTools, onToolsUpdated }: ToolsProps) {
  const [tools, setTools] = useState<ExternalTool[]>(externalTools)
  const [presetSearch, setPresetSearch] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [draft, setDraft] = useState({
    id: '',
    name: '',
    engines: [] as string[],
    launchMode: 'args' as 'args' | 'clipboard',
    executable: '',
    args: ['{uri}'] as ToolArgs,
    isCustomId: false,
  })

  // Import modal state
  const [importOpen, setImportOpen] = useState(false)
  const [importDragActive, setImportDragActive] = useState(false)
  const [importError, setImportError] = useState<Diagnostic | null>(null)
  const [importPending, setImportPending] = useState<ToolDefinition[] | null>(null)
  const [duplicateActions, setDuplicateActions] = useState<Record<string, 'skip' | 'overwrite' | 'rename'>>({})

  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'list_external_tools' }).then((r) => {
      if (r.type === 'external_tools') {
        setTools(r.tools)
        onToolsUpdated(r.tools)
      }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  function resetDraft() {
    setEditingId(null)
    setDraft({
      id: '',
      name: '',
      engines: [],
      launchMode: 'args',
      executable: '',
      args: ['{uri}'],
      isCustomId: false,
    })
  }

  function handleNameChange(name: string) {
    if (!draft.isCustomId && !editingId) {
      setDraft((curr) => ({
        ...curr,
        name,
        id: slugify(name),
      }))
    } else {
      setDraft((curr) => ({ ...curr, name }))
    }
  }

  function handleIdChange(id: string) {
    setDraft((curr) => ({
      ...curr,
      id: id.toLowerCase().replace(/\s+/g, '-'),
      isCustomId: true,
    }))
  }

  function applyPreset(p: ToolDefinition) {
    const existing = tools.find((t) => t.id === p.id)
    const detected = dbTools.find((d) => d.id === p.id)?.found_path ?? ''

    setEditingId(existing ? existing.id : null)
    setDraft({
      id: p.id,
      name: p.name,
      engines: [...p.engines],
      launchMode: p.launchMode,
      args: argsAreEmpty(p.args) && p.launchMode === 'args' ? ['{uri}'] : p.args,
      executable: existing?.executable || detected,
      isCustomId: true,
    })
  }

  async function handleBrowseExe() {
    const picked = await open({
      title: 'Select program executable',
      multiple: false,
      directory: false,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    })
    if (picked && !Array.isArray(picked)) {
      setDraft((curr) => ({ ...curr, executable: picked }))
    }
  }


  async function handleSave() {
    if (!draft.id.trim() || !draft.name.trim()) return

    await run('save', async () => {
      const tool: ExternalTool = {
        id: draft.id.trim().toLowerCase(),
        name: draft.name.trim(),
        engines: draft.engines,
        launchMode: draft.launchMode,
        executable: draft.executable.trim(),
        args:
          draft.launchMode === 'clipboard' ? [] : cleanArgs(draft.args, argsSections(draft.engines)),
      }

      const res = await runCommand({ type: 'save_external_tool', tool })
      if (res.type === 'external_tools') {
        setTools(res.tools)
        onToolsUpdated(res.tools)
        notify.success(`Tool "${tool.name}" saved.`)
        resetDraft()
      }
    })
  }

  async function fixPath(tool: ExternalTool) {
    const picked = await open({
      title: `Locate executable for ${tool.name}`,
      multiple: false,
      directory: false,
      filters: [{ name: 'Executable', extensions: ['exe'] }],
    })
    if (!picked || Array.isArray(picked)) return

    await run(`fix:${tool.id}`, async () => {
      const updated: ExternalTool = {
        ...tool,
        executable: picked,
      }
      const res = await runCommand({ type: 'save_external_tool', tool: updated })
      if (res.type === 'external_tools') {
        setTools(res.tools)
        onToolsUpdated(res.tools)
        notify.success(`Path updated for "${tool.name}".`)
        if (editingId === tool.id) {
          setDraft((curr) => ({ ...curr, executable: picked }))
        }
      }
    })
  }

  async function handleExport() {
    const dest = await save({
      title: 'Export external tools',
      defaultPath: 'external_tools.json',
      filters: [{ name: 'JSON', extensions: ['json'] }],
    })
    if (!dest) return

    await run('export', async () => {
      const json = exportToolsJson(tools)
      try {
        await writeTextFile(dest, json)
      } catch {
        // Web fallback if running without full Tauri FS
        const blob = new Blob([json], { type: 'application/json' })
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = 'external_tools.json'
        a.click()
        URL.revokeObjectURL(url)
      }
      notify.success(`Exported ${tools.length} tool(s) to ${dest}.`)
    })
  }

  async function processImportContent(content: string) {
    setImportError(null)
    const result = validateImportJson(content)
    if (!result.ok) {
      setImportError({
        problem: result.problem,
        cause: result.cause,
        fix: result.fix,
      })
      setImportPending(null)
      return
    }

    setImportPending(result.tools)
    const actions: Record<string, 'skip' | 'overwrite' | 'rename'> = {}
    for (const t of result.tools) {
      if (tools.some((existing) => existing.id === t.id)) {
        actions[t.id] = 'skip'
      }
    }
    setDuplicateActions(actions)
  }

  async function handlePickImportFile() {
    const picked = await open({
      title: 'Import external tools',
      multiple: false,
      directory: false,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    })
    if (!picked || Array.isArray(picked)) return

    try {
      const content = await readTextFile(picked)
      await processImportContent(content)
    } catch (err) {
      setImportError({
        problem: 'Could not read import file.',
        cause: err instanceof Error ? err.message : String(err),
        fix: 'Check that the file exists and is accessible.',
      })
    }
  }

  async function handleConfirmImport() {
    if (!importPending) return

    await run('import', async () => {
      let addedCount = 0
      let overwrittenCount = 0
      let skippedCount = 0
      let latestList = tools

      for (const t of importPending) {
        const isDuplicate = latestList.some((existing) => existing.id === t.id)
        let finalId = t.id

        if (isDuplicate) {
          const action = duplicateActions[t.id] ?? 'skip'
          if (action === 'skip') {
            skippedCount++
            continue
          }
          if (action === 'rename') {
            let counter = 1
            while (latestList.some((existing) => existing.id === finalId)) {
              finalId = `${t.id}-${counter++}`
            }
          }
        }

        const newTool: ExternalTool = {
          id: finalId,
          name: t.name,
          engines: t.engines,
          launchMode: t.launchMode,
          args: t.args,
          executable: '', // Starts unavailable until user browses executable
        }

        const res = await runCommand({ type: 'save_external_tool', tool: newTool })
        if (res.type === 'external_tools') {
          latestList = res.tools
          if (isDuplicate && duplicateActions[t.id] === 'overwrite') {
            overwrittenCount++
          } else {
            addedCount++
          }
        }
      }

      setTools(latestList)
      onToolsUpdated(latestList)
      setImportOpen(false)
      setImportPending(null)
      notify.success(`Import finished: ${addedCount} added, ${overwrittenCount} overwritten, ${skippedCount} skipped.`)
    })
  }

  const presets = (presetsCatalog as { tools: ToolDefinition[] }).tools ?? []
  const filteredPresets = presets.filter(
    (p) =>
      p.name.toLowerCase().includes(presetSearch.toLowerCase()) ||
      p.id.toLowerCase().includes(presetSearch.toLowerCase()) ||
      p.engines.some((e) => e.toLowerCase().includes(presetSearch.toLowerCase())),
  )

  return (
    <div className="flex flex-col gap-6">
      {/* Page Header */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold tracking-tight">External Tools</h2>
          <p className="text-sm text-muted-foreground">
            Connect GUI database clients (DBeaver, HeidiSQL, Letos, etc.). Each tool has one user-chosen executable path.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={busy !== null}
            onClick={() => {
              setImportOpen(true)
              setImportError(null)
              setImportPending(null)
            }}
          >
            <Import className="size-4" /> Import JSON
          </Button>
          <Button size="sm" variant="outline" disabled={busy !== null || tools.length === 0} onClick={handleExport}>
            <Download className="size-4" /> Export JSON
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      {/* Main 2-column Grid: Left = Presets Gallery, Right = Register/Edit Form */}
      <div className="grid items-start gap-6 lg:grid-cols-12">
        {/* Left Column: Quick Add Presets Gallery (5 cols) */}
        <Card className="lg:col-span-5">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between gap-2">
              <CardTitle className="text-sm font-semibold flex items-center gap-2">
                <Sparkles className="size-4 text-primary" /> Quick Add Presets
              </CardTitle>
              <Badge variant="outline" className="text-xs">
                {presets.length} available
              </Badge>
            </div>
            <CardDescription>Click a preset to pre-fill the form with verified arguments.</CardDescription>
            <div className="pt-2">
              <Input
                value={presetSearch}
                onChange={(e) => setPresetSearch(e.target.value)}
                placeholder="Search presets by name or engine…"
                className="h-8 text-xs"
              />
            </div>
          </CardHeader>
          <CardContent className="flex flex-col gap-2 max-h-[580px] overflow-y-auto pr-1">
            {filteredPresets.map((p) => {
              const isAdded = tools.some((t) => t.id === p.id)
              const detectedPath = dbTools.find((d) => d.id === p.id)?.found_path
              const isSelected = draft.id === p.id

              return (
                <div
                  key={p.id}
                  role="button"
                  tabIndex={0}
                  onClick={() => applyPreset(p)}
                  onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && applyPreset(p)}
                  className={`flex flex-col gap-1.5 rounded-lg border p-2.5 text-xs transition cursor-pointer select-none hover:border-primary/50 ${
                    isSelected ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border bg-card'
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="flex items-center gap-2 font-medium text-sm">
                      <TechIcon id={p.id} className="size-4 shrink-0" />
                      {p.name}
                    </span>
                    <div className="flex items-center gap-1.5">
                      <Badge variant="outline" className="text-[10px] h-4.5 px-1.5 capitalize">
                        {p.launchMode}
                      </Badge>
                      {isAdded ? (
                        <Badge variant="secondary" className="text-[10px] h-4.5 px-1.5 text-emerald-600 dark:text-emerald-400">
                          Added
                        </Badge>
                      ) : (
                        <Badge variant="outline" className="text-[10px] h-4.5 px-1.5 text-muted-foreground">
                          Not added
                        </Badge>
                      )}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-1">
                    {p.engines.map((eng) => (
                      <span key={eng} className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                        {eng}
                      </span>
                    ))}
                  </div>
                  {detectedPath && (
                    <div className="flex items-center gap-1 text-[11px] text-emerald-600 dark:text-emerald-400 font-medium">
                      <span>● Detected on system</span>
                      <span className="truncate font-mono text-[10px] text-muted-foreground">({detectedPath})</span>
                    </div>
                  )}
                </div>
              )
            })}
            {filteredPresets.length === 0 && (
              <p className="p-4 text-center text-xs text-muted-foreground">No presets matched your search.</p>
            )}
          </CardContent>
        </Card>

        {/* Right Column: Register / Edit Form (7 cols) */}
        <Card className="lg:col-span-7">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between gap-2">
              <CardTitle className="text-sm font-semibold flex items-center gap-2">
                <Wrench className="size-4 text-primary" />
                {editingId ? `Edit ${draft.name || 'tool'}` : 'Register a database explorer'}
              </CardTitle>
              {editingId && (
                <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={resetDraft}>
                  New tool
                </Button>
              )}
            </div>
            <CardDescription>
              {editingId
                ? 'Update settings or change the executable path.'
                : 'Register an explorer of your own or customize a preset.'}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Name" hint="Display name for menus and buttons">
                <Input
                  value={draft.name}
                  onChange={(e) => handleNameChange(e.target.value)}
                  placeholder="e.g. DBeaver"
                />
              </Field>
              <Field label="ID" hint="Unique slug identifier (lowercase)">
                <Input
                  value={draft.id}
                  onChange={(e) => handleIdChange(e.target.value)}
                  placeholder="e.g. dbeaver"
                />
              </Field>
            </div>

            {/* Launch Mode Selection */}
            <Field label="Launch Mode" hint="How connection parameters are passed to the program">
              <div className="grid grid-cols-2 gap-2 pt-1">
                <label
                  className={`flex cursor-pointer flex-col rounded-lg border p-2.5 transition text-xs ${
                    draft.launchMode === 'args'
                      ? 'border-primary bg-primary/5 ring-1 ring-primary'
                      : 'border-border bg-card hover:bg-muted/30'
                  }`}
                >
                  <div className="flex items-center gap-2 font-medium">
                    <input
                      type="radio"
                      name="launchMode"
                      value="args"
                      checked={draft.launchMode === 'args'}
                      onChange={() => setDraft((curr) => ({ ...curr, launchMode: 'args' }))}
                      className="sr-only"
                    />
                    <span>CLI Arguments</span>
                  </div>
                  <span className="mt-1 text-[11px] text-muted-foreground">
                    Pass arguments directly to the program on startup.
                  </span>
                </label>

                <label
                  className={`flex cursor-pointer flex-col rounded-lg border p-2.5 transition text-xs ${
                    draft.launchMode === 'clipboard'
                      ? 'border-primary bg-primary/5 ring-1 ring-primary'
                      : 'border-border bg-card hover:bg-muted/30'
                  }`}
                >
                  <div className="flex items-center gap-2 font-medium">
                    <input
                      type="radio"
                      name="launchMode"
                      value="clipboard"
                      checked={draft.launchMode === 'clipboard'}
                      onChange={() => setDraft((curr) => ({ ...curr, launchMode: 'clipboard' }))}
                      className="sr-only"
                    />
                    <span>Clipboard mode</span>
                  </div>
                  <span className="mt-1 text-[11px] text-muted-foreground">
                    Launch tool and copy URI to clipboard (safest for GUI tools).
                  </span>
                </label>
              </div>
            </Field>

            {/* Engines Checkboxes */}
            <Field
              label="Engines it opens"
              hint="Check the tabs it should appear on. None ticked offers it everywhere."
            >
              <div className="flex flex-wrap gap-x-4 gap-y-2 pt-1">
                {TOOL_ENGINES.map((e) => (
                  <label key={e.id} className="inline-flex items-center gap-2 text-sm cursor-pointer select-none">
                    <Checkbox
                      checked={draft.engines.includes(e.id)}
                      onChange={(on) =>
                        setDraft((curr) => ({
                          ...curr,
                          engines: on ? [...curr.engines, e.id] : curr.engines.filter((x) => x !== e.id),
                        }))
                      }
                      label={`${e.label}`}
                    />
                    <TechIcon id={e.id} className="size-3.5 opacity-80" />
                    <span>{e.label}</span>
                  </label>
                ))}
              </div>
            </Field>

            {/* Arguments: one box per argument, optionally different per engine (if args mode) */}
            {draft.launchMode === 'args' && (
              <div className="flex flex-col gap-2">
                <ToolArgsEditor
                  engines={draft.engines}
                  value={draft.args}
                  onChange={(args) => setDraft((curr) => ({ ...curr, args }))}
                />

                {/* Security Note */}
                <div className="rounded-lg border border-amber-500/20 bg-amber-500/10 p-2 text-[11px] text-amber-700 dark:text-amber-300">
                  <strong>Security Note:</strong> Passwords in <code>{'{uri}'}</code> may be visible to other local processes
                  via the OS process list. Prefer <em>Clipboard mode</em> if your tool supports pasting a connection URI.
                </div>
              </div>
            )}
            {/* Executable Path */}
            <Field label="Program executable (.exe)" hint="Single local path. The file must exist on disk.">
              <div className="flex gap-2">
                <Input
                  value={draft.executable}
                  onChange={(e) => setDraft((curr) => ({ ...curr, executable: e.target.value }))}
                  placeholder="C:\Program Files\..."
                  className="font-mono text-xs"
                />
                <Button variant="secondary" onClick={handleBrowseExe} className="shrink-0">
                  <FolderSearch className="size-4" /> Browse
                </Button>
              </div>
            </Field>

            {/* Form Actions */}
            <div className="flex items-center justify-between pt-2">
              {editingId ? (
                <Button variant="ghost" size="sm" onClick={resetDraft}>
                  Cancel
                </Button>
              ) : (
                <div />
              )}
              <Button
                disabled={busy !== null || !draft.id.trim() || !draft.name.trim()}
                onClick={handleSave}
              >
                {busy === 'save' && <Spinner />}
                {editingId ? 'Update tool' : 'Save tool'}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Bottom Section: Unified Registered Tools List */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between gap-2">
            <div>
              <CardTitle className="text-sm font-semibold">Registered GUI tools ({tools.length})</CardTitle>
              <CardDescription>
                Tools configured for opening databases. Unavailable tools are highlighted with Fix path options.
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {tools.map((t) => {
            const isUnavailable = !t.executable || t.available === false

            return (
              <div
                key={t.id}
                className={`flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3 text-sm transition ${
                  isUnavailable ? 'border-dashed border-destructive/50 bg-destructive/5 opacity-80' : 'border-border bg-card'
                }`}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <TechIcon id={t.id} className="size-4.5 shrink-0" />
                    <span className="font-medium text-foreground">{t.name}</span>
                    <Badge variant="outline" className="font-mono text-[10px]">
                      {t.id}
                    </Badge>
                    <Badge variant="outline" className="text-[10px] capitalize">
                      {t.launchMode ?? 'args'}
                    </Badge>
                    {isUnavailable && (
                      <Badge variant="destructive" className="flex items-center gap-1 text-[10px]">
                        <AlertCircle className="size-3" /> Executable not found
                      </Badge>
                    )}
                  </div>

                  <div className="mt-1 flex flex-wrap items-center gap-2">
                    <span className="font-mono text-xs text-muted-foreground break-all">
                      {t.executable ? t.executable : '(No executable path configured)'}
                    </span>
                  </div>

                  <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                    <span>Engines:</span>
                    {t.engines.length === 0 ? (
                      <span className="rounded bg-muted px-1.5 py-0.2 text-[10px]">every engine</span>
                    ) : (
                      t.engines.map((e) => (
                        <span key={e} className="rounded bg-muted px-1.5 py-0.2 text-[10px]">
                          {e}
                        </span>
                      ))
                    )}
                    {t.launchMode === 'args' && !argsAreEmpty(t.args) && (
                      <span className="font-mono text-[11px] text-muted-foreground truncate max-w-sm">
                        · args: {summarizeArgs(t.args)}
                      </span>
                    )}
                  </div>
                </div>

                {/* Tool Actions */}
                <div className="flex flex-wrap items-center gap-1.5 shrink-0">
                  {isUnavailable ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="border-destructive/40 text-destructive hover:bg-destructive/10"
                      onClick={() => fixPath(t)}
                      title="Locate the executable file on disk"
                    >
                      <FolderSearch className="size-3.5" /> Fix path
                    </Button>
                  ) : (
                    <>
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={busy !== null}
                        onClick={() =>
                          run(`open:${t.id}`, async () => {
                            await runCommand({ type: 'open_db_tool', id: t.id })
                          })
                        }
                        title={`Open ${t.name}`}
                      >
                        <ExternalLink className="size-3.5" /> Open
                      </Button>

                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy !== null}
                        onClick={() =>
                          run(`test:${t.id}`, async () => {
                            if (t.launchMode === 'clipboard') {
                              try {
                                await navigator.clipboard.writeText('mysql://root@127.0.0.1:3306/test_db')
                              } catch {
                                // ignore
                              }
                            }
                            const res = await runCommand({ type: 'test_external_tool', id: t.id })
                            if (res.type === 'text') {
                              notify.success(res.text)
                            }
                          })
                        }
                        title="Test launching with sample connection arguments"
                      >
                        <Play className="size-3.5" /> Test launch
                      </Button>

                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy !== null}
                        onClick={() =>
                          run(`folder:${t.id}`, async () => {
                            const folder = t.executable.replace(/[\\/][^\\/]*$/, '')
                            await runCommand({ type: 'open_path', path: folder })
                          })
                        }
                        title="Reveal program folder in Explorer"
                      >
                        <FolderOpen className="size-3.5" /> Reveal folder
                      </Button>
                    </>
                  )}

                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                      setEditingId(t.id)
                      setDraft({
                        id: t.id,
                        name: t.name,
                        engines: [...t.engines],
                        launchMode: t.launchMode ?? 'args',
                        args: t.args,
                        executable: t.executable,
                        isCustomId: true,
                      })
                      window.scrollTo({ top: 180, behavior: 'smooth' })
                    }}
                    title="Edit tool definition"
                  >
                    <Pencil className="size-3.5" />
                  </Button>

                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-destructive hover:bg-destructive/10"
                    title={`Remove ${t.name}`}
                    onClick={() =>
                      confirmThen(`Remove ${t.name} from external tools? The program is not uninstalled.`, () =>
                        run('rm', async () => {
                          const r = await runCommand({ type: 'remove_external_tool', id: t.id })
                          if (r.type === 'external_tools') {
                            setTools(r.tools)
                            onToolsUpdated(r.tools)
                            if (editingId === t.id) resetDraft()
                          }
                        }),
                      )
                    }
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </div>
            )
          })}

          {tools.length === 0 && (
            <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
              No external tools registered yet. Click any preset in the gallery above or register your own explorer.
            </div>
          )}
        </CardContent>
      </Card>

      {/* Import Modal Dialog */}
      <Dialog
        open={importOpen}
        onClose={() => {
          setImportOpen(false)
          setImportPending(null)
          setImportError(null)
        }}
        title="Import External Tools"
        description="Load external tools from a shared or backed up JSON file."
        size="form"
        footer={
          <div className="flex items-center justify-between w-full">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setImportOpen(false)
                setImportPending(null)
                setImportError(null)
              }}
            >
              Cancel
            </Button>
            {importPending && importPending.length > 0 && (
              <Button size="sm" onClick={handleConfirmImport} disabled={busy !== null}>
                {busy === 'import' && <Spinner />}
                Confirm Import ({importPending.length} tools)
              </Button>
            )}
          </div>
        }
      >
        <div className="flex flex-col gap-4 text-xs">
          <ErrorCard error={importError} onDismiss={() => setImportError(null)} overlay />

          {/* File Dropzone */}
          <div
            onDragOver={(e) => {
              e.preventDefault()
              setImportDragActive(true)
            }}
            onDragLeave={() => setImportDragActive(false)}
            onDrop={async (e) => {
              e.preventDefault()
              setImportDragActive(false)
              if (e.dataTransfer.files.length > 0) {
                const file = e.dataTransfer.files[0]
                const text = await file.text()
                await processImportContent(text)
              }
            }}
            className={`flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-6 text-center transition ${
              importDragActive ? 'border-primary bg-primary/5' : 'border-border bg-muted/20'
            }`}
          >
            <Upload className="size-8 text-muted-foreground mb-2" />
            <p className="font-medium text-foreground">Drag and drop tools JSON file here</p>
            <p className="text-muted-foreground text-[11px] mt-0.5">or choose a file from your computer</p>
            <div className="mt-3">
              <Button size="sm" variant="secondary" onClick={handlePickImportFile}>
                <FileUp className="size-3.5" /> Pick JSON file
              </Button>
            </div>
          </div>

          {/* Import Preview Table */}
          {importPending && (
            <div className="flex flex-col gap-3 pt-2">
              <div className="flex flex-wrap items-center justify-between gap-2 border-b pb-2">
                <span className="font-semibold text-sm">
                  Preview: {importPending.length} tool(s) found in file
                </span>
                {/* Bulk Resolution Controls */}
                <div className="flex items-center gap-1.5 text-[11px]">
                  <span className="text-muted-foreground">For duplicates:</span>
                  <button
                    type="button"
                    onClick={() => {
                      const next = { ...duplicateActions }
                      for (const k of Object.keys(next)) next[k] = 'skip'
                      setDuplicateActions(next)
                    }}
                    className="rounded bg-muted px-2 py-0.5 hover:bg-muted/80"
                  >
                    Skip all
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      const next = { ...duplicateActions }
                      for (const k of Object.keys(next)) next[k] = 'overwrite'
                      setDuplicateActions(next)
                    }}
                    className="rounded bg-muted px-2 py-0.5 hover:bg-muted/80"
                  >
                    Overwrite all
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      const next = { ...duplicateActions }
                      for (const k of Object.keys(next)) next[k] = 'rename'
                      setDuplicateActions(next)
                    }}
                    className="rounded bg-muted px-2 py-0.5 hover:bg-muted/80"
                  >
                    Rename all
                  </button>
                </div>
              </div>

              <div className="flex flex-col gap-2 max-h-60 overflow-y-auto pr-1">
                {importPending.map((t) => {
                  const isDuplicate = tools.some((existing) => existing.id === t.id)
                  const action = duplicateActions[t.id] ?? 'skip'

                  return (
                    <div
                      key={t.id}
                      className="flex items-center justify-between gap-3 rounded-lg border border-border p-2.5 bg-card"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <TechIcon id={t.id} className="size-4 shrink-0" />
                          <span className="font-medium">{t.name}</span>
                          <Badge variant="outline" className="font-mono text-[10px]">
                            {t.id}
                          </Badge>
                          <Badge variant="outline" className="text-[10px]">
                            {t.launchMode}
                          </Badge>
                          {isDuplicate && (
                            <Badge variant="secondary" className="text-[10px] text-amber-600 dark:text-amber-400">
                              Duplicate ID
                            </Badge>
                          )}
                        </div>
                        <div className="mt-1 flex flex-wrap gap-1 text-[11px] text-muted-foreground">
                          <span>Engines: {t.engines.length ? t.engines.join(', ') : 'every engine'}</span>
                          {!argsAreEmpty(t.args) && <span className="font-mono">· args: {summarizeArgs(t.args)}</span>}
                        </div>
                      </div>

                      {/* Duplicate Action Selector */}
                      {isDuplicate && (
                        <div className="flex items-center gap-1 shrink-0">
                          <select
                            value={action}
                            onChange={(e) =>
                              setDuplicateActions((curr) => ({
                                ...curr,
                                [t.id]: e.target.value as 'skip' | 'overwrite' | 'rename',
                              }))
                            }
                            className="h-7 rounded border border-border bg-background px-2 text-xs"
                          >
                            <option value="skip">Skip</option>
                            <option value="overwrite">Overwrite</option>
                            <option value="rename">Rename ({t.id}-1)</option>
                          </select>
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>

              <p className="text-[11px] text-muted-foreground italic">
                * Note: Imported tools contain no local paths. They will be registered in the "Unavailable" state until you
                click "Fix path" to locate their executable.
              </p>
            </div>
          )}
        </div>
      </Dialog>
    </div>
  )
}
