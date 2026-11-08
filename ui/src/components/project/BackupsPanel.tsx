import { save, open } from '@tauri-apps/plugin-dialog'
import { Camera, CloudDownload, Copy, Download, History, Pencil, Plug, Plus, Timer, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { SaveButton } from '@/components/SaveButton'
import { Spinner } from '@/components/Spinner'
import { CronFields } from '@/components/ui/cron-fields'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, SwitchRow, Toggle } from '@/components/ui/form'
import { Input, NumberInput } from '@/components/ui/input'
import { type AutoBackupSiteSettings, type AutoBackupStatus, type CloneResult, type Diagnostic, type GuardSnapshot, type Project, type RemoteDb, type RestoreOptions, type RestoreResult, type SnapshotInfo, type SnapshotOptions, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { asDiagnostic } from '@/lib/diagnostic'
import { formatBytes, timeAgo, useAction } from '@/lib/hooks'

/** §130–132, §158: backups of a project — the automatic plan, snapshots, restoring,
 * exporting, and cloning the environment. */
export function BackupsPanel({ project }: { project: Project }) {
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([])
  const [label, setLabel] = useState('')
  const [opts, setOpts] = useState<SnapshotOptions>({ env: true, databases: false, files: false })
  const [restoring, setRestoring] = useState<SnapshotInfo | null>(null)
  const [cloning, setCloning] = useState(false)
  const action = useAction()
  const { error, setError, run } = action

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_snapshots', project_id: project.id })
    if (r.type === 'snapshots') setSnapshots(r.snapshots)
  }, [project.id])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  return (
    <div className="flex flex-col gap-5">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      <AutoBackupSection project={project} onChanged={load} />

      <GuardSection project={project} />

      <ServerPullSection project={project} />

      <section className="flex flex-col gap-3 rounded-lg border border-border p-3">
        <h3 className="flex items-center gap-2 text-sm font-medium">
          <Camera className="size-4" /> Take a snapshot
        </h3>
        <p className="text-xs text-muted-foreground">
          Always saved: the manifest, runtime versions, sites and their web configs, certificate and database details, workers, scheduled tasks, tunnels, Quick Commands and the mode.
        </p>
        <div className="flex flex-wrap gap-x-6 gap-y-2">
          <Toggle checked={opts.env} onChange={(v) => setOpts({ ...opts, env: v })} label=".env files" hint="Can hold passwords and keys." />
          <Toggle checked={opts.databases} onChange={(v) => setOpts({ ...opts, databases: v })} label="Database data" hint="A SQL dump of the project's database." />
          <Toggle checked={opts.files} onChange={(v) => setOpts({ ...opts, files: v })} label="Project files" hint="Without node_modules, vendor and the like." />
        </div>
        <div className="flex flex-wrap gap-2">
          <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="Before the upgrade" className="h-8 min-w-0 flex-1 sm:max-w-72" />
          <SaveButton
            action={action}
            name="snap"
            size="sm"
            busyLabel="Taking…"
            savedLabel="Taken"
            onClick={() =>
              void run('snap', async () => {
                await runCommand({ type: 'create_snapshot', project_id: project.id, label: label.trim() || 'Snapshot', options: opts })
                setLabel('')
                await load()
              })
            }
          >
            <Camera /> Take snapshot
          </SaveButton>
          <Button size="sm" variant="secondary" onClick={() => setCloning(true)}>
            <Copy /> Clone environment…
          </Button>
        </div>
      </section>

      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Snapshots · {snapshots.length}</h3>
        {snapshots.length === 0 && <p className="text-sm text-muted-foreground">None yet.</p>}
        {snapshots.map((s) => (
          <div key={s.id} className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-border px-3 py-2">
            <div className="min-w-0">
              <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                {s.label || 'Snapshot'}
                {s.label === 'auto' && <Badge variant="secondary">automatic</Badge>}
                <span className="text-xs font-normal text-muted-foreground">
                  {timeAgo(s.created_ms)} · {formatBytes(s.size_bytes)}
                </span>
                {s.options.env && <Badge variant="secondary">.env</Badge>}
                {s.options.databases && <Badge variant="secondary">data</Badge>}
                {s.options.files && <Badge variant="secondary">files</Badge>}
              </p>
              <p className="text-xs text-muted-foreground">{s.summary.join(' · ')}</p>
            </div>
            <div className="flex gap-1">
              <Button size="sm" variant="secondary" onClick={() => setRestoring(s)}>
                <History /> Restore
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="size-8"
                title="Export as an environment file"
                onClick={async () => {
                  const dest = await save({ title: 'Export environment', defaultPath: `${project.name}-environment.zip`, filters: [{ name: 'Environment', extensions: ['zip'] }] })
                  if (dest) await run(`exp:${s.id}`, () => runCommand({ type: 'export_snapshot', project_id: project.id, id: s.id, dest }))
                }}
              >
                <Download />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="size-8"
                title="Delete"
                onClick={async () => {
                  if (await confirmAction(`Delete the snapshot "${s.label}"?`))
                    await run(`del:${s.id}`, async () => {
                      await runCommand({ type: 'delete_snapshot', project_id: project.id, id: s.id })
                      await load()
                    })
                }}
              >
                <Trash2 />
              </Button>
            </div>
          </div>
        ))}
      </section>

      {restoring && <RestoreDialog project={project} snapshot={restoring} onClose={() => setRestoring(null)} onDone={load} />}
      {cloning && <CloneDialog project={project} onClose={() => setCloning(false)} />}
    </div>
  )
}

