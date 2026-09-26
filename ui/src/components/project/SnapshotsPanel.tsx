import { save, open } from '@tauri-apps/plugin-dialog'
import { Camera, Copy, Download, History, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CloneResult, type Project, type RestoreOptions, type RestoreResult, type SnapshotInfo, type SnapshotOptions, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { formatBytes, timeAgo, useAction } from '@/lib/hooks'

/** §130–132, §158: snapshots of a project, restoring them, exporting, and cloning the environment. */
export function SnapshotsPanel({ project }: { project: Project }) {
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([])
  const [label, setLabel] = useState('')
  const [opts, setOpts] = useState<SnapshotOptions>({ env: true, databases: false, files: false })
  const [restoring, setRestoring] = useState<SnapshotInfo | null>(null)
  const [cloning, setCloning] = useState(false)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_snapshots', project_id: project.id })
    if (r.type === 'snapshots') setSnapshots(r.snapshots)
  }, [project.id])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  return (
    <div className="flex flex-col gap-5">
      <ErrorCard error={error} onDismiss={() => setError(null)} />

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
          <Button
            size="sm"
            disabled={busy !== null}
            onClick={() =>
              run('snap', async () => {
                await runCommand({ type: 'create_snapshot', project_id: project.id, label: label.trim() || 'Snapshot', options: opts })
                setLabel('')
                await load()
              })
            }
          >
            {busy === 'snap' ? <Spinner /> : <Camera />} Take snapshot
          </Button>
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
        <ErrorCard error={error} onDismiss={() => setError(null)} />
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
        <ErrorCard error={error} onDismiss={() => setError(null)} />
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
