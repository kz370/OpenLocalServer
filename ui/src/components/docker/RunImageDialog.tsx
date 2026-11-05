import { Play, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input, NumberInput } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { type Diagnostic, runCommand } from '@/core'
import { notify } from '@/lib/notify'

/** §166: create and start a container from an image. Ports are published on 127.0.0.1 only. */
export function RunImageDialog({ image, onClose, onStarted }: { image: string; onClose: () => void; onStarted: () => void }) {
  const [name, setName] = useState('')
  const [ports, setPorts] = useState<{ host: number | null; container: number | null }[]>([])
  const [env, setEnv] = useState<{ key: string; value: string }[]>([])
  const [restart, setRestart] = useState('unless-stopped')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  async function run() {
    setBusy(true)
    setError(null)
    try {
      await runCommand({
        type: 'run_docker_image',
        spec: {
          image,
          name: name.trim() || null,
          ports: ports.filter((p) => p.host && p.container).map((p) => [p.host!, p.container!]),
          env: env.filter((e) => e.key.trim()).map((e) => [e.key.trim(), e.value]),
          restart_policy: restart,
        },
      })
      notify.success(`${name.trim() || image} is running.`, { title: 'Container started' })
      onStarted()
      onClose()
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
      size="form"
      icon={<Play className="size-5 text-emerald-500" />}
      title={`Run ${image}`}
      description="Creates a container from this image and starts it. Ports open on this computer only (127.0.0.1)."
      footer={
        <div className="flex w-full justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" className="cursor-pointer" disabled={busy} onClick={() => void run()}>
            {busy && <Spinner className="size-3.5" />} Run
          </Button>
        </div>
      }
    >
      <div className="@container flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-3 @lg:grid-cols-2">
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Name (optional)</span>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Docker picks one" />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="font-medium">Restart</span>
            <Select value={restart} onChange={(e) => setRestart(e.target.value)}>
              <option value="no">Don&apos;t restart</option>
              <option value="unless-stopped">Unless stopped by you</option>
              <option value="always">Always</option>
              <option value="on-failure">Only when it fails</option>
            </Select>
          </label>
        </div>
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <span className="font-medium">Ports (this computer → container)</span>
            <Button size="sm" variant="ghost" className="h-7 cursor-pointer text-xs" onClick={() => setPorts([...ports, { host: null, container: null }])}>
              <Plus className="size-3.5" /> Add
            </Button>
          </div>
          {ports.length === 0 && <p className="text-xs text-muted-foreground">No ports published.</p>}
          {ports.map((p, i) => (
            <div key={i} className="flex items-center gap-2">
              <div className="w-36">
                <NumberInput value={p.host} onChange={(v) => setPorts(ports.map((x, j) => (j === i ? { ...x, host: v } : x)))} min={1} max={65535} label="port on this computer" placeholder="8080" className="h-8" />
              </div>
              <span className="text-muted-foreground">→</span>
              <div className="w-36">
                <NumberInput value={p.container} onChange={(v) => setPorts(ports.map((x, j) => (j === i ? { ...x, container: v } : x)))} min={1} max={65535} label="container port" placeholder="80" className="h-8" />
              </div>
              <Button size="sm" variant="ghost" className="size-8 cursor-pointer p-0" title="Remove port" onClick={() => setPorts(ports.filter((_, j) => j !== i))}>
                <Trash2 className="size-4 text-destructive/80" />
              </Button>
            </div>
          ))}
        </div>
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <span className="font-medium">Environment</span>
            <Button size="sm" variant="ghost" className="h-7 cursor-pointer text-xs" onClick={() => setEnv([...env, { key: '', value: '' }])}>
              <Plus className="size-3.5" /> Add
            </Button>
          </div>
          {env.length === 0 && <p className="text-xs text-muted-foreground">No variables.</p>}
          {env.map((e, i) => (
            <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-2">
              <Input value={e.key} onChange={(ev) => setEnv(env.map((x, j) => (j === i ? { ...x, key: ev.target.value } : x)))} placeholder="NAME" className="h-8 font-mono text-xs" />
              <Input value={e.value} onChange={(ev) => setEnv(env.map((x, j) => (j === i ? { ...x, value: ev.target.value } : x)))} placeholder="value" className="h-8 font-mono text-xs" />
              <Button size="sm" variant="ghost" className="size-8 cursor-pointer p-0" title="Remove variable" onClick={() => setEnv(env.filter((_, j) => j !== i))}>
                <Trash2 className="size-4 text-destructive/80" />
              </Button>
            </div>
          ))}
        </div>
      </div>
    </Dialog>
  )
}