/** Periods offered per site, in the words the scheduler already uses. */
const PERIODS = [
  { id: 'hourly', label: 'Every hour' },
  { id: 'daily', label: 'Every day (00:00)' },
  { id: 'weekly', label: 'Every Sunday (00:00)' },
  { id: 'monthly', label: 'On the 1st of the month (00:00)' },
]

/**
 * §130: this project's automatic backups, kept next to the snapshots they produce rather
 * than on a settings page where the sites are not in view. Each site carries its own period,
 * keep count and choice of what gets copied — not every site is equally worth an hourly
 * backup, and one site's `.env` files are not every site's business — but only when the
 * app-wide plan is scoped to chosen sites; when the plan covers everything, the plan's
 * answers are what run and this page says so instead of offering an edit that would do
 * nothing.
 */
function AutoBackupSection({ project, onChanged }: { project: Project; onChanged: () => Promise<void> }) {
  const [status, setStatus] = useState<AutoBackupStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)
  /** Hostnames whose custom cron field is open. Local only until an expression is typed. */
  const [custom, setCustom] = useState<Set<string>>(() => new Set())

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_auto_backup' })
    if (r.type === 'auto_backup') setStatus(r.status)
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  const mine = (status?.sites ?? []).filter((site) => site.project_id === project.id)
  const covered = mine.some((site) => site.enabled)
  const plan = status?.settings
  const perSite = plan?.scope === 'site'
  /**
   * The plan is on and covers everything, so this site's switch would save a value the app-wide
   * plan overrides: the switch is shown for its state, not as an offer, and is disabled so it
   * reads as inert rather than as a second answer to a question Settings already answered.
   */
  const rowLocked = !!plan?.enabled && !perSite

  async function saveSite(hostname: string, patch: Partial<AutoBackupSiteSettings>) {
    const current = mine.find((s) => s.hostname === hostname)
    if (!current) return
    setBusy(true)
    setError(null)
    try {
      // The command replaces the site's whole entry, so every field it does not mention is
      // sent as the value it already holds — a patch to the period must not silently reset
      // the contents back to following the plan.
      const pick = <K extends keyof AutoBackupSiteSettings>(key: K) =>
        (key in patch ? (patch[key] ?? null) : current[key]) as AutoBackupSiteSettings[K]
      const r = await runCommand({
        type: 'set_site_auto_backup',
        hostname,
        site: {
          enabled: patch.enabled ?? current.enabled,
          schedule: pick('schedule'),
          keep: pick('keep'),
          include_files: pick('include_files'),
          include_env: pick('include_env'),
          databases: pick('databases'),
        },
      })
      if (r.type === 'auto_backup') setStatus(r.status)
      await onChanged()
    } catch (e) {
      setError(e as Diagnostic)
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border p-3">
      <h3 className="flex items-center gap-2 text-sm font-medium">
        <Timer className="size-4" /> Automatic backups
      </h3>
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <p className="text-xs text-muted-foreground">
        {!plan
          ? 'Reading the plan…'
          : !plan.enabled
            ? 'Automatic backups are switched off for the whole app, so nothing is taken on a schedule. Turn them on in Settings → Backups.'
            : perSite
              ? 'Each site sets its own period, how many backups to keep, and what gets copied, below. Sites of the same project share a snapshot, so give them the same settings.'
              : `The plan covers every project and database ${status?.description}, keeping ${plan.keep} of each, and manages that for every site. Set it to chosen sites in Settings → Backups to give a site its own period.`}
      </p>
      {mine.length === 0 && (
        <p className="text-sm text-muted-foreground">This project has no site yet, so there is nothing to cover.</p>
      )}
      {mine.map((site) => (
        <div key={site.hostname} className="flex flex-col gap-2 rounded-md border border-border/60 p-3">
          <Toggle
            checked={site.enabled}
            onChange={(enabled) => void saveSite(site.hostname, { enabled })}
            disabled={busy || rowLocked}
            label={site.hostname}
            hint={
              rowLocked
                ? 'Managed by Settings — set the scope to chosen sites to switch this site on or off here.'
                : perSite
                  ? 'A snapshot records the project, so one site switched on covers this project.'
                  : undefined
            }
          />
          {site.enabled && (
            <>
            <div className="flex flex-wrap items-end gap-4 pl-1">
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-muted-foreground" htmlFor={`period-${site.hostname}`}>
                  How often
                </label>
                {perSite ? (
                  <>
                    <Select
                      id={`period-${site.hostname}`}
                      className="w-64"
                      value={
                        custom.has(site.hostname)
                          ? 'custom'
                          : site.schedule && !PERIODS.some((p) => p.id === site.schedule)
                            ? 'custom'
                            : site.schedule ?? ''
                      }
                      disabled={busy}
                      onChange={(e) => {
                        const value = e.target.value
                        if (value === 'custom') {
                          // Opening the custom field is not itself a change: nothing is
                          // stored until an expression is typed, so a half-typed cron can
                          // never become a plan that cannot run.
                          setCustom((prev) => new Set(prev).add(site.hostname))
                          return
                        }
                        setCustom((prev) => {
                          const next = new Set(prev)
                          next.delete(site.hostname)
                          return next
                        })
                        void saveSite(site.hostname, { schedule: value || null })
                      }}
                    >
                      <option value="">Use the app default</option>
                      {PERIODS.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.label}
                        </option>
                      ))}
                      <option value="custom">Custom (cron)…</option>
                    </Select>
                    {custom.has(site.hostname) && (
                      <CronFields
                        value={site.schedule ?? site.effective_schedule}
                        disabled={busy}
                        onCommit={(expression) => void saveSite(site.hostname, { schedule: expression })}
                      />
                    )}
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground">Managed by Settings — {describeLocal(site.effective_schedule)}</p>
                )}
              </div>
              <div className="flex flex-col gap-1.5">
                <label className="text-xs font-medium text-muted-foreground" htmlFor={`keep-${site.hostname}`}>
                  How many to keep
                </label>
                {perSite ? (
                  <NumberInput
                    id={`keep-${site.hostname}`}
                    className="w-24"
                    label="Backups to keep"
                    min={1}
                    max={200}
                    value={site.keep}
                    placeholder={String(site.effective_keep)}
                    disabled={busy}
                    onChange={(keep) => void saveSite(site.hostname, { keep })}
                  />
                ) : (
                  <p className="text-sm text-muted-foreground">Managed by Settings — {site.effective_keep}</p>
                )}
              </div>
            </div>
            <div className="flex flex-col gap-1.5 pl-1">
              <span className="text-xs font-medium text-muted-foreground">What to copy</span>
              {perSite ? (
                <div className="flex flex-col gap-1.5">
                  <Toggle
                    checked={site.effective_include_files}
                    onChange={(include_files) => void saveSite(site.hostname, { include_files })}
                    disabled={busy}
                    label="Project files"
                    hint={site.include_files === null ? 'Following Settings right now.' : undefined}
                  />
                  <Toggle
                    checked={site.effective_include_env}
                    onChange={(include_env) => void saveSite(site.hostname, { include_env })}
                    disabled={busy}
                    label=".env files (hold passwords and keys)"
                    hint={site.include_env === null ? 'Following Settings right now.' : undefined}
                  />
                  <Toggle
                    checked={site.effective_databases}
                    onChange={(databases) => void saveSite(site.hostname, { databases })}
                    disabled={busy}
                    label="Databases"
                    hint={
                      site.databases === null
                        ? 'Following Settings right now. MariaDB and PostgreSQL are dumped; SQLite files are copied.'
                        : 'MariaDB and PostgreSQL are dumped; SQLite files are copied.'
                    }
                  />
                  <p className="text-xs text-muted-foreground">
                    Database data is written as its own dump beside the snapshot, not packed into the zip. Leave all three
                    off and the site takes configuration only.
                  </p>
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  Managed by Settings —{' '}
                  {describeContents(site.effective_include_files, site.effective_include_env, site.effective_databases)}
                </p>
              )}
            </div>
            </>
          )}
        </div>
      ))}
      {covered && perSite && (
        <p className="text-xs text-muted-foreground">
          Covered. The next pass happens while OLS is open; old automatic backups are deleted past the keep count, and
          anything you took by hand is never touched.
        </p>
      )}
    </section>
  )
}

