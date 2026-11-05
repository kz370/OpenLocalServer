import { BookmarkPlus, Plus, Trash2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Input, NumberInput } from '@/components/ui/input'
import { type Diagnostic, type ServicePreset, runCommand } from '@/core'

type EnvRow = { key: string; value: string; secret: boolean }

/** A blank preset, or one started from an image someone typed in the builder. */
export function blankPreset(image = '', containerPort: number | null = null): ServicePreset {
  // `nginx:1.28` -> image `nginx`, tag `1.28`. A registry port (`host:5000/app`) stays in the name.
  const lastSlash = image.lastIndexOf('/')
  const colon = image.lastIndexOf(':')
  const hasTag = colon > lastSlash
  return {
    id: '',
    name: '',
    category: 'My presets',
    description: '',
    image: hasTag ? image.slice(0, colon) : image,
    tag: '',
    tags: hasTag ? [image.slice(colon + 1)] : [],
    default_service: 'app',
    ports: containerPort ? [{ container: containerPort, default_host: Math.max(containerPort, 1024), label: 'Web', web: true }] : [],
    env: [],
    volumes: [],
    command: null,
    working_dir: null,
    custom: true,
  }
}

function Section({ title, onAdd, children }: { title: string; onAdd: () => void; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium">{title}</span>
        <Button size="sm" variant="ghost" className="h-7 cursor-pointer text-xs" onClick={onAdd}>
          <Plus className="size-3.5" /> Add
        </Button>
      </div>
      {children}
    </div>
  )
}

/**
 * §166: create or edit one of the user's own builder presets. The core checks every
 * field (it is rendered into compose files) and refuses built-in ids.
 */
