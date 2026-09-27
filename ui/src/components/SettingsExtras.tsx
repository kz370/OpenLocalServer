import { Activity, Archive, Boxes, Copy, Database, ExternalLink, Gauge, History, Info, Leaf, RotateCcw, Settings2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { type ResourceLimits, type SettingsBackup, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { formatBytes, timeAgo, useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'

type LimitKey = keyof ResourceLimits
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
  mongodb_cache_mb: null,
  node_max_old_space_mb: null,
  max_worker_count: null,
  max_processes: null,
  k6_max_vus: null,
}

function LimitField({ def, value, onChange }: { def: LimitDef; value: number | null; onChange: (v: number | null) => void }) {
  const Icon = def.icon
  const input = (
    <Input
      type="number"
      min={1}
      value={value ?? ''}
      placeholder="Use default"
      onChange={(e) => {
        const n = parseInt(e.target.value, 10)
        onChange(Number.isFinite(n) && n > 0 ? n : null)
      }}
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

/** §129: optional memory and process limits. Blank means the program's own default. */
export function ResourcesCard() {
  const [limits, setLimits] = useState<ResourceLimits | null>(null)
  const [saved, setSaved] = useState(false)
  const { busy, error, setError, run } = useAction()
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_resource_limits' })
    if (r.type === 'resources') setLimits(r.limits)
  }, [])
  useEffect(() => {
    void load()
  }, [load])
  if (!limits) return null
  const set = (key: LimitKey, v: number | null) => {
    setSaved(false)
    setLimits({ ...limits, [key]: v })
  }
  return (
    <Card className="border-border/60 bg-card">
      <CardContent className="flex flex-col gap-6 p-6">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-start gap-3">
            <Gauge className="mt-0.5 size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div>
              <h2 className="text-[15px] font-semibold tracking-tight text-foreground">Resources</h2>
              <p className="mt-1 text-[13px] leading-relaxed text-muted-foreground">Optional resource limits. Changes apply the next time each service starts.</p>
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

        <div className="flex items-center gap-2 rounded-lg border border-sky-500/25 bg-sky-500/10 px-3 py-2 text-[13px] text-sky-200/90">
          <Info className="size-4 shrink-0" aria-hidden="true" />
          CPU limits are not available on Windows.
        </div>

        <ErrorCard error={error} onDismiss={() => setError(null)} />

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
            <Button
              disabled={busy !== null}
              className="bg-emerald-500 text-zinc-950 hover:bg-emerald-400"
              onClick={() =>
                run('limits', async () => {
                  const r = await runCommand({ type: 'set_resource_limits', limits })
                  if (r.type === 'resources') setLimits(r.limits)
                  setSaved(true)
                })
              }
            >
              {busy ? <Spinner /> : null} Save limits
            </Button>
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
  useEffect(() => {
    runCommand({ type: 'ping' }).then((r) => r.type === 'pong' && setVersion(r.version))
  }, [])
  return (
    <Card className="border-border/60 bg-card">
      <CardContent className="flex flex-col gap-5 p-6">
        <div className="flex items-center gap-4">
          <img src="/favicon.svg" alt="" className="size-12 rounded-xl shadow-sm shadow-teal-500/30" />
          <div>
            <h2 className="text-lg font-semibold tracking-tight text-foreground">OpenLocalServer</h2>
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
        <CardDescription>Projects, sites, services, Quick Apps and Commands, profiles, workers, schedules, tunnels and certificate details. Project snapshots are on each project's Snapshots tab.</CardDescription>
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
                if (!(await confirmAction('Restore these settings? The current ones are backed up first. Restart OpenLocalServer afterwards to load them.', 'Restore settings'))) return
                await run(`restore:${b.id}`, async () => {
                  await runCommand({ type: 'restore_settings', id: b.id })
                  setMessage('Restored. Restart OpenLocalServer to load them.')
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
