import { Activity, Archive, Boxes, Copy, Cpu, Database, ExternalLink, Gauge, History, Info, Leaf, RotateCcw, Settings2, Timer } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { SaveButton } from '@/components/SaveButton'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { CronFields } from '@/components/ui/cron-fields'
import { Field, Select, SettingRow, Toggle } from '@/components/ui/form'
import { NumberInput, Input } from '@/components/ui/input'
import { type AutoBackupSettings, type AutoBackupStatus, type CpuCapStatus, type ResourceLimits, type SettingsBackup, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { formatBytes, timeAgo, useAction, useAppMarkPath } from '@/lib/hooks'
import { cn } from '@/lib/utils'

/** Only the numeric limits render as fields; `cpu_limiter_path` is a string and is not one. */
type LimitKey = Exclude<keyof ResourceLimits, 'cpu_limiter_path'>
interface LimitDef {
  key: LimitKey
  label: string
  hint: string
  unit: string
  icon: typeof Database
  iconClass: string
}

const DB_LIMITS: LimitDef[] = [
  { key: 'mariadb_buffer_pool_mb', label: 'MariaDB buffer pool', hint: 'InnoDB buffer pool size', unit: 'MB', icon: Database, iconClass: 'text-sky-400' },
  { key: 'postgres_shared_buffers_mb', label: 'PostgreSQL shared buffers', hint: 'PostgreSQL shared_buffers', unit: 'MB', icon: Database, iconClass: 'text-blue-400' },
  { key: 'redis_maxmemory_mb', label: 'Redis memory', hint: 'Max memory, oldest keys evicted first', unit: 'MB', icon: Boxes, iconClass: 'text-red-400' },
  { key: 'memcached_max_memory_mb', label: 'Memcached memory', hint: 'Cache size in RAM; it evicts when full', unit: 'MB', icon: Boxes, iconClass: 'text-red-400' },
  { key: 'mongodb_cache_mb', label: 'MongoDB cache', hint: 'WiredTiger cache · minimum 256 MB', unit: 'MB', icon: Leaf, iconClass: 'text-emerald-400' },
]

const APP_LIMITS: LimitDef[] = [
  { key: 'node_max_old_space_mb', label: 'Node memory', hint: 'Heap limit for every project command', unit: 'MB', icon: Boxes, iconClass: 'text-emerald-400' },
  { key: 'max_worker_count', label: 'Copies per worker', hint: 'Upper limit for queue workers', unit: '', icon: Copy, iconClass: 'text-muted-foreground' },
  { key: 'max_processes', label: 'Process limit', hint: 'Maximum concurrent processes the app may run', unit: '', icon: Activity, iconClass: 'text-muted-foreground' },
  { key: 'k6_max_vus', label: 'Load-test users', hint: 'Maximum virtual users (default 200)', unit: '', icon: Activity, iconClass: 'text-muted-foreground' },
]

const EMPTY_LIMITS: ResourceLimits = {
  mariadb_buffer_pool_mb: null,
  postgres_shared_buffers_mb: null,
  redis_maxmemory_mb: null,
  memcached_max_memory_mb: null,
  mongodb_cache_mb: null,
  cpu_percent: null,
  cpu_threads: null,
  cpu_limiter_path: null,
  node_max_old_space_mb: null,
  max_worker_count: null,
  max_processes: null,
  k6_max_vus: null,
}

function LimitField({ def, value, onChange }: { def: LimitDef; value: number | null; onChange: (v: number | null) => void }) {
  const Icon = def.icon
  const input = (
    <NumberInput
      label={def.label}
      min={1}
      value={value}
      placeholder="Use default"
      onChange={(n) => onChange(n !== null && n > 0 ? n : null)}
      className={cn(def.unit && 'rounded-r-none')}
    />
  )
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <Icon className={cn('size-4 shrink-0', def.iconClass)} aria-hidden="true" />
        <span className="truncate text-[13px] font-medium text-foreground">{def.label}</span>
        <span title={def.hint} className="flex size-4 shrink-0 cursor-help items-center justify-center rounded-full border border-muted-foreground/40 text-[10px] leading-none text-muted-foreground">
          ?
        </span>
      </div>
      {def.unit ? (
        <div className="flex">
          {input}
          <span className="flex h-9 shrink-0 items-center rounded-r-lg border border-l-0 border-border/60 bg-muted/40 px-3 text-[13px] text-muted-foreground">
            {def.unit}
          </span>
        </div>
      ) : (
        input
      )}
      <p className="text-xs leading-relaxed text-muted-foreground">{def.hint}</p>
    </div>
  )
}

