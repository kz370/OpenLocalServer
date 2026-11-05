import { Copy, FileCode2, Lock, Pencil, Plus, Settings2, Trash2 } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input, NumberInput } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { type ContainerDetails, type Diagnostic, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

type Tab = 'overview' | 'env' | 'storage' | 'settings'

/** One row while editing the environment. A hidden value stays as it is unless replaced. */
type EnvRow = { key: string; value: string; masked: boolean; replaced: boolean }

const POLICIES: { id: string; label: string }[] = [
  { id: 'no', label: "Don't restart" },
  { id: 'unless-stopped', label: 'Unless stopped by you' },
  { id: 'always', label: 'Always' },
  { id: 'on-failure', label: 'Only when it fails' },
]

function copy(text: string, what: string) {
  void navigator.clipboard.writeText(text).then(() => notify.success(what, { title: 'Copied' }))
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-1 gap-0.5 py-1.5 text-sm @lg:grid-cols-[10rem_minmax(0,1fr)] @lg:gap-3">
      <span className="text-xs text-muted-foreground @lg:text-sm">{label}</span>
      <span className="min-w-0 break-all">{children}</span>
    </div>
  )
}

/**
 * §166: a container's settings, loaded only when opened. Shows what `docker inspect`
 * knows (password-like environment values are removed by the core) and changes what
 * Docker can change without recreating the container. Keyed by container id by the caller.
 */