/** The schedule words the core uses, for the read-only case where only the plan knows. */
function describeLocal(schedule: string): string {
  return cronWords[schedule] ?? schedule
}

/** What a set of copy answers actually copies, for the read-only case. */
function describeContents(files: boolean, env: boolean, databases: boolean): string {
  if (files && env && databases) return 'project files, .env files and databases'
  if (files && env) return 'project files and .env files'
  if (files && databases) return 'project files and databases'
  if (files) return 'project files only'
  if (env && databases) return '.env files and databases'
  if (env) return '.env files only'
  if (databases) return 'databases only'
  return 'configuration only'
}

const cronWords: Record<string, string> = {
  '* * * * *': 'every minute',
  '0 * * * *': 'every hour',
  '0 0 * * *': 'every day at 00:00',
  '0 0 * * 0': 'every Sunday at 00:00',
  '0 0 1 * *': 'on the 1st of the month at 00:00',
}

function RestoreDialog({ project, snapshot, onClose, onDone }: { project: Project; snapshot: SnapshotInfo; onClose: () => void; onDone: () => Promise<void> }) {
  const [opts, setOpts] = useState<RestoreOptions>({ config: true, env: false, databases: false, files: false })
  const [result, setResult] = useState<RestoreResult | null>(null)
  const { busy, error, setError, run } = useAction()
  return (
    <Dialog
      open
      onClose={() => busy === null && onClose()}
      title={`Restore "${snapshot.label}"`}
      description={`A "Before restore" snapshot of ${project.name} is taken first, so this can be undone.`}
      footer={
        result ? (
          <Button onClick={onClose}>Close</Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose} disabled={busy !== null}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null || !(opts.config || opts.env || opts.databases || opts.files)}
              onClick={() =>
                run('restore', async () => {
                  const r = await runCommand({ type: 'restore_snapshot', project_id: project.id, id: snapshot.id, options: opts })
                  if (r.type === 'restored') setResult(r.result)
                  await onDone()
                })
              }
            >
              {busy ? <Spinner /> : <History />} Restore
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
        {!result ? (
          <>
            <Toggle checked={opts.config} onChange={(v) => setOpts({ ...opts, config: v })} label="Configuration" hint="Manifest, sites, web configs, workers, tasks, tunnels. Sites added since are removed." />
            <Toggle checked={opts.env} onChange={(v) => setOpts({ ...opts, env: v })} disabled={!snapshot.options.env} label=".env files" hint={snapshot.options.env ? 'The current ones are kept as backups.' : 'Not in this snapshot.'} />
            <Toggle checked={opts.databases} onChange={(v) => setOpts({ ...opts, databases: v })} disabled={!snapshot.options.databases} label="Database data" hint={snapshot.options.databases ? 'The current data is backed up first.' : 'Not in this snapshot.'} />
            <Toggle checked={opts.files} onChange={(v) => setOpts({ ...opts, files: v })} disabled={!snapshot.options.files} label="Project files" hint={snapshot.options.files ? 'Files in the snapshot overwrite the current ones; newer files are left alone.' : 'Not in this snapshot.'} />
          </>
        ) : (
          <div className="flex flex-col gap-1 text-sm">
            {result.restored.map((r) => (
              <p key={r} className="text-success">
                ✓ {r}
              </p>
            ))}
            {result.problems.map((p) => (
              <p key={p} className="text-warning">
                ⚠ {p}
              </p>
            ))}
          </div>
        )}
      </div>
    </Dialog>
  )
}