export function PresetEditorDialog({
  initial,
  onClose,
  onSaved,
}: {
  initial: ServicePreset
  onClose: () => void
  onSaved: (presets: ServicePreset[]) => void
}) {
  const editing = !!initial.id
  const [p, setP] = useState<ServicePreset>(initial)
  const [tags, setTags] = useState(initial.tags.join(', '))
  const [env, setEnv] = useState<EnvRow[]>(
    initial.env.map((e) => ({ key: e.key, value: e.value.kind === 'literal' ? e.value.value : '', secret: e.value.kind === 'secret' })),
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  const set = (patch: Partial<ServicePreset>) => setP({ ...p, ...patch })

  async function save() {
    setBusy(true)
    setError(null)
    try {
      const preset: ServicePreset = {
        ...p,
        // A renamed preset gets a new id from its new name.
        id: editing && p.name.trim() === initial.name.trim() ? initial.id : '',
        tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
        env: env
          .filter((e) => e.key.trim())
          .map((e) => ({ key: e.key.trim(), value: e.secret ? { kind: 'secret' as const } : { kind: 'literal' as const, value: e.value } })),
      }
      const r = await runCommand({ type: 'save_docker_compose_preset', preset, previous_id: editing ? initial.id : null })
      if (r.type === 'docker_compose_presets') {
        onSaved(r.presets)
        onClose()
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      size="wide"
      layer="sub"
      icon={<BookmarkPlus className="size-5 text-violet-500" />}
      title={editing ? `Edit preset · ${initial.name}` : 'New preset'}
      description="Your own service for the compose builder. It appears under its category next to the built-in ones."
      footer={
        <div className="flex w-full justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" className="cursor-pointer" disabled={busy || !p.name.trim() || !p.image.trim() || !tags.trim()} onClick={() => void save()}>
            {busy && <Spinner className="size-3.5" />} Save preset
          </Button>
        </div>
      }
    >
      <div className="@container flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-3 @xl:grid-cols-2">
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Name</span>
            <Input value={p.name} onChange={(e) => set({ name: e.target.value })} placeholder="Company API" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Category</span>
            <Input value={p.category} onChange={(e) => set({ category: e.target.value })} placeholder="My presets" />
          </label>
          <label className="flex flex-col gap-1.5 @xl:col-span-2">
            <span className="font-medium">Description</span>
            <Input value={p.description} onChange={(e) => set({ description: e.target.value })} placeholder="What it is, in a few words" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Image (without version)</span>
            <Input value={p.image} onChange={(e) => set({ image: e.target.value })} placeholder="ghcr.io/acme/api" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Versions (first is the default)</span>
            <Input value={tags} onChange={(e) => setTags(e.target.value)} placeholder="2.4, 2.3" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Service name</span>
            <Input value={p.default_service} onChange={(e) => set({ default_service: e.target.value })} placeholder="api" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Working folder (optional)</span>
            <Input value={p.working_dir ?? ''} onChange={(e) => set({ working_dir: e.target.value || null })} placeholder="/app" />
          </label>
          <label className="flex flex-col gap-1.5 @xl:col-span-2">
            <span className="font-medium">Command (optional)</span>
            <Input value={p.command ?? ''} onChange={(e) => set({ command: e.target.value || null })} placeholder="npm run start" />
          </label>
        </div>

        <Section title="Ports" onAdd={() => set({ ports: [...p.ports, { container: 80, default_host: 8080, label: 'Web', web: true }] })}>
          <p className="text-xs text-muted-foreground">
            {p.ports.length === 0
              ? 'No ports.'
              : 'Label · port inside the container · default port on this computer (OLS picks the next free one when it is taken).'}
          </p>
          {p.ports.map((port, i) => (
            <div key={i} className="grid grid-cols-2 items-center gap-2 rounded-md border p-2 @xl:grid-cols-[minmax(0,1fr)_8rem_8rem_auto_auto]">
              <Input
                value={port.label}
                onChange={(e) => set({ ports: p.ports.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)) })}
                placeholder="Label"
                className="col-span-2 h-8 @xl:col-span-1"
              />
              <NumberInput
                value={port.container}
                onChange={(v) => set({ ports: p.ports.map((x, j) => (j === i ? { ...x, container: v ?? 0 } : x)) })}
                min={1}
                max={65535}
                label="container port"
                placeholder="in container"
                className="h-8"
              />
              <NumberInput
                value={port.default_host}
                onChange={(v) => set({ ports: p.ports.map((x, j) => (j === i ? { ...x, default_host: v ?? 0 } : x)) })}
                min={1}
                max={65535}
                label="port on this computer"
                placeholder="on this PC"
                className="h-8"
              />
              <label className="flex items-center gap-1.5 text-xs">
                <Checkbox checked={port.web} onChange={(v) => set({ ports: p.ports.map((x, j) => (j === i ? { ...x, web: v } : x)) })} />
                Web page
              </label>
              <Button
                size="sm"
                variant="ghost"
                className="size-8 cursor-pointer justify-self-end p-0"
                title="Remove port"
                onClick={() => set({ ports: p.ports.filter((_, j) => j !== i) })}
              >
                <Trash2 className="size-4 text-destructive/80" />
              </Button>
            </div>
          ))}
        </Section>

        <Section title="Environment" onAdd={() => setEnv([...env, { key: '', value: '', secret: false }])}>
          {env.length === 0 && <p className="text-xs text-muted-foreground">No variables.</p>}
          {env.map((e, i) => (
            <div key={i} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-md border p-2 @xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto_auto]">
              <Input
                value={e.key}
                onChange={(ev) => setEnv(env.map((x, j) => (j === i ? { ...x, key: ev.target.value } : x)))}
                placeholder="NAME"
                className="h-8 font-mono"
              />
              <Input
                value={e.secret ? '' : e.value}
                disabled={e.secret}
                onChange={(ev) => setEnv(env.map((x, j) => (j === i ? { ...x, value: ev.target.value } : x)))}
                placeholder={e.secret ? 'generated password' : 'value'}
                className="order-3 col-span-2 h-8 font-mono @xl:order-none @xl:col-span-1"
              />
              <label
                className="order-4 flex items-center gap-1.5 text-xs @xl:order-none"
                title="OLS generates a password and keeps it in the Windows credential store"
              >
                <Checkbox checked={e.secret} onChange={(v) => setEnv(env.map((x, j) => (j === i ? { ...x, secret: v } : x)))} />
                Secret
              </label>
              <Button
                size="sm"
                variant="ghost"
                className="size-8 cursor-pointer justify-self-end p-0"
                title="Remove variable"
                onClick={() => setEnv(env.filter((_, j) => j !== i))}
              >
                <Trash2 className="size-4 text-destructive/80" />
              </Button>
            </div>
          ))}
        </Section>

        <Section title="Volumes" onAdd={() => set({ volumes: [...p.volumes, { source: 'data', target: '/data', read_only: false }] })}>
          {p.volumes.length === 0 && <p className="text-xs text-muted-foreground">No volumes.</p>}
          {p.volumes.map((v, i) => (
            <div key={i} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-md border p-2 @xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto_auto]">
              <Input
                value={v.source}
                onChange={(e) => set({ volumes: p.volumes.map((x, j) => (j === i ? { ...x, source: e.target.value } : x)) })}
                placeholder="data or ./folder"
                className="h-8 font-mono"
              />
              <Input
                value={v.target}
                onChange={(e) => set({ volumes: p.volumes.map((x, j) => (j === i ? { ...x, target: e.target.value } : x)) })}
                placeholder="/path/in/container"
                className="order-3 col-span-2 h-8 font-mono @xl:order-none @xl:col-span-1"
              />
              <label className="order-4 flex items-center gap-1.5 text-xs @xl:order-none">
                <Checkbox
                  checked={v.read_only}
                  onChange={(c) => set({ volumes: p.volumes.map((x, j) => (j === i ? { ...x, read_only: c } : x)) })}
                />
                Read-only
              </label>
              <Button
                size="sm"
                variant="ghost"
                className="size-8 cursor-pointer justify-self-end p-0"
                title="Remove volume"
                onClick={() => set({ volumes: p.volumes.filter((_, j) => j !== i) })}
              >
                <Trash2 className="size-4 text-destructive/80" />
              </Button>
            </div>
          ))}
          <p className="text-xs text-muted-foreground">
            A plain name (<code>data</code>) is a Docker volume that keeps its data. <code>./folder</code> is a folder inside the
            project.
          </p>
        </Section>
      </div>
    </Dialog>
  )
}
