import { Archive, Gauge, History } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type ResourceLimits, type SettingsBackup, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { formatBytes, timeAgo, useAction } from '@/lib/hooks'

const LIMITS: { key: keyof ResourceLimits; label: string; hint: string; unit: string }[] = [
  { key: 'mariadb_buffer_pool_mb', label: 'MariaDB buffer pool', hint: 'innodb_buffer_pool_size', unit: 'MB' },
  { key: 'postgres_shared_buffers_mb', label: 'PostgreSQL shared buffers', hint: 'shared_buffers', unit: 'MB' },
  { key: 'redis_maxmemory_mb', label: 'Redis memory', hint: 'maxmemory, oldest keys evicted first', unit: 'MB' },
  { key: 'mongodb_cache_mb', label: 'MongoDB cache', hint: 'WiredTiger cache, at least 256', unit: 'MB' },
  { key: 'node_max_old_space_mb', label: 'Node memory', hint: 'heap limit for every project command', unit: 'MB' },
  { key: 'max_worker_count', label: 'Copies per worker', hint: 'upper limit for queue workers', unit: '' },
  { key: 'max_processes', label: 'Process limit', hint: 'processes the app may run at once', unit: '' },
]

/** §129: optional memory and process limits. Blank means the program's own default. */
export function ResourcesCard() {
  const [limits, setLimits] = useState<ResourceLimits | null>(null)
  const [saved, setSaved] = useState(false)
  const { busy, error, setError, run } = useAction()
  useEffect(() => {
    runCommand({ type: 'get_resource_limits' }).then((r) => r.type === 'resources' && setLimits(r.limits))
  }, [])
  if (!limits) return null
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Gauge className="size-4" /> Resources
        </CardTitle>
        <CardDescription>Optional limits, applied the next time each service starts. Windows offers no simple per-program CPU limit, so there is none here.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {LIMITS.map((l) => (
            <Field key={l.key} label={`${l.label}${l.unit ? ` (${l.unit})` : ''}`} hint={l.hint}>
              <Input
                type="number"
                min={1}
                value={limits[l.key] ?? ''}
                placeholder="default"
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10)
                  setSaved(false)
                  setLimits({ ...limits, [l.key]: Number.isFinite(n) && n > 0 ? n : null })
                }}
              />
            </Field>
          ))}
        </div>
        <div className="flex items-center gap-3">
          <Button
            variant="secondary"
            disabled={busy !== null}
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
          {saved && <span className="text-sm text-success">Saved. Restart a service to apply its limit.</span>}
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