/**
 * §129 CPU cap. A percentage is a share of *all* cores, which is the one thing people
 * get wrong ("20% should be half a core" — it is a fifth of the whole machine), so the
 * threads mode converts against the real core count and the hint states both numbers.
 * The cap is applied by the external `cpulimit` utility, so a wanted cap with no utility
 * on disk is stated here rather than quietly not applied at the next start.
 */
function CpuLimitField({
  limits,
  cpu,
  onChange,
}: {
  limits: ResourceLimits
  cpu: CpuCapStatus
  onChange: (patch: Partial<ResourceLimits>) => void
}) {
  const mode: 'off' | 'percent' | 'threads' = limits.cpu_percent !== null ? 'percent' : limits.cpu_threads !== null ? 'threads' : 'off'
  const value = mode === 'percent' ? limits.cpu_percent : mode === 'threads' ? limits.cpu_threads : null
  const cores = Math.max(1, cpu.logical_cores)
  const effective = mode === 'percent' ? value : mode === 'threads' && value !== null ? Math.min(100, Math.ceil((value * 100) / cores)) : null
  const setMode = (next: string) => {
    if (next === 'off') onChange({ cpu_percent: null, cpu_threads: null })
    else if (next === 'percent') onChange({ cpu_percent: value ?? 50, cpu_threads: null })
    else onChange({ cpu_percent: null, cpu_threads: value ?? 1 })
  }
  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 sm:grid-cols-2">
        <Field
          label="How to measure the cap"
          hint="Threads is the easier number: 2 means about half of two cores, whatever this machine has."
        >
          <Select value={mode} onChange={(e) => setMode(e.target.value)}>
            <option value="off">No CPU limit</option>
            <option value="percent">Percentage of the whole CPU</option>
            <option value="threads">Number of threads</option>
          </Select>
        </Field>
        {mode !== 'off' ? (
          <Field
            label={mode === 'percent' ? 'Limit' : 'Threads'}
            hint={
              effective === null
                ? undefined
                : mode === 'percent'
                  ? `${effective}% of all ${cores} ${cores === 1 ? 'core' : 'cores'}. A cap above what the program can use on its own has no effect.`
                  : `${effective}% of all ${cores} ${cores === 1 ? 'core' : 'cores'} — about ${value} full ${cores === 1 ? 'core' : 'core'}${value === 1 ? '' : 's'}.`
            }
          >
            <NumberInput
              label={mode === 'percent' ? 'CPU percentage' : 'CPU threads'}
              min={1}
              max={mode === 'percent' ? 100 : cores}
              value={value}
              placeholder="Not set"
              onChange={(n) =>
                onChange(mode === 'percent' ? { cpu_percent: n !== null && n > 0 ? n : null } : { cpu_threads: n !== null && n > 0 ? n : null })
              }
            />
          </Field>
        ) : null}
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">
        Applied to every service and web server the app starts. The cap holds even for processes they spawn.
      </p>
      {mode !== 'off' ? (
        cpu.limiter_path ? (
          <p className="flex items-center gap-2 text-xs text-muted-foreground">
            <Badge variant="secondary" className="shrink-0">
              <Cpu aria-hidden="true" className="size-3" /> cpulimit
            </Badge>
            <span className="min-w-0 truncate" title={cpu.limiter_path}>
              {cpu.limiter_path}
            </span>
          </p>
        ) : (
          <div className="flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-[13px] text-warning">
            <Info className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <span>
              This cap cannot be applied yet: <code className="font-mono text-xs">cpulimit.exe</code> was not found, so a start would be refused. {cpu.hint}
            </span>
          </div>
        )
      ) : null}
      {mode !== 'off' && !cpu.limiter_path ? (
        <Field label="Path to cpulimit.exe" hint="Only needed when it is not beside the app — a portable copy, or a dev build run from target\.">
          <Input
            value={limits.cpu_limiter_path ?? ''}
            placeholder="C:\OpenLocalServer\cpulimit.exe"
            onChange={(e) => onChange({ cpu_limiter_path: e.target.value.trim() || null })}
          />
        </Field>
      ) : null}
    </div>
  )
}