export function CloneDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const parent = project.path.replace(/[\\/][^\\/]+$/, '')
  const [name, setName] = useState(`${project.name}-copy`)
  const [target, setTarget] = useState(`${parent}\\${project.name}-copy`)
  const [what, setWhat] = useState<'full' | 'infrastructure' | 'configuration'>('full')
  const [result, setResult] = useState<CloneResult | null>(null)
  const { busy, error, setError, run } = useAction()
  return (
    <Dialog
      open
      onClose={() => busy === null && onClose()}
      title={`Clone ${project.name}`}
      description="A second copy that can run beside the original: its name, site, database and app port are adjusted."
      footer={
        result ? (
          <Button onClick={onClose}>Close</Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose} disabled={busy !== null}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null || !name.trim() || !target.trim()}
              onClick={() =>
                run('clone', async () => {
                  const r = await runCommand({ type: 'clone_environment', project_id: project.id, target, name: name.trim(), what })
                  if (r.type === 'cloned') setResult(r.result)
                })
              }
            >
              {busy ? <Spinner /> : <Copy />} Clone
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
        {!result ? (
          <>
            <Field label="What to copy">
              <Select value={what} onChange={(e) => setWhat(e.target.value as typeof what)}>
                <option value="full">Everything: files, configuration and database data</option>
                <option value="infrastructure">Infrastructure: sites, services, workers and an empty database</option>
                <option value="configuration">Configuration only: the manifest and .env files</option>
              </Select>
            </Field>
            <Field label="New project name">
              <Input
                value={name}
                onChange={(e) => {
                  setName(e.target.value)
                  setTarget(`${parent}\\${e.target.value}`)
                }}
              />
            </Field>
            <Field label="Folder" hint={what === 'full' ? 'Must be empty or new.' : 'An existing folder, or a new empty one.'}>
              <div className="flex gap-2">
                <Input value={target} onChange={(e) => setTarget(e.target.value)} className="min-w-0 flex-1 font-mono text-xs" />
                <Button
                  variant="secondary"
                  onClick={async () => {
                    const p = await open({ directory: true, title: 'Folder for the copy' })
                    if (p && !Array.isArray(p)) setTarget(p)
                  }}
                >
                  Browse
                </Button>
              </div>
            </Field>
          </>
        ) : (
          <div className="flex flex-col gap-1 text-sm">
            <p className="font-medium">{result.project.name} is ready.</p>
            {result.changes.map((c) => (
              <p key={c} className="text-xs text-muted-foreground">
                ✓ {c}
              </p>
            ))}
            {result.problems.map((p) => (
              <p key={p} className="text-xs text-warning">
                ⚠ {p}
              </p>
            ))}
          </div>
        )}
      </div>
    </Dialog>
  )
}

