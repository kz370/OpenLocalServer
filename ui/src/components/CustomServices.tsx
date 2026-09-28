import { open } from '@tauri-apps/plugin-dialog'
import { FolderSearch, Pencil, Plus, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Textarea, Toggle } from '@/components/ui/form'
import { Input, NumberInput } from '@/components/ui/input'
import { type CustomServiceDef, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmThen } from '@/lib/confirm'

/** The form works on text: one argument per line, `KEY=value` per line. */
interface Draft {
  id: string
  name: string
  executable: string
  args: string
  cwd: string
  env: string
  port: string
  health: 'none' | 'tcp' | 'http'
  path: string
  restart: boolean
}

const EMPTY: Draft = { id: '', name: '', executable: '', args: '', cwd: '', env: '', port: '', health: 'none', path: '/', restart: false }

function toDraft(s: CustomServiceDef): Draft {
  return {
    id: s.id,
    name: s.name,
    executable: s.executable,
    args: s.args.join('\n'),
    cwd: s.cwd ?? '',
    env: s.env.map(([k, v]) => `${k}=${v}`).join('\n'),
    port: s.port ? String(s.port) : '',
    health: s.health.kind,
    path: s.health.kind === 'http' ? s.health.path : '/',
    restart: s.restart_on_crash,
  }
}

function fromDraft(d: Draft): CustomServiceDef {
  const lines = (text: string) => text.split(/\r?\n/).filter((l) => l.trim() !== '')
  return {
    id: d.id,
    name: d.name.trim(),
    executable: d.executable.trim(),
    args: lines(d.args),
    cwd: d.cwd.trim() || null,
    env: lines(d.env).map((l) => {
      const i = l.indexOf('=')
      return (i < 0 ? [l.trim(), ''] : [l.slice(0, i).trim(), l.slice(i + 1)]) as [string, string]
    }),
    port: d.port.trim() ? Number(d.port) : null,
    health: d.health === 'http' ? { kind: 'http', path: d.path.trim() } : { kind: d.health },
    restart_on_crash: d.restart,
  }
}

/** §67: run any program as a service — start, stop, port and health check like the built-in ones. */
export function CustomServices({ onChanged }: { onChanged: () => void }) {
  const [services, setServices] = useState<CustomServiceDef[]>([])
  const [draft, setDraft] = useState<Draft | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_custom_services' })
    if (r.type === 'custom_services') setServices(r.services)
  }, [])

  useEffect(() => {
    load().catch((e) => setError(e))
  }, [load, setError])

  async function browse(kind: 'file' | 'folder') {
    const picked = await open(
      kind === 'file'
        ? { multiple: false, directory: false, title: 'Choose the program', filters: [{ name: 'Program', extensions: ['exe', 'bat', 'cmd'] }] }
        : { multiple: false, directory: true, title: 'Choose the working folder' },
    )
    if (!picked || Array.isArray(picked) || !draft) return
    setDraft(kind === 'file' ? { ...draft, executable: picked } : { ...draft, cwd: picked })
  }

  const save = () =>
    run('save', async () => {
      await runCommand({ type: 'save_custom_service', service: fromDraft(draft!) })
      setDraft(null)
      await load()
      onChanged()
    })

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between gap-3">
          <div>
            <CardTitle className="text-sm">Custom services</CardTitle>
            <CardDescription>Any program you want started and watched like the built-in services. It shows up in the list above.</CardDescription>
          </div>
          <Button size="sm" variant="secondary" onClick={() => setDraft({ ...EMPTY })}>
            <Plus /> Add service
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {!draft && <ErrorCard error={error} onDismiss={() => setError(null)} />}
        {services.map((s) => (
          <div key={s.id} className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-1.5 text-sm">
            <div className="min-w-0">
              <div className="font-medium">{s.name}</div>
              <div className="truncate font-mono text-xs text-muted-foreground">
                {s.executable} {s.args.join(' ')}
              </div>
            </div>
            <span className="flex shrink-0 gap-1">
              <Button size="sm" variant="ghost" title="Edit" onClick={() => setDraft(toDraft(s))}>
                <Pencil className="size-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                title="Remove (stops it first)"
                disabled={busy !== null}
                onClick={() =>
                  confirmThen(`Remove ${s.name}? It is stopped first. The program itself is not deleted.`, () =>
                    run('remove', async () => {
                      await runCommand({ type: 'remove_custom_service', id: s.id })
                      await load()
                      onChanged()
                    }),
                  )
                }
              >
                <Trash2 className="size-3.5" />
              </Button>
            </span>
          </div>
        ))}
        {services.length === 0 && <p className="text-sm text-muted-foreground">None yet.</p>}
      </CardContent>

      {draft && (
        <Dialog
          open
          wide
          onClose={() => setDraft(null)}
          title={draft.id ? 'Edit custom service' : 'Add custom service'}
          description="The program runs directly, not through a shell. Changes apply the next time it starts."
          footer={
            <>
              <Button variant="ghost" onClick={() => setDraft(null)}>
                Cancel
              </Button>
              <Button disabled={busy !== null || !draft.name.trim() || !draft.executable.trim()} onClick={save}>
                Save
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <ErrorCard error={error} onDismiss={() => setError(null)} />
            <Field label="Name">
              <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="My API" />
            </Field>
            <Field label="Program">
              <div className="flex gap-2">
                <Input value={draft.executable} onChange={(e) => setDraft({ ...draft, executable: e.target.value })} placeholder="C:\tools\my-api.exe" />
                <Button variant="outline" onClick={() => browse('file')} title="Browse">
                  <FolderSearch />
                </Button>
              </div>
            </Field>
            <Field label="Arguments" hint="One per line, exactly as the program should receive them.">
              <Textarea rows={3} value={draft.args} onChange={(e) => setDraft({ ...draft, args: e.target.value })} placeholder={'--port\n9000'} />
            </Field>
            <Field label="Working folder (optional)">
              <div className="flex gap-2">
                <Input value={draft.cwd} onChange={(e) => setDraft({ ...draft, cwd: e.target.value })} />
                <Button variant="outline" onClick={() => browse('folder')} title="Browse">
                  <FolderSearch />
                </Button>
              </div>
            </Field>
            <Field label="Environment variables (optional)" hint="One KEY=value per line.">
              <Textarea rows={2} value={draft.env} onChange={(e) => setDraft({ ...draft, env: e.target.value })} placeholder="APP_ENV=local" />
            </Field>
            <div className="grid gap-3 sm:grid-cols-3">
              <Field label="Port (optional)">
                <NumberInput
                  label="port"
                  min={1}
                  max={65535}
                  placeholder="9000"
                  value={draft.port === '' ? null : Number(draft.port)}
                  onChange={(n) => setDraft({ ...draft, port: n === null ? '' : String(n) })}
                />
              </Field>
              <Field label="Health check">
                <Select value={draft.health} onChange={(e) => setDraft({ ...draft, health: e.target.value as Draft['health'] })}>
                  <option value="none">None</option>
                  <option value="tcp">Port accepts connections</option>
                  <option value="http">HTTP request succeeds</option>
                </Select>
              </Field>
              {draft.health === 'http' && (
                <Field label="Path">
                  <Input value={draft.path} onChange={(e) => setDraft({ ...draft, path: e.target.value })} placeholder="/health" />
                </Field>
              )}
            </div>
            <Toggle checked={draft.restart} onChange={(restart) => setDraft({ ...draft, restart })} label="Restart if it crashes" hint="Up to three times." />
          </div>
        </Dialog>
      )}
    </Card>
  )
}