/** §129: optional memory and process limits. Blank means the program's own default. */
export function ResourcesCard() {
  const [limits, setLimits] = useState<ResourceLimits | null>(null)
  const [cpu, setCpu] = useState<CpuCapStatus | null>(null)
  const [saved, setSaved] = useState(false)
  const action = useAction()
  const { busy, error, setError, run } = action
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_resource_limits' })
    if (r.type === 'resources') {
      setLimits(r.limits)
      setCpu(r.cpu)
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])
  if (!limits || !cpu) return null
  const set = (key: LimitKey, v: number | null) => {
    setSaved(false)
    setLimits({ ...limits, [key]: v })
  }
  const patch = (p: Partial<ResourceLimits>) => {
    setSaved(false)
    setLimits({ ...limits, ...p })
  }
  return (
    <Card className="border-border/60 bg-card">
      <CardContent className="flex flex-col gap-6 p-6">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-start gap-3">
            <Gauge className="mt-0.5 size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div>
              <h2 className="text-[15px] font-semibold tracking-tight text-foreground">Resources</h2>
              <p className="mt-1 text-[13px] leading-relaxed text-muted-foreground">Optional limits on memory, CPU and process count. Blank means the program's own default, and every change applies the next time a service starts.</p>
            </div>
          </div>
          <Button
            size="sm"
            variant="outline"
            disabled={busy !== null}
            onClick={() => {
              setSaved(false)
              setLimits({ ...EMPTY_LIMITS })
            }}
          >
            <RotateCcw /> Reset to defaults
          </Button>
        </div>

        <ErrorCard error={error} onDismiss={() => setError(null)} />

        <section className="flex flex-col gap-4 border-t border-border/60 pt-6">
          <div className="flex items-start gap-3">
            <Cpu className="mt-0.5 size-5 shrink-0 text-amber-400" aria-hidden="true" />
            <div>
              <h3 className="text-[14px] font-semibold text-foreground">CPU</h3>
              <p className="mt-0.5 text-[13px] leading-relaxed text-muted-foreground">
                How much CPU a service may use while it runs. Applies to databases, caches, mail and the web servers.
              </p>
            </div>
          </div>
          <CpuLimitField limits={limits} cpu={cpu} onChange={patch} />
        </section>

        <section className="flex flex-col gap-4 border-t border-border/60 pt-6">
          <div className="flex items-start gap-3">
            <Database className="mt-0.5 size-5 shrink-0 text-blue-400" aria-hidden="true" />
            <div>
              <h3 className="text-[14px] font-semibold text-foreground">Database resources</h3>
              <p className="mt-0.5 text-[13px] text-muted-foreground">Limits for database services. Leave empty to use the default values.</p>
            </div>
          </div>
          <div className="grid gap-x-5 gap-y-5 sm:grid-cols-2 xl:grid-cols-3">
            {DB_LIMITS.map((def) => (
              <LimitField key={def.key} def={def} value={limits[def.key]} onChange={(v) => set(def.key, v)} />
            ))}
          </div>
        </section>

        <section className="flex flex-col gap-4 border-t border-border/60 pt-6">
          <div className="flex items-start gap-3">
            <Settings2 className="mt-0.5 size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div>
              <h3 className="text-[14px] font-semibold text-foreground">Application resources</h3>
              <p className="mt-0.5 text-[13px] text-muted-foreground">Limits for application and worker processes. Leave empty to use the default values.</p>
            </div>
          </div>
          <div className="grid gap-x-5 gap-y-5 sm:grid-cols-2">
            {APP_LIMITS.map((def) => (
              <LimitField key={def.key} def={def} value={limits[def.key]} onChange={(v) => set(def.key, v)} />
            ))}
          </div>
        </section>

        <div className="flex flex-col gap-3 border-t border-border/60 pt-4 sm:flex-row sm:items-center sm:justify-between">
          <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
            <Info className="size-4 shrink-0 text-sky-400" aria-hidden="true" />
            Changes are applied the next time the service starts.
          </p>
          <div className="flex items-center gap-2.5">
            <Button
              variant="secondary"
              disabled={busy !== null}
              onClick={() => {
                setSaved(false)
                void load()
              }}
            >
              Cancel
            </Button>
<SaveButton
              action={action}
              name="limits"
              className="bg-emerald-500 text-zinc-950 hover:bg-emerald-400"
              busyLabel="Saving…"
              savedLabel="Saved"
              onClick={() =>
                void run('limits', async () => {
                  const r = await runCommand({ type: 'set_resource_limits', limits })
                  if (r.type === 'resources') {
                    setLimits(r.limits)
                    setCpu(r.cpu)
                  }
                  setSaved(true)
                })
              }
            >
              Save limits
            </SaveButton>
          </div>
        </div>
        {saved && <span className="text-sm text-success">Saved. Restart a service to apply its limit.</span>}
      </CardContent>
    </Card>
  )
}