/**
 * §168: the database copies OLS saved just before a migration. Rewind puts one back (the
 * current state is saved first, so a rewind can be undone too).
 */
function GuardSection({ project }: { project: Project }) {
  const [copies, setCopies] = useState<GuardSnapshot[]>([])
  const [taskId, setTaskId] = useState<string | null>(null)
  const action = useAction()
  const { error, setError, run } = action

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_guard_snapshots', project_id: project.id })
    if (r.type === 'guard_snapshots') setCopies(r.snapshots)
  }, [project.id])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  return (
    <section className="flex flex-col gap-2">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <h3 className="flex items-center gap-2 text-sm font-medium">
        <History className="size-4" /> Before migrations · {copies.length}
      </h3>
      <p className="text-xs text-muted-foreground">
        OLS saves the database before it runs a migration (Laravel, Symfony, Django, Prisma and others). Commands typed into the built-in terminal are not covered.
      </p>
      {copies.length === 0 && <p className="text-sm text-muted-foreground">No copies yet.</p>}
      {copies.map((c) => (
        <div key={c.id} className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-border px-3 py-2">
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
              {c.database}
              <Badge variant="secondary">{c.kind}</Badge>
              <span className="text-xs font-normal text-muted-foreground">
                {timeAgo(c.created * 1000)} · {formatBytes(c.size)}
              </span>
            </p>
            <p className="truncate text-xs text-muted-foreground" title={c.command}>
              {c.command}
            </p>
          </div>
          <div className="flex gap-1">
            <Button
              size="sm"
              variant="secondary"
              onClick={async () => {
                const when = new Date(c.created * 1000).toLocaleString()
                if (!(await confirmAction(`Put back ${c.database} as it was at ${when}? OLS saves the current state first, so this can be undone.`, 'Rewind database?', 'Rewind', 'amber'))) return
                await run(`rewind:${c.id}`, async () => {
                  const r = await runCommand({ type: 'rewind_guard_snapshot', id: c.id })
                  if (r.type === 'task_started') setTaskId(r.id)
                })
              }}
            >
              <History /> Rewind
            </Button>
            <Button
              size="icon"
              variant="ghost"
              className="size-8"
              title="Delete this copy"
              onClick={async () => {
                if (await confirmAction(`Delete the saved copy of ${c.database}?`))
                  await run(`gdel:${c.id}`, async () => {
                    await runCommand({ type: 'delete_guard_snapshot', id: c.id })
                    await load()
                  })
              }}
            >
              <Trash2 />
            </Button>
          </div>
        </div>
      ))}
      <TaskOutputDialog taskId={taskId} onClose={() => setTaskId(null)} onFinished={() => void load()} />
    </section>
  )
}

