import { open as openFolder } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, KeyRound, Plus, Trash2, Wand2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input, NumberInput } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import {
  type BuildPlan,
  type BuildService,
  type BuildSpec,
  type Diagnostic,
  type Project,
  type ServicePreset,
  runCommand,
} from '@/core'
import { confirmAction } from '@/lib/confirm'
import { cn } from '@/lib/utils'

type Picked = BuildService & { key: number }

/**
 * §166: build a compose.yaml by picking services. OLS picks free ports, generates
 * passwords into the Windows credential store, and shows the exact file before writing it.
 */
export function ComposeBuilderDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean
  onClose: () => void
  /** The written file, and whether the user asked to start it right away. */
  onCreated: (file: string, start: boolean) => void
}) {
  const [presets, setPresets] = useState<ServicePreset[]>([])
  const [projects, setProjects] = useState<Project[]>([])
  const [dir, setDir] = useState('')
  const [name, setName] = useState('')
  const [picked, setPicked] = useState<Picked[]>([])
  const [plan, setPlan] = useState<BuildPlan | null>(null)
  const [planError, setPlanError] = useState<Diagnostic | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [busy, setBusy] = useState(false)
  const [nextKey, setNextKey] = useState(1)

  useEffect(() => {
    if (!open) return
    let alive = true
    void Promise.all([runCommand({ type: 'list_docker_compose_presets' }), runCommand({ type: 'list_projects' })])
      .then(([p, pr]) => {
        if (!alive) return
        if (p.type === 'docker_compose_presets') setPresets(p.presets)
        if (pr.type === 'projects') setProjects(pr.projects)
      })
      .catch((err) => alive && setError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [open])

  const spec: BuildSpec | null = useMemo(
    () =>
      dir && picked.length
        ? {
            dir,
            name: name.trim() || null,
            services: picked.map(({ key: _key, ...s }) => s),
          }
        : null,
    [dir, name, picked],
  )

  // Live preview, debounced so typing a name does not plan on every key.
  useEffect(() => {
    if (!spec) return
    let alive = true
    const t = setTimeout(() => {
      void runCommand({ type: 'plan_docker_compose', spec })
        .then((r) => {
          if (alive && r.type === 'docker_compose_plan') {
            setPlan(r.plan)
            setPlanError(null)
          }
        })
        .catch((err) => {
          if (!alive) return
          setPlan(null)
          setPlanError(asDiagnostic(err))
        })
    }, 300)
    return () => {
      alive = false
      clearTimeout(t)
    }
  }, [spec])

  const byCategory = useMemo(() => {
    const map = new Map<string, ServicePreset[]>()
    for (const p of presets) map.set(p.category, [...(map.get(p.category) ?? []), p])
    return [...map.entries()]
  }, [presets])

  function add(preset: ServicePreset | 'custom') {
    const base = preset === 'custom' ? 'app' : preset.default_service
    const taken = new Set(picked.map((p) => p.name))
    let n = base
    for (let i = 2; taken.has(n); i++) n = `${base}${i}`
    setPicked([
      ...picked,
      preset === 'custom'
        ? { key: nextKey, preset: 'custom', name: n, image: '', container_port: 80 }
        : { key: nextKey, preset: preset.id, name: n, tag: preset.tag, host_ports: preset.ports.map(() => null) },
    ])
    setNextKey(nextKey + 1)
  }

  const update = (key: number, patch: Partial<BuildService>) =>
    setPicked(picked.map((p) => (p.key === key ? { ...p, ...patch } : p)))

  async function browse() {
    const selected = await openFolder({ directory: true, multiple: false, title: 'Folder for the compose file' })
    if (typeof selected === 'string') setDir(selected)
  }

  async function create(start: boolean) {
    if (!spec || !plan) return
    if (plan.exists && !(await confirmAction(`${plan.file} already exists. Replace it? The old file is backed up first.`, 'Replace the compose file?', 'Replace', 'amber')))
      return
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'create_docker_compose', spec, overwrite: plan.exists })
      if (r.type === 'docker_compose_plan') {
        onCreated(r.plan.file, start)
        onClose()
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const presetOf = (id: string) => presets.find((p) => p.id === id)
  const shownPlan = spec ? plan : null

  return (
    <Dialog
      open={open}
      onClose={onClose}
      size="wide"
      icon={<Wand2 className="size-5 text-violet-500" />}
      title="New compose file"
      description="Pick services. OLS chooses free ports, keeps passwords in the Windows credential store, and shows the file before writing it."
      footer={
        <div className="flex w-full items-center justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" variant="outline" className="cursor-pointer" disabled={busy || !shownPlan} onClick={() => void create(false)}>
            Create file
          </Button>
          <Button size="sm" className="cursor-pointer" disabled={busy || !shownPlan} onClick={() => void create(true)}>
            {busy && <Spinner className="size-3.5" />} Create and start
          </Button>
        </div>
      }
    >
      <div className="@container flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-3 @xl:grid-cols-[2fr_1fr]">
          <label className="flex min-w-0 flex-col gap-1.5 text-sm">
            <span className="font-medium">Folder</span>
            <span className="flex gap-2">
              <Select value={dir} onChange={(e) => setDir(e.target.value)} placeholder="Choose a project…" className="min-w-0 flex-1">
                {dir && !projects.some((p) => p.path === dir) && <option value={dir}>{dir}</option>}
                {projects.map((p) => (
                  <option key={p.id} value={p.path}>
                    {p.name} — {p.path}
                  </option>
                ))}
              </Select>
              <Button size="sm" variant="outline" className="h-9 shrink-0 cursor-pointer" onClick={() => void browse()}>
                Browse…
              </Button>
            </span>
          </label>
          <label className="flex flex-col gap-1.5 text-sm">
            <span className="font-medium">Stack name (optional)</span>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="the folder name" />
          </label>
        </div>

        <div className="grid grid-cols-1 gap-4 @3xl:grid-cols-2">
          {/* Catalog */}
          <div className="flex flex-col gap-3">
            <span className="text-sm font-medium">Add services</span>
            <div className="flex max-h-72 flex-col gap-3 overflow-y-auto pr-1">
              {byCategory.map(([category, items]) => (
                <div key={category} className="flex flex-col gap-1">
                  <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{category}</span>
                  {items.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => add(p)}
                      className="flex cursor-pointer items-center justify-between gap-3 rounded-md border bg-card/60 px-3 py-2 text-left text-sm hover:border-primary/50 hover:bg-card"
                    >
                      <span className="min-w-0">
                        <span className="block font-medium">{p.name}</span>
                        <span className="block truncate text-xs text-muted-foreground">{p.description}</span>
                      </span>
                      <Plus className="size-4 shrink-0 text-primary" />
                    </button>
                  ))}
                </div>
              ))}
              <button
                type="button"
                onClick={() => add('custom')}
                className="flex cursor-pointer items-center justify-between rounded-md border border-dashed px-3 py-2 text-left text-sm hover:border-primary/50"
              >
                <span>
                  <span className="block font-medium">Any other image</span>
                  <span className="block text-xs text-muted-foreground">Type an image like ghcr.io/org/app:1.0</span>
                </span>
                <Plus className="size-4 shrink-0 text-primary" />
              </button>
            </div>
          </div>

          {/* Picked services */}
          <div className="flex flex-col gap-3">
            <span className="text-sm font-medium">In this file ({picked.length})</span>
            {picked.length === 0 && (
              <p className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">Nothing yet. Add a service on the left.</p>
            )}
            <div className="flex max-h-72 flex-col gap-2 overflow-y-auto pr-1">
              {picked.map((s) => {
                const preset = presetOf(s.preset)
                const planned = shownPlan?.ports.filter((p) => p.service === s.name) ?? []
                return (
                  <div key={s.key} className="flex flex-col gap-2 rounded-md border p-3 text-sm">
                    <div className="flex items-center gap-2">
                      <Badge variant="outline" className="shrink-0">
                        {preset?.name ?? 'Image'}
                      </Badge>
                      <Input
                        value={s.name}
                        onChange={(e) => update(s.key, { name: e.target.value })}
                        className="h-8 min-w-0 flex-1"
                        aria-label="Service name"
                      />
                      <Button
                        size="sm"
                        variant="ghost"
                        className="size-8 shrink-0 cursor-pointer p-0"
                        title={`Remove ${s.name}`}
                        onClick={() => setPicked(picked.filter((p) => p.key !== s.key))}
                      >
                        <Trash2 className="size-4 text-destructive/80" />
                      </Button>
                    </div>
                    {preset ? (
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-xs text-muted-foreground">Version</span>
                        <Select
                          value={s.tag ?? preset.tag}
                          onChange={(e) => update(s.key, { tag: e.target.value })}
                          className="w-40"
                          title={`${preset.image}:${s.tag ?? preset.tag}`}
                        >
                          {preset.tags.map((t) => (
                            <option key={t} value={t}>
                              {t}
                            </option>
                          ))}
                        </Select>
                      </div>
                    ) : (
                      <div className="flex flex-wrap items-center gap-2">
                        <Input
                          value={s.image ?? ''}
                          onChange={(e) => update(s.key, { image: e.target.value })}
                          placeholder="image:tag"
                          className="h-8 min-w-0 flex-1"
                        />
                        <div className="w-36 shrink-0">
                        <NumberInput
                          value={s.container_port ?? null}
                          onChange={(v) => update(s.key, { container_port: v })}
                          min={1}
                          max={65535}
                          label="container port"
                          className="h-8"
                          placeholder="port"
                        />
                        </div>
                      </div>
                    )}
                    {(preset?.ports ?? []).map((pp, i) => {
                      const auto = planned.find((p) => p.container === pp.container)?.host
                      return (
                        <div key={pp.container} className="flex items-center gap-2">
                          <span className="min-w-0 flex-1 text-xs text-muted-foreground" title={`${pp.label}, container port ${pp.container}`}>
                            <span className="block truncate">{pp.label}</span>
                            <span className="block truncate">port {pp.container} → this computer</span>
                          </span>
                          <div className="w-36 shrink-0">
                            <NumberInput
                              value={s.host_ports?.[i] ?? null}
                              onChange={(v) => {
                                const next = [...(s.host_ports ?? preset!.ports.map(() => null))]
                                next[i] = v
                                update(s.key, { host_ports: next })
                              }}
                              min={1}
                              max={65535}
                              label={`${pp.label} port`}
                              className="h-8"
                              placeholder={auto ? `auto: ${auto}` : 'auto'}
                            />
                          </div>
                        </div>
                      )
                    })}
                  </div>
                )
              })}
            </div>
          </div>
        </div>

        {/* Preview */}
        {spec && (
          <div className="flex flex-col gap-2">
            <span className="text-sm font-medium">
              Preview {shownPlan && <span className="font-normal text-muted-foreground">· {shownPlan.file}</span>}
            </span>
            <ErrorCard error={planError} />
            {shownPlan && (
              <>
                {shownPlan.secrets.length > 0 && (
                  <p className="flex items-start gap-2 text-xs text-muted-foreground">
                    <KeyRound className="mt-0.5 size-3.5 shrink-0 text-emerald-500" />
                    {shownPlan.secrets.length} password{shownPlan.secrets.length > 1 ? 's' : ''} will be generated and stored in the
                    Windows credential store, not in the file. See them later with &quot;Passwords&quot; on the stack.
                  </p>
                )}
                {shownPlan.warnings.map((w) => (
                  <p key={w} className="flex items-start gap-2 text-xs text-amber-600 dark:text-amber-400">
                    <AlertTriangle className="mt-0.5 size-3.5 shrink-0" /> {w}
                  </p>
                ))}
                {shownPlan.exists && (
                  <p className={cn('text-xs text-amber-600 dark:text-amber-400')}>This folder already has a compose file; creating replaces it (after a backup).</p>
                )}
                <CodeEditor value={shownPlan.yaml} language="yaml" readOnly height="260px" />
              </>
            )}
          </div>
        )}
      </div>
    </Dialog>
  )
}