/** About this app: name, version, license, stack. Version comes from the backend `ping`. */
export function AboutCard() {
  const [version, setVersion] = useState('')
  const mark = useAppMarkPath()
  useEffect(() => {
    runCommand({ type: 'ping' }).then((r) => r.type === 'pong' && setVersion(r.version))
  }, [])
  return (
    <Card className="border-border/60 bg-card">
      <CardContent className="flex flex-col gap-5 p-6">
        <div className="flex items-center gap-4">
          <img src={mark} alt="" className="size-12 rounded-xl shadow-sm shadow-teal-500/30" />
          <div>
            <h2 className="text-lg font-semibold tracking-tight text-foreground">
              OLS <span className="font-normal text-foreground/70">(Open Local Server)</span>
            </h2>
            <div className="mt-1 flex items-center gap-2">
              {version ? <Badge variant="secondary">v{version}</Badge> : null}
              <Badge variant="outline">GPL-3.0-only</Badge>
            </div>
          </div>
        </div>
        <p className="text-[13px] leading-relaxed text-muted-foreground">
          A local development environment manager for Windows — runtimes, sites, databases, and trusted HTTPS, without touching your system by hand.
        </p>
        <div className="flex flex-col gap-1 border-t border-border/60 pt-4 text-[13px]">
          <div className="flex items-center gap-2 text-muted-foreground">
            <Info className="size-4 shrink-0" aria-hidden="true" />
            Licensed under GPL-3.0-only. Full text in the LICENSE file.
          </div>
          <button
            onClick={() => runCommand({ type: 'open_url', url: 'https://github.com/kz370' })}
            className="flex w-fit cursor-pointer items-center gap-2 text-muted-foreground transition-colors hover:text-foreground"
          >
            <svg viewBox="0 0 24 24" fill="currentColor" className="size-4 shrink-0" aria-hidden="true">
              <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.55v-2.15c-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.18 1.76 1.18 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.72-1.54-2.55-.29-5.23-1.28-5.23-5.68 0-1.26.45-2.28 1.18-3.09-.12-.29-.51-1.46.11-3.05 0 0 .96-.31 3.15 1.18a10.9 10.9 0 0 1 5.74 0c2.19-1.49 3.15-1.18 3.15-1.18.62 1.59.23 2.76.11 3.05.74.81 1.18 1.83 1.18 3.09 0 4.41-2.69 5.38-5.25 5.67.41.35.77 1.05.77 2.12v3.15c0 .3.21.67.8.55A11.51 11.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
            </svg>
            github.com/kz370
            <ExternalLink className="size-3.5 shrink-0" aria-hidden="true" />
          </button>
          <p className="text-xs text-muted-foreground">Built with Rust · Tauri 2 · React 19 · Tokio</p>
        </div>
      </CardContent>
    </Card>
  )
}