/** §168: the Settings switch for Migration guard (used on the Settings page). */
export function MigrationGuardSettings() {
  const [enabled, setEnabled] = useState(true)
  const [maxMb, setMaxMb] = useState<number | null>(1024)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    void (async () => {
      const on = await runCommand({ type: 'get_setting', key: 'db.migration_guard' })
      if (on.type === 'setting' && typeof on.value === 'boolean') setEnabled(on.value)
      const mb = await runCommand({ type: 'get_setting', key: 'db.migration_guard.max_mb' })
      if (mb.type === 'setting' && typeof mb.value === 'number') setMaxMb(mb.value)
    })().catch(() => {})
  }, [])

  const save = async (key: string, value: boolean | number) => {
    setError(null)
    try {
      await runCommand({ type: 'set_setting', key, value })
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <SwitchRow
        checked={enabled}
        onChange={(v) => {
          setEnabled(v)
          void save('db.migration_guard', v)
        }}
        title="Save the database before migrations"
        hint="Covers commands run from OLS. Commands typed into the built-in terminal are not covered. If the copy cannot be made, the migration does not start."
      />
      <Field label="Skip the copy when the database is bigger than (MB)" hint="Bigger databases are not copied; the migration still runs.">
        <NumberInput
          label="Size limit in megabytes"
          value={maxMb}
          min={1}
          disabled={!enabled}
          onChange={(v) => {
            setMaxMb(v)
            if (v !== null && v > 0) void save('db.migration_guard.max_mb', v)
          }}
          className="h-8 w-32"
        />
      </Field>
    </div>
  )
}

const EMPTY_REMOTE: RemoteDb = { id: '', name: '', ssh_target: '', ssh_port: null, engine: 'mariadb', database: '', db_user: '', db_host: '127.0.0.1', db_port: null, has_password: false }

const SSH_TARGET = /^[A-Za-z0-9._-]+(@[A-Za-z0-9._-]+)?$/
const USER = /^[A-Za-z0-9_.-]{1,64}$/
const HOST = /^[A-Za-z0-9.-]{1,253}$/
const DB_NAME = /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}$/

/** The same checks the core runs, so the form can say what is wrong before saving. */
function remoteProblem(r: RemoteDb): string | null {
  if (!r.name.trim() || r.name.length > 40) return 'Give the profile a name (up to 40 characters).'
  if (!SSH_TARGET.test(r.ssh_target) || r.ssh_target.length > 255 || r.ssh_target.startsWith('-')) return 'SSH address: use user@host or a name from your SSH config.'
  if (!DB_NAME.test(r.database)) return 'Database name: letters, digits and _ - . only.'
  if (!USER.test(r.db_user)) return 'Database user: letters, digits and _ . - only.'
  if (!HOST.test(r.db_host)) return 'Database host: letters, digits, . and - only.'
  for (const p of [r.ssh_port, r.db_port]) if (p !== null && (p < 1 || p > 65535)) return 'Ports are 1 to 65535.'
  return null
}

/**
 * §177 Pull from a server: saved SSH profiles, a Pull dialog, and the anonymizing rules.
 * The password is sent once to the core and kept in the OS keyring; it is never shown again.
 */