export function ContainerSettingsDialog({
  id,
  onClose,
  onChanged,
  onRunTask,
  onEditCompose,
}: {
  id: string
  onClose: () => void
  onChanged: () => void
  /** Show a background task's live output (the recreate). */
  onRunTask: (taskId: string) => void
  /** Open a compose file in the editor. */
  onEditCompose: (path: string) => void
}) {
  const [d, setD] = useState<ContainerDetails | null>(null)
  const [tab, setTab] = useState<Tab>('overview')
  const [error, setError] = useState<Diagnostic | null>(null)
  const [busy, setBusy] = useState(false)
  // Editable copies.
  const [name, setName] = useState('')
  const [policy, setPolicy] = useState('no')
  const [retries, setRetries] = useState<number | null>(0)
  const [memory, setMemory] = useState<number | null>(0)
  const [cpus, setCpus] = useState<number | null>(0)
  const [envRows, setEnvRows] = useState<EnvRow[] | null>(null)

  function take(details: ContainerDetails) {
    setD(details)
    setName(details.name)
    setPolicy(details.restart_policy)
    setRetries(details.max_retries)
    setMemory(details.memory_mb)
    setCpus(details.cpus)
  }

  useEffect(() => {
    let alive = true
    void runCommand({ type: 'get_docker_container', id })
      .then((r) => alive && r.type === 'docker_container_details' && take(r.details))
      .catch((err) => alive && setError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [id])

  const limitsChanged =
    !!d && (policy !== d.restart_policy || (policy === 'on-failure' && (retries ?? 0) !== d.max_retries) || (memory ?? 0) !== d.memory_mb || (cpus ?? 0) !== d.cpus)
  const nameChanged = !!d && name.trim() !== d.name

  async function save() {
    if (!d) return
    setBusy(true)
    setError(null)
    try {
      if (nameChanged) {
        await runCommand({ type: 'rename_docker_container', id: d.id, name: name.trim() })
      }
      if (limitsChanged) {
        const r = await runCommand({
          type: 'update_docker_container',
          id: d.id,
          update: {
            restart_policy: policy !== d.restart_policy || policy === 'on-failure' ? policy : null,
            max_retries: policy === 'on-failure' ? (retries ?? 0) : null,
            memory_mb: (memory ?? 0) !== d.memory_mb ? (memory ?? 0) : null,
            cpus: (cpus ?? 0) !== d.cpus ? (cpus ?? 0) : null,
          },
        })
        if (r.type === 'docker_container_details') take(r.details)
      } else if (nameChanged) {
        const r = await runCommand({ type: 'get_docker_container', id: d.id })
        if (r.type === 'docker_container_details') take(r.details)
      }
      notify.success('Applied without restarting the container.', { title: 'Container settings saved' })
      onChanged()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const composeFile = d?.labels['com.docker.compose.project.config_files']?.split(',')[0]?.trim() || null

  function startEnvEdit() {
    if (!d) return
    setEnvRows(d.env.map((e) => ({ key: e.key, value: e.masked ? '' : e.value, masked: e.masked, replaced: false })))
  }

  async function saveEnv() {
    if (!d || !envRows) return
    const keys = envRows.map((r) => r.key.trim()).filter(Boolean)
    if (new Set(keys).size !== keys.length) {
      setError({ problem: 'A variable is listed twice.', cause: 'Each name can appear once.', fix: 'Remove or rename the duplicate.' })
      return
    }
    const ok = await confirmAction(
      `OLS recreates "${d.name}" with the new variables: it creates a copy, stops the old container and starts the copy under the same name. Its volumes and folders are kept. If anything fails, the old container is put back and started again. The container is down for a few seconds.`,
      'Save and recreate the container?',
      'Recreate',
      'amber',
    )
    if (!ok) return
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({
        type: 'recreate_docker_container',
        id: d.id,
        env: envRows
          .filter((r) => r.key.trim())
          .map((r) => ({ key: r.key.trim(), value: r.masked && !r.replaced ? null : r.value })),
      })
      if (r.type === 'task_started') {
        onRunTask(r.id)
        onClose()
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    if (!d) return
    const running = d.state === 'running' || d.state === 'paused' || d.state === 'restarting'
    const ok = await confirmAction(
      `${running ? 'Stops and removes' : 'Removes'} "${d.name}". Named volumes (your data) are kept.${d.compose_project ? ` Up on the "${d.compose_project}" stack creates it again.` : ''}`,
      'Remove container?',
      'Remove',
    )
    if (!ok) return
    setBusy(true)
    setError(null)
    try {
      await runCommand({ type: 'remove_docker_container', id: d.id, force: running, volumes: false })
      notify.success(`${d.name} was removed.`, { title: 'Container removed' })
      onChanged()
      onClose()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const tabs: [Tab, string][] = [
    ['overview', 'Overview'],
    ['env', `Environment${d ? ` (${d.env.length})` : ''}`],
    ['storage', 'Storage & network'],
    ['settings', 'Settings'],
  ]

  return (
    <Dialog
      open
      onClose={onClose}
      size="wide"
      icon={<Settings2 className="size-5 text-sky-500" />}
      title={d?.name ?? 'Container'}
      description={d?.image}
      footer={
        <div className="flex w-full flex-wrap items-center justify-between gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer text-destructive" disabled={!d || busy} onClick={() => void remove()}>
            <Trash2 className="size-3.5" /> Remove container
          </Button>
          <span className="flex gap-2">
            <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
              Close
            </Button>
            {tab === 'settings' && (
              <Button size="sm" className="cursor-pointer" disabled={!d || busy || (!limitsChanged && !nameChanged)} onClick={() => void save()}>
                {busy && <Spinner className="size-3.5" />} Save
              </Button>
            )}
          </span>
        </div>
      }
    >
      <div className="@container flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="flex flex-wrap gap-1 border-b" role="tablist">
          {tabs.map(([t, label]) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={cn(
                '-mb-px cursor-pointer border-b-2 px-3 py-1.5 text-sm transition-colors',
                tab === t ? 'border-primary font-medium text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground',
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {!d ? (
          !error && (
            <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
              <Spinner className="size-4" /> Reading the container…
            </p>
          )
        ) : tab === 'overview' ? (
          <div className="divide-y">
            <Field label="ID">
              <span className="font-mono text-xs">{d.id}</span>{' '}
              <Button size="sm" variant="ghost" className="h-6 cursor-pointer px-1.5" onClick={() => copy(d.id, 'Container ID copied.')}>
                <Copy className="size-3" /> Copy
              </Button>
            </Field>
            <Field label="State">
              <span className="flex flex-wrap items-center gap-1.5">
                <Badge variant={d.state === 'running' ? 'success' : 'secondary'}>{d.state}</Badge>
                {d.health && <Badge variant={d.health === 'healthy' ? 'success' : 'warning'}>{d.health}</Badge>}
                {d.restart_count > 0 && <span className="text-xs text-muted-foreground">restarted {d.restart_count}×</span>}
              </span>
            </Field>
            <Field label="Image">{d.image}</Field>
            {d.compose_project && <Field label="Compose project">{d.compose_project}</Field>}
            <Field label="Command">
              <code className="text-xs">{[...d.entrypoint, ...d.command].join(' ') || '—'}</code>
            </Field>
            {d.working_dir && <Field label="Working folder">{d.working_dir}</Field>}
            <Field label="Ports">
              {d.ports.length === 0
                ? '—'
                : d.ports.map((p) => `${p.container_port}/${p.protocol}${p.host_port ? ` → ${p.host_ip ?? ''}:${p.host_port}` : ' (not published)'}`).join(', ')}
            </Field>
            <Field label="Created">{d.created}</Field>
            {d.started_at && <Field label="Started">{d.started_at}</Field>}
          </div>
        ) : tab === 'env' ? (
          <div className="flex flex-col gap-2">
            {d.compose_project ? (
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-xs">
                <span className="text-muted-foreground">
                  This container belongs to the &quot;{d.compose_project}&quot; Compose stack. Change its variables in the compose file and
                  press Up: a change made only to the container would be undone by the next Up.
                </span>
                {composeFile && (
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 shrink-0 cursor-pointer text-xs"
                    onClick={() => {
                      onEditCompose(composeFile)
                      onClose()
                    }}
                  >
                    <FileCode2 className="size-3.5" /> Edit compose file
                  </Button>
                )}
              </div>
            ) : (
              !envRows && (
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Lock className="size-3.5" /> Values of password-like variables are hidden and never leave OLS&apos;s core.
                  </p>
                  <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" onClick={startEnvEdit}>
                    <Pencil className="size-3.5" /> Edit
                  </Button>
                </div>
              )
            )}
            {envRows ? (
              <div className="flex flex-col gap-2">
                <p className="text-xs text-muted-foreground">
                  Hidden values stay as they are unless you type a new one. Saving recreates the container (a few seconds of
                  downtime); its volumes and folders are kept.
                </p>
                {envRows.map((r, i) => (
                  <div key={i} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 @lg:grid-cols-[minmax(0,14rem)_minmax(0,1fr)_auto]">
                    <Input
                      value={r.key}
                      onChange={(e) => setEnvRows(envRows.map((x, j) => (j === i ? { ...x, key: e.target.value } : x)))}
                      placeholder="NAME"
                      className="h-8 font-mono text-xs"
                    />
                    <Input
                      value={r.value}
                      onChange={(e) => setEnvRows(envRows.map((x, j) => (j === i ? { ...x, value: e.target.value, replaced: true } : x)))}
                      placeholder={r.masked ? 'hidden: leave empty to keep it' : 'value'}
                      type={r.masked ? 'password' : 'text'}
                      className="order-3 col-span-2 h-8 font-mono text-xs @lg:order-none @lg:col-span-1"
                    />
                    <Button
                      size="sm"
                      variant="ghost"
                      className="size-8 cursor-pointer justify-self-end p-0"
                      title={`Remove ${r.key || 'variable'}`}
                      onClick={() => setEnvRows(envRows.filter((_, j) => j !== i))}
                    >
                      <Trash2 className="size-4 text-destructive/80" />
                    </Button>
                  </div>
                ))}
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 cursor-pointer text-xs"
                    onClick={() => setEnvRows([...envRows, { key: '', value: '', masked: false, replaced: true }])}
                  >
                    <Plus className="size-3.5" /> Add variable
                  </Button>
                  <span className="flex gap-2">
                    <Button size="sm" variant="ghost" className="cursor-pointer" disabled={busy} onClick={() => setEnvRows(null)}>
                      Cancel
                    </Button>
                    <Button size="sm" className="cursor-pointer" disabled={busy} onClick={() => void saveEnv()}>
                      {busy && <Spinner className="size-3.5" />} Save &amp; recreate
                    </Button>
                  </span>
                </div>
              </div>
            ) : (
              <div className="divide-y rounded-md border">
                {d.env.length === 0 && <p className="p-3 text-sm text-muted-foreground">No environment variables.</p>}
                {d.env.map((e) => (
                  <div key={e.key} className="grid grid-cols-1 gap-1 px-3 py-1.5 text-sm @lg:grid-cols-[minmax(0,14rem)_minmax(0,1fr)] @lg:gap-3">
                    <span className="font-mono text-xs break-all">{e.key}</span>
                    <span className="font-mono text-xs break-all text-muted-foreground">{e.masked ? '••••••••' : e.value || '""'}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : tab === 'storage' ? (
          <div className="flex flex-col gap-4 text-sm">
            <div>
              <p className="mb-1 font-medium">Mounts</p>
              {d.mounts.length === 0 ? (
                <p className="text-xs text-muted-foreground">No volumes or folders mounted.</p>
              ) : (
                <div className="divide-y rounded-md border">
                  {d.mounts.map((m) => (
                    <div key={m.target} className="flex flex-col gap-0.5 px-3 py-2">
                      <span className="font-mono text-xs break-all">{m.target}</span>
                      <span className="text-xs break-all text-muted-foreground">
                        {m.kind === 'volume' ? 'Volume' : m.kind === 'bind' ? 'Folder' : m.kind}: {m.source}
                        {m.read_only && ' · read-only'}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div>
              <p className="mb-1 font-medium">Networks</p>
              <div className="divide-y rounded-md border">
                {d.networks.map((n) => (
                  <div key={n.name} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                    <span className="break-all">{n.name}</span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {n.ip ?? '—'}
                      {n.aliases.length > 0 && ` · ${n.aliases.join(', ')}`}
                    </span>
                  </div>
                ))}
              </div>
            </div>
            {Object.keys(d.labels).length > 0 && (
              <details>
                <summary className="cursor-pointer font-medium">Labels ({Object.keys(d.labels).length})</summary>
                <div className="mt-1 divide-y rounded-md border">
                  {Object.entries(d.labels).map(([k, v]) => (
                    <div key={k} className="grid grid-cols-1 gap-0.5 px-3 py-1.5 @lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] @lg:gap-3">
                      <span className="font-mono text-xs break-all">{k}</span>
                      <span className="font-mono text-xs break-all text-muted-foreground">{v}</span>
                    </div>
                  ))}
                </div>
              </details>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-4 text-sm">
            <p className="text-xs text-muted-foreground">
              These apply right away, without restarting the container. Environment variables are edited on the Environment tab.
              Ports and volumes need the container to be recreated: change them in its compose file and press Up.
            </p>
            <label className="flex flex-col gap-1.5">
              <span className="font-medium">Name</span>
              <Input value={name} onChange={(e) => setName(e.target.value)} />
              {d.compose_project && (
                <span className="text-xs text-muted-foreground">Compose gives it its usual name back the next time Up recreates it.</span>
              )}
            </label>
            <div className="grid grid-cols-1 gap-4 @xl:grid-cols-2">
              <label className="flex flex-col gap-1.5">
                <span className="font-medium">Restart</span>
                <Select value={policy} onChange={(e) => setPolicy(e.target.value)}>
                  {POLICIES.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </Select>
              </label>
              {policy === 'on-failure' && (
                <label className="flex flex-col gap-1.5">
                  <span className="font-medium">Give up after (0 = never)</span>
                  <NumberInput value={retries} onChange={setRetries} min={0} max={1000} label="retries" />
                </label>
              )}
              <label className="flex flex-col gap-1.5">
                <span className="font-medium">Memory limit, MB (0 = none)</span>
                <NumberInput value={memory} onChange={setMemory} min={0} step={128} label="memory" />
              </label>
              <label className="flex flex-col gap-1.5">
                <span className="font-medium">CPU limit</span>
                <Select value={String(cpus ?? 0)} onChange={(e) => setCpus(Number(e.target.value))}>
                  {[...new Set([0, 0.25, 0.5, 1, 1.5, 2, 3, 4, 6, 8, 12, 16, d.cpus])]
                    .sort((a, b) => a - b)
                    .map((c) => (
                      <option key={c} value={String(c)}>
                        {c === 0 ? 'No limit' : `${c} ${c === 1 ? 'core' : 'cores'}`}
                      </option>
                    ))}
                </Select>
              </label>
            </div>
          </div>
        )}
      </div>
    </Dialog>
  )
}