/** §130: backups of the app's own settings (not runtimes, caches or logs). */
export function SettingsBackupsCard() {
  const [backups, setBackups] = useState<SettingsBackup[]>([])
  const [message, setMessage] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_settings_backups' })
    if (r.type === 'settings_backups') setBackups(r.backups)
  }, [])
  useEffect(() => {
    void load()
  }, [load])
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Archive className="size-4" /> Settings backups
        </CardTitle>
        <CardDescription>Projects, sites, services, Quick Apps and Commands, profiles, workers, schedules, tunnels and certificate details. A project's own snapshots and automatic backups are on its Backups tab.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="flex items-center gap-3">
          <Button
            variant="secondary"
            disabled={busy !== null}
            onClick={() =>
              run('backup', async () => {
                await runCommand({ type: 'backup_settings' })
                setMessage('Backup saved.')
                await load()
              })
            }
          >
            {busy === 'backup' ? <Spinner /> : <Archive />} Back up settings now
          </Button>
          {message && <span className="text-sm text-success">{message}</span>}
        </div>
        {backups.map((b) => (
          <div key={b.id} className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2 text-sm">
            <span>
              {new Date(b.created_ms).toLocaleString()} <span className="text-xs text-muted-foreground">({timeAgo(b.created_ms)}, {formatBytes(b.size_bytes)})</span>
            </span>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy !== null}
              onClick={async () => {
                if (!(await confirmAction('Restore these settings? The current ones are backed up first. Restart OLS to load them.', 'Restore settings'))) return
                await run(`restore:${b.id}`, async () => {
                  await runCommand({ type: 'restore_settings', id: b.id })
                  setMessage('Restored. Restart OLS to load them.')
                  await load()
                })
              }}
            >
              <History /> Restore
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

const PERIODS = [
  { id: 'every_hour', schedule: 'hourly', label: 'Every hour' },
  { id: 'every_day', schedule: 'daily', label: 'Every day (00:00)' },
  { id: 'every_week', schedule: 'weekly', label: 'Every Sunday (00:00)' },
  { id: 'every_month', schedule: 'monthly', label: 'On the 1st of the month (00:00)' },
  { id: 'custom', schedule: '', label: 'Custom (cron)' },
]

/** §130: sites (snapshots) and databases, taken on a schedule, capped at a number to keep. */
export function AutoBackupCard() {
  const [status, setStatus] = useState<AutoBackupStatus | null>(null)
  const [period, setPeriod] = useState('every_day')
  const [saved, setSaved] = useState<string | null>(null)
  const action = useAction()
  const { error, setError, run } = action

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_auto_backup' })
    if (r.type === 'auto_backup') {
      setStatus(r.status)
      setPeriod(PERIODS.find((p) => p.schedule === r.status.settings.schedule)?.id ?? 'custom')
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  if (!status) return null
  const s = status.settings
  const set = (patch: Partial<AutoBackupSettings>) => setStatus({ ...status, settings: { ...s, ...patch } })

  const save = (patch: Partial<AutoBackupSettings>) =>
    run('save', async () => {
      const r = await runCommand({ type: 'set_auto_backup', settings: { ...s, ...patch } })
      if (r.type === 'auto_backup') {
        setStatus(r.status)
        setSaved('Saved.')
        setTimeout(() => setSaved(null), 2500)
      }
    })

  const pickPeriod = (id: string) => {
    setPeriod(id)
    const preset = PERIODS.find((p) => p.id === id)
    if (preset?.schedule) set({ schedule: preset.schedule })
  }

  const chosen = status.sites.filter((site) => site.enabled)
  const loose = chosen.filter((site) => !site.project_id).length
  const perSite = s.scope === 'site'
  const keptTotal = Object.values(status.kept).reduce((sum, n) => sum + n, 0)
  const last = status.last_run

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Timer className="size-4" /> Automatic backups
        </CardTitle>
        <CardDescription>
          Takes project snapshots and database dumps on a timetable, while OLS is open. Automatic backups are deleted oldest-first once the limit is reached; backups you take by hand are never touched.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />

        <Toggle
          checked={s.enabled}
          onChange={(enabled) => save({ enabled })}
          label="Back up automatically"
          hint={
            status.next_run_ms
              ? `Next run ${timeAgo(status.next_run_ms).replace(' ago', ' from now')}`
              : 'Off. Nothing is taken until you turn this on.'
          }
        />

        <div>
          <SettingRow title="What to back up" hint="Sites are snapshotted; databases are dumped. Each can be left out. Set the scope to chosen sites below and each site makes these three choices for itself instead.">
            <div className="flex flex-col gap-1.5">
              <Toggle checked={s.snapshots} onChange={(snapshots) => save({ snapshots })} label="Sites (snapshots)" />
              <Toggle checked={s.databases} onChange={(databases) => save({ databases })} label="All databases" />
            </div>
          </SettingRow>

          {s.snapshots && (
            <>
              <SettingRow title="Snapshot contents" hint="Database data is dumped separately, so it is not also packed into the zip. Under the chosen-sites scope each site picks these for itself.">
                <div className="flex flex-col gap-1.5">
                  <Toggle checked={s.include_files} onChange={(include_files) => save({ include_files })} label="Project files" />
                  <Toggle checked={s.include_env} onChange={(include_env) => save({ include_env })} label=".env files (hold passwords and keys)" />
                </div>
              </SettingRow>

              <SettingRow
                title="Which sites"
                stacked
                hint="Each site decides for itself: open the site's settings dialog and turn on its Backups tab. A site is backed up through the project behind it, so a project with three sites is one backup."
              >
                <div className="flex flex-col gap-1">
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="radio"
                      name="auto-backup-scope"
                      checked={s.scope === 'app'}
                      onChange={() => save({ scope: 'app' })}
                    />
                    Every project and database in OLS
                  </label>
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="radio"
                      name="auto-backup-scope"
                      checked={s.scope === 'site'}
                      onChange={() => save({ scope: 'site' })}
                    />
                    Only the sites that asked for it
                  </label>
                  {s.scope === 'site' && (
                    <div className="mt-1 rounded-md border border-border/60 p-2 text-xs text-muted-foreground">
                      {chosen.length === 0 ? (
                        <p>
                          No site has asked yet. Open a site, switch to its <span className="text-foreground">Backups</span> tab and
                          turn on its automatic backup.
                        </p>
                      ) : (
                        <>
                          <p>
                            {chosen.length} of {status.sites.length} site{status.sites.length === 1 ? '' : 's'} asked:{' '}
                            {chosen.map((site) => site.hostname).join(', ')}
                          </p>
                          <p className="mt-1">
                            Each of them sets its own period and how many to keep on its Backups page.
                          </p>
                          {loose > 0 && (
                            <p className="mt-1 text-destructive">
                              {loose} of them belong to no project yet, so a snapshot has nothing to record. Point them at a
                              project on their settings tab.
                            </p>
                          )}
                        </>
                      )}
                    </div>
                  )}
                </div>
              </SettingRow>
            </>
          )}

          <SettingRow
            title="How often"
            hint={
              perSite
                ? 'Each site sets its own period on its own Backups page, so this number is not used.'
                : period === 'custom'
                  ? 'Five fields, each accepting * for every value.'
                  : undefined
            }
          >
            {perSite ? (
              <p className="max-w-64 text-sm text-muted-foreground">Set per site, on the site's Backups page</p>
            ) : period === 'custom' ? (
              <CronFields value={s.schedule} onCommit={(expression) => save({ schedule: expression })} />
            ) : (
              <Select value={period} onChange={(e) => pickPeriod(e.target.value)} className="w-48" aria-label="Backup period">
                {PERIODS.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.label}
                  </option>
                ))}
              </Select>
            )}
          </SettingRow>

          <SettingRow
            title="How many to keep"
            hint={
              perSite
                ? 'Each site keeps its own count, on its own Backups page.'
                : keptTotal
                  ? `${keptTotal} automatic backup(s) on disk right now.`
                  : 'Oldest automatic backups are deleted once the limit is reached.'
            }
          >
            {perSite ? (
              <p className="max-w-64 text-sm text-muted-foreground">Set per site, on the site's Backups page</p>
            ) : (
              <NumberInput
                value={s.keep}
                min={1}
                max={200}
                onChange={(keep) => set({ keep: keep ?? 1 })}
                onBlur={() => save({})}
                className="w-24"
                label="Backups to keep"
              />
            )}
          </SettingRow>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <SaveButton
            action={action}
            name="now"
            variant="secondary"
            busyLabel="Running…"
            savedLabel="Done"
            onClick={() =>
              void run('now', async () => {
                const r = await runCommand({ type: 'run_auto_backup' })
                if (r.type === 'auto_backup_run') await load()
              })
            }
          >
            <Archive /> Back up now
          </SaveButton>
          {status.running && <span className="text-sm text-muted-foreground">A backup is already running.</span>}
          {saved && <span className="text-sm text-success">{saved}</span>}
        </div>

        {last && (
          <div className="flex flex-col gap-1.5 rounded-lg border border-border px-3 py-2 text-sm">
            <div className="flex items-center justify-between gap-3">
              <span className="font-medium">Last run {timeAgo(last.finished_ms)}</span>
              <Badge variant={last.problems.length ? 'destructive' : 'secondary'}>
                {last.problems.length ? `${last.problems.length} problem(s)` : 'Everything backed up'}
              </Badge>
            </div>
            {last.created.length > 0 && <div className="text-xs text-muted-foreground">Wrote {last.created.join(', ')}.</div>}
            {last.removed.length > 0 && (
              <div className="text-xs text-muted-foreground">
                Deleted {last.removed.length} backup(s) past the limit: {last.removed.join(', ')}
              </div>
            )}
            {last.problems.map((problem) => (
              <p key={problem} className="text-xs text-destructive">
                {problem}
              </p>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