function ServerPullSection({ project }: { project: Project }) {
  const [remotes, setRemotes] = useState<RemoteDb[]>([])
  const [form, setForm] = useState<{ remote: RemoteDb; password: string } | null>(null)
  const [pulling, setPulling] = useState<RemoteDb | null>(null)
  const [taskId, setTaskId] = useState<string | null>(null)
  const action = useAction()
  const { busy, error, setError, run } = action

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_remote_dbs', project_id: project.id })
    if (r.type === 'remote_dbs') setRemotes(r.remotes)
  }, [project.id])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  const problem = form ? remoteProblem(form.remote) : null

  return (
    <section className="flex flex-col gap-2">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-sm font-medium">
          <CloudDownload className="size-4" /> Pull from a server · {remotes.length}
        </h3>
        <Button size="sm" variant="secondary" onClick={() => setForm({ remote: EMPTY_REMOTE, password: '' })}>
          <Plus /> Add
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Copies a staging or production database to this computer over SSH. SSH key login only (OLS cannot answer password prompts); the server needs mysqldump or pg_dump. A server database usually holds real people's data: pull with anonymizing whenever you can.
      </p>
      {remotes.length === 0 && <p className="text-sm text-muted-foreground">No server profiles yet.</p>}
      {remotes.map((r) => (
        <div key={r.id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
              {r.name}
              <Badge variant="secondary">{r.engine}</Badge>
            </p>
            <p className="truncate text-xs text-muted-foreground">
              {r.ssh_target} · {r.database}
            </p>
          </div>
          <div className="flex gap-1">
            <Button size="sm" onClick={() => setPulling(r)}>
              <CloudDownload /> Pull
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy !== null}
              onClick={() =>
                void run(`test:${r.id}`, async () => {
                  const t = await runCommand({ type: 'test_remote_db', project_id: project.id, id: r.id })
                  if (t.type === 'task_started') setTaskId(t.id)
                })
              }
            >
              <Plug /> Test
            </Button>
            <Button size="icon" variant="ghost" className="size-8" title="Edit" onClick={() => setForm({ remote: r, password: '' })}>
              <Pencil />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              className="size-8"
              title="Delete"
              onClick={async () => {
                if (await confirmAction(`Delete the server profile ${r.name}? Its saved password is removed too.`))
                  await run(`rdel:${r.id}`, async () => {
                    await runCommand({ type: 'delete_remote_db', project_id: project.id, id: r.id })
                    await load()
                  })
              }}
            >
              <Trash2 />
            </Button>
          </div>
        </div>
      ))}

      <Dialog
        open={form !== null}
        onClose={() => setForm(null)}
        title={form?.remote.id ? 'Edit server profile' : 'Add server profile'}
        footer={
          <>
            <Button variant="ghost" onClick={() => setForm(null)}>
              Cancel
            </Button>
            <Button
              disabled={!form || problem !== null || busy !== null}
              onClick={() =>
                void run('save', async () => {
                  if (!form) return
                  const pw = form.password === '' ? null : form.password
                  await runCommand({ type: 'save_remote_db', project_id: project.id, remote: form.remote, password: pw })
                  setForm(null)
                  await load()
                })
              }
            >
              Save
            </Button>
          </>
        }
      >
        {form && (
          <div className="flex flex-col gap-3">
            <Field label="Name" hint="Only shown here, for example staging.">
              <Input value={form.remote.name} onChange={(e) => setForm({ ...form, remote: { ...form.remote, name: e.target.value } })} />
            </Field>
            <Field label="SSH address" hint="user@host, or a name from your SSH config. Key login only.">
              <Input value={form.remote.ssh_target} onChange={(e) => setForm({ ...form, remote: { ...form.remote, ssh_target: e.target.value.trim() } })} className="font-mono" />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Database type">
                <Select value={form.remote.engine} onChange={(e) => setForm({ ...form, remote: { ...form.remote, engine: e.target.value } })}>
                  <option value="mariadb">MariaDB / MySQL</option>
                  <option value="postgres">PostgreSQL</option>
                </Select>
              </Field>
              <Field label="Database name">
                <Input value={form.remote.database} onChange={(e) => setForm({ ...form, remote: { ...form.remote, database: e.target.value.trim() } })} className="font-mono" />
              </Field>
              <Field label="Database user">
                <Input value={form.remote.db_user} onChange={(e) => setForm({ ...form, remote: { ...form.remote, db_user: e.target.value.trim() } })} className="font-mono" />
              </Field>
              <Field label="Database password" hint={form.remote.has_password ? 'Saved. Leave empty to keep it.' : 'Saved in the Windows credential store.'}>
                <Input type="password" autoComplete="off" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
              </Field>
              <Field label="Database host" hint="As seen from the server.">
                <Input value={form.remote.db_host} onChange={(e) => setForm({ ...form, remote: { ...form.remote, db_host: e.target.value.trim() } })} className="font-mono" />
              </Field>
              <Field label="Ports (optional)" hint="SSH port, then database port.">
                <div className="flex gap-2">
                  <NumberInput label="SSH port" value={form.remote.ssh_port} min={1} max={65535} onChange={(v) => setForm({ ...form, remote: { ...form.remote, ssh_port: v } })} className="h-8 w-24" />
                  <NumberInput label="Database port" value={form.remote.db_port} min={1} max={65535} onChange={(v) => setForm({ ...form, remote: { ...form.remote, db_port: v } })} className="h-8 w-24" />
                </div>
              </Field>
            </div>
            {form.remote.has_password && form.password === '' && <p className="text-xs text-muted-foreground">To remove the saved password, delete the profile and add it again.</p>}
            {problem && <p className="text-xs text-warning">{problem}</p>}
          </div>
        )}
      </Dialog>

      {pulling && <PullDialog project={project} remote={pulling} onClose={() => setPulling(null)} onStarted={setTaskId} />}
      <TaskOutputDialog taskId={taskId} onClose={() => setTaskId(null)} onFinished={() => void load()} />
    </section>
  )
}

