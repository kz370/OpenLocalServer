import { open, save } from '@tauri-apps/plugin-dialog'
import { Copy, Download, FileUp, Layers, Pencil, Plus, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { type EnvironmentManifest, type Profile, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

/** What a profile sets up, as short tags. */
function summary(e: EnvironmentManifest): string[] {
  const out: string[] = []
  if (e.runtime.php) out.push(`PHP ${e.runtime.php}`)
  if (e.runtime.node) out.push(`Node ${e.runtime.node}`)
  if (e.runtime.python) out.push(`Python ${e.runtime.python}`)
  if (e.package_manager) out.push(e.package_manager)
  if (e.database) out.push(e.database.engine === 'mysql' ? 'MySQL (MariaDB)' : e.database.engine)
  for (const [k, v] of Object.entries(e.services ?? {})) if (v === true || (typeof v === 'object' && v.enabled)) out.push(k)
  if (e.domain) out.push(e.domain.https ? 'HTTPS site' : 'site')
  if (Object.keys(e.workers ?? {}).length) out.push('queue worker')
  if (e.scheduler) out.push('scheduler')
  return out
}

const TEMPLATE = `id: my-profile
name: My profile
description: What this environment is for.
environment:
  runtime:
    php: "8.4"
  domain:
    hostname: ""
    https: true
  database:
    engine: mariadb
  services:
    redis: true
`

/** §69, §132: reusable environments, their import and export. */
export function ProfilesPage() {
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [editor, setEditor] = useState<{ title: string; yaml: string } | null>(null)
  const [review, setReview] = useState<{ source: string; profile: Profile } | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_profiles' })
    if (r.type === 'profiles') setProfiles(r.profiles)
  }, [])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  async function edit(p: Profile, copy: boolean) {
    await run(`yaml:${p.id}`, async () => {
      const r = await runCommand({ type: 'profile_yaml', id: p.id })
      if (r.type !== 'text') return
      const yaml = copy ? r.text.replace(/^id: .*$/m, `id: ${p.id}-copy`).replace(/^name: .*$/m, `name: ${p.name} (copy)`) : r.text
      setEditor({ title: copy ? `Copy of ${p.name}` : p.name, yaml })
    })
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Profiles</h1>
          <p className="text-sm text-muted-foreground">Reusable environments. Use one on a project's Environment tab; it writes the project's manifest, and the plan shows what it will set up.</p>
        </div>
        <div className="flex gap-2">
          <Button
            size="sm"
            variant="secondary"
            onClick={async () => {
              const source = await open({ multiple: false, title: 'Import a profile', filters: [{ name: 'Profile', extensions: ['yaml', 'yml'] }] })
              if (!source || Array.isArray(source)) return
              await run('read', async () => {
                const r = await runCommand({ type: 'read_profile_file', source })
                if (r.type === 'profile') setReview({ source, profile: r.profile })
              })
            }}
          >
            <FileUp /> Import
          </Button>
          <Button size="sm" onClick={() => setEditor({ title: 'New profile', yaml: TEMPLATE })}>
            <Plus /> New profile
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {profiles.map((p) => (
          <Card key={p.id}>
            <CardHeader className="pb-2">
              <CardTitle className="flex items-center gap-2 text-base">
                <Layers className="size-4 text-muted-foreground" />
                {p.name}
                {p.builtin ? <Badge variant="secondary">built-in</Badge> : <Badge variant="outline">yours</Badge>}
              </CardTitle>
              <CardDescription>{p.description}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <div className="flex flex-wrap gap-1">
                {summary(p.environment).map((t) => (
                  <Badge key={t} variant="outline" className="font-normal">
                    {t}
                  </Badge>
                ))}
                {summary(p.environment).length === 0 && <span className="text-xs text-muted-foreground">Empty: add what you need.</span>}
              </div>
              <div className="flex flex-wrap gap-1">
                {!p.builtin && (
                  <Button size="sm" variant="ghost" onClick={() => edit(p, false)} disabled={busy !== null}>
                    <Pencil /> Edit
                  </Button>
                )}
                <Button size="sm" variant="ghost" onClick={() => edit(p, true)} disabled={busy !== null}>
                  <Copy /> Copy
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={async () => {
                    const dest = await save({ title: 'Export profile', defaultPath: `${p.id}.yaml`, filters: [{ name: 'Profile', extensions: ['yaml'] }] })
                    if (dest) await run(`exp:${p.id}`, () => runCommand({ type: 'export_profile', id: p.id, dest }))
                  }}
                >
                  <Download /> Export
                </Button>
                {!p.builtin && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={async () => {
                      if (await confirmAction(`Delete the profile "${p.name}"? Projects that used it keep their manifests.`))
                        await run(`del:${p.id}`, async () => {
                          await runCommand({ type: 'delete_profile', id: p.id })
                          await load()
                        })
                    }}
                  >
                    <Trash2 /> Delete
                  </Button>
                )}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {editor && (
        <ProfileEditor
          title={editor.title}
          initial={editor.yaml}
          onClose={() => setEditor(null)}
          onSaved={async () => {
            setEditor(null)
            await load()
          }}
        />
      )}

      <Dialog
        open={review !== null}
        onClose={() => setReview(null)}
        title={`Import "${review?.profile.name ?? ''}"?`}
        description="Review what it sets up. Importing only saves the profile; nothing is installed until you use it on a project and apply the plan."
        footer={
          <>
            <Button variant="ghost" onClick={() => setReview(null)}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null}
              onClick={() =>
                run('import', async () => {
                  if (review) await runCommand({ type: 'import_profile', source: review.source })
                  setReview(null)
                  await load()
                })
              }
            >
              Import
            </Button>
          </>
        }
      >
        {review && (
          <div className="flex flex-col gap-2 text-sm">
            <p>{review.profile.description}</p>
            <div className="flex flex-wrap gap-1">
              {summary(review.profile.environment).map((t) => (
                <Badge key={t} variant="outline">
                  {t}
                </Badge>
              ))}
            </div>
            <pre className="max-h-64 overflow-auto rounded-md bg-muted p-2 font-mono text-[11px]">{JSON.stringify(review.profile.environment, null, 2)}</pre>
          </div>
        )}
      </Dialog>
    </div>
  )
}

function ProfileEditor({ title, initial, onClose, onSaved }: { title: string; initial: string; onClose: () => void; onSaved: () => void }) {
  const [yaml, setYaml] = useState(initial)
  const { busy, error, setError, run } = useAction()
  return (
    <Dialog
      open
      wide
      onClose={onClose}
      title={title}
      description="Same fields as .openlocalserver/environment.yaml under `environment:`. The site name and database name are filled in per project."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={busy !== null}
            onClick={() =>
              run('save', async () => {
                await runCommand({ type: 'save_profile_yaml', yaml })
                onSaved()
              })
            }
          >
            {busy ? <Spinner /> : null} Save profile
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="overflow-hidden rounded-lg border border-border">
          <CodeEditor value={yaml} onChange={setYaml} language="yaml" height="420px" />
        </div>
      </div>
    </Dialog>
  )
}