function PullDialog({ project, remote, onClose, onStarted }: { project: Project; remote: RemoteDb; onClose: () => void; onStarted: (id: string) => void }) {
  const [mode, setMode] = useState<'replace' | 'new'>('replace')
  const [anonymize, setAnonymize] = useState(true)
  const [understood, setUnderstood] = useState(false)
  const [rules, setRules] = useState<string | null>(null)
  const [draft, setDraft] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    void runCommand({ type: 'get_anonymize_rules', project_id: project.id })
      .then((r) => {
        if (r.type === 'anonymize_rules') setRules(r.yaml)
      })
      .catch(setError)
  }, [project.id, setError])

  const haveRules = rules !== null
  const useAnonymize = anonymize && haveRules
  const canPull = useAnonymize || understood

  return (
    <Dialog
      open
      onClose={onClose}
      title={`Pull ${remote.name}`}
      description="Copies the server database to this computer."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={!canPull || busy !== null}
            onClick={() =>
              void run('pull', async () => {
                const r = await runCommand({ type: 'pull_remote_db', project_id: project.id, remote_id: remote.id, mode, anonymize: useAnonymize, confirm_real_data: !useAnonymize && understood })
                if (r.type === 'task_started') {
                  onStarted(r.id)
                  onClose()
                }
              })
            }
          >
            {busy ? <Spinner /> : <CloudDownload />} Pull
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="What to do with your local database">
          <Select value={mode} onChange={(e) => setMode(e.target.value as 'replace' | 'new')}>
            <option value="replace">Replace it (OLS saves a backup first)</option>
            <option value="new">Keep it and create a new one</option>
          </Select>
        </Field>
        <SwitchRow
          checked={useAnonymize}
          disabled={!haveRules}
          onChange={setAnonymize}
          title="Anonymize"
          hint={haveRules ? 'Emails, names, phone numbers and passwords are replaced with fake values before you open the data.' : 'There are no rules yet. Create rules first, or pull the real data.'}
        />
        {!haveRules && draft === null && (
          <Button
            size="sm"
            variant="secondary"
            onClick={() =>
              void run('suggest', async () => {
                const r = await runCommand({ type: 'suggest_anonymize_rules', project_id: project.id })
                if (r.type === 'anonymize_rules') setDraft(r.yaml ?? '')
              })
            }
          >
            Create rules
          </Button>
        )}
        {draft !== null && (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">OLS read your local database and suggests these rules. Check them, then save. They are stored in .openlocalserver/anonymize.yaml, which your team can commit.</p>
            <CodeEditor value={draft} onChange={setDraft} language="yaml" height="260px" />
            <Button
              size="sm"
              onClick={() =>
                void run('save-rules', async () => {
                  await runCommand({ type: 'save_anonymize_rules', project_id: project.id, yaml: draft })
                  setRules(draft)
                  setDraft(null)
                })
              }
            >
              Save rules
            </Button>
          </div>
        )}
        {!useAnonymize && (
          <div className="rounded-md border border-warning/40 bg-warning/10 p-3 text-xs">
            <p className="mb-2">Without anonymizing, you copy real personal data onto your computer, and you are responsible for protecting it. OLS deletes the downloaded file as soon as it is imported.</p>
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} />I understand this copies real personal data
            </label>
          </div>
        )}
      </div>
    </Dialog>
  )
}

