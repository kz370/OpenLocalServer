import { open } from '@tauri-apps/plugin-dialog'
import { FileArchive, FolderOpen, Plug, Plus, RefreshCw, ShieldCheck, ShieldAlert, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/form'
import { type CatalogView, type PluginInfo, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction } from '@/lib/hooks'

function adds(p: PluginInfo): string[] {
  const out: string[] = []
  if (p.runtimes) out.push(`${p.runtimes} runtime${p.runtimes > 1 ? 's' : ''}`)
  if (p.quick_apps) out.push(`${p.quick_apps} Quick App${p.quick_apps > 1 ? 's' : ''}`)
  if (p.detections) out.push(`${p.detections} detection${p.detections > 1 ? 's' : ''}`)
  if (p.health_checks) out.push(`${p.health_checks} health check${p.health_checks > 1 ? 's' : ''}`)
  return out
}

/** §133–135: declarative plugins and signed catalogs. */
export function PluginsPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([])
  const [catalogs, setCatalogs] = useState<CatalogView[]>([])
  const [review, setReview] = useState<PluginInfo | null>(null)
  const [allowed, setAllowed] = useState(false)
  const [adding, setAdding] = useState(false)
  const [form, setForm] = useState({ name: '', url: '', key: '' })
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const [p, c] = await Promise.all([runCommand({ type: 'list_plugins' }), runCommand({ type: 'list_catalog_sources' })])
    if (p.type === 'plugins') setPlugins(p.plugins)
    if (c.type === 'catalog_sources') setCatalogs(c.catalogs)
  }, [])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  async function install(directory: boolean) {
    const source = await open({ directory, multiple: false, title: directory ? 'Choose the plugin folder' : 'Choose a plugin .zip', filters: directory ? undefined : [{ name: 'Plugin', extensions: ['zip'] }] })
    if (!source || Array.isArray(source)) return
    await run('install', async () => {
      await runCommand({ type: 'install_plugin', source })
      await load()
    })
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Plugins</h1>
          <p className="text-sm text-muted-foreground">
            Add runtimes, Quick Apps, project detections and health checks. A plugin is data, not code, and stays off until you approve exactly what it asks for.
          </p>
        </div>
        <div className="flex gap-2">
          <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => install(true)}>
            <FolderOpen /> From folder
          </Button>
          <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => install(false)}>
            <FileArchive /> From .zip
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {plugins.map((p) => (
          <Card key={p.manifest.id}>
            <CardHeader className="pb-2">
              <CardTitle className="flex flex-wrap items-center gap-2 text-base">
                <Plug className="size-4 text-muted-foreground" />
                {p.manifest.name}
                <span className="text-xs font-normal text-muted-foreground">{p.manifest.version}</span>
                {p.builtin && <Badge variant="secondary">built-in</Badge>}
                {p.problem ? <Badge variant="destructive">unusable</Badge> : p.enabled ? <Badge variant="success">on</Badge> : <Badge variant="outline">off</Badge>}
              </CardTitle>
              <CardDescription>{p.manifest.description || 'No description.'}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              {p.problem && <p className="text-sm text-destructive">{p.problem}</p>}
              <div className="flex flex-wrap gap-1">
                {adds(p).map((t) => (
                  <Badge key={t} variant="outline" className="font-normal">
                    {t}
                  </Badge>
                ))}
                {p.manifest.author && <span className="text-xs text-muted-foreground">by {p.manifest.author}</span>}
              </div>
              <div className="flex flex-wrap gap-1">
                {!p.problem &&
                  (p.enabled ? (
                    <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => run(`off:${p.manifest.id}`, async () => { await runCommand({ type: 'set_plugin_enabled', id: p.manifest.id, enabled: false, approve: [] }); await load() })}>
                      Turn off
                    </Button>
                  ) : (
                    <Button size="sm" disabled={busy !== null} onClick={() => { setAllowed(false); setReview(p) }}>
                      Review and turn on
                    </Button>
                  ))}
                {!p.builtin && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={async () => {
                      if (await confirmAction(`Remove ${p.manifest.name}? Its runtimes stay installed, but stop being listed.`, 'Remove plugin')) {
                        await run(`rm:${p.manifest.id}`, async () => { await runCommand({ type: 'remove_plugin', id: p.manifest.id }); await load() })
                      }
                    }}
                  >
                    <Trash2 /> Remove
                  </Button>
                )}
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold tracking-tight">Catalogs</h2>
          <p className="text-sm text-muted-foreground">
            A catalog lists runtimes and plugins and is signed by its publisher. Nothing in it is read unless the signature matches the public key you added.
          </p>
        </div>
        <div className="flex gap-2">
          <Button size="sm" variant="secondary" disabled={busy !== null || catalogs.length === 0} onClick={() => run('refresh', async () => { const r = await runCommand({ type: 'refresh_catalogs', id: null }); if (r.type === 'catalog_sources') setCatalogs(r.catalogs) })}>
            {busy === 'refresh' ? <Spinner /> : <RefreshCw />} Refresh all
          </Button>
          <Button size="sm" onClick={() => setAdding(true)}>
            <Plus /> Add catalog
          </Button>
        </div>
      </div>

      {catalogs.length === 0 && <p className="text-sm text-muted-foreground">No catalogs yet. Add one with its address and the publisher's minisign public key.</p>}
      {catalogs.map((c) => (
        <Card key={c.source.id}>
          <CardHeader className="pb-2">
            <CardTitle className="flex flex-wrap items-center gap-2 text-base">
              {c.verified ? <ShieldCheck className="size-4 text-success" /> : <ShieldAlert className="size-4 text-warning" />}
              {c.source.name}
              {c.verified ? <Badge variant="success">signature verified</Badge> : <Badge variant="warning">not verified</Badge>}
              {c.refreshed_ms && <span className="text-xs font-normal text-muted-foreground">downloaded {timeAgo(c.refreshed_ms)}</span>}
            </CardTitle>
            <CardDescription className="break-all">{c.source.url}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            {c.error && <p className="text-sm text-warning">{c.error}</p>}
            {c.note && <p className="text-xs text-muted-foreground">Publisher's note: {c.note}</p>}
            {c.doc && (
              <div className="flex flex-col gap-2 text-sm">
                <div>
                  <span className="text-muted-foreground">Runtimes:</span> {c.doc.runtimes.length ? c.doc.runtimes.map((r) => `${r.name} ${r.version}`).join(', ') : 'none'}
                </div>
                {c.doc.plugins.map((p) => {
                  const have = plugins.some((x) => x.manifest.id === p.id)
                  return (
                    <div key={p.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border px-3 py-2">
                      <div>
                        <span className="font-medium">{p.name}</span> <span className="text-xs text-muted-foreground">{p.version}</span>
                        <div className="text-xs text-muted-foreground">{p.description}</div>
                      </div>
                      <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => run(`cp:${p.id}`, async () => { await runCommand({ type: 'install_catalog_plugin', source_id: c.source.id, plugin_id: p.id }); await load() })}>
                        {busy === `cp:${p.id}` ? <Spinner /> : null} {have ? 'Reinstall' : 'Install'}
                      </Button>
                    </div>
                  )
                })}
                {c.doc.quick_app_sources.length > 0 && (
                  <div className="text-xs text-muted-foreground">Quick App sources: {c.doc.quick_app_sources.map((q) => q.name).join(', ')}. Import them from the Quick Apps page; they stay untrusted until you approve.</div>
                )}
              </div>
            )}
            <div className="flex gap-1">
              <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => run(`rf:${c.source.id}`, async () => { const r = await runCommand({ type: 'refresh_catalogs', id: c.source.id }); if (r.type === 'catalog_sources') setCatalogs(r.catalogs) })}>
                {busy === `rf:${c.source.id}` ? <Spinner /> : <RefreshCw />} Refresh
              </Button>
              <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => run(`rmc:${c.source.id}`, async () => { const r = await runCommand({ type: 'remove_catalog_source', id: c.source.id }); if (r.type === 'catalog_sources') setCatalogs(r.catalogs) })}>
                <Trash2 /> Remove
              </Button>
            </div>
          </CardContent>
        </Card>
      ))}

      <Dialog
        open={review !== null}
        onClose={() => setReview(null)}
        title={review ? `Turn on ${review.manifest.name}?` : ''}
        description="It can do only what is listed here."
        footer={
          <>
            <Button variant="ghost" onClick={() => setReview(null)}>
              Cancel
            </Button>
            <Button
              disabled={!allowed || busy !== null}
              onClick={() =>
                review &&
                run('on', async () => {
                  await runCommand({ type: 'set_plugin_enabled', id: review.manifest.id, enabled: true, approve: review.manifest.permissions })
                  setReview(null)
                  await load()
                })
              }
            >
              Turn on
            </Button>
          </>
        }
      >
        {review && (
          <div className="flex flex-col gap-3 text-sm">
            {review.permissions.length === 0 && <p>It asks for no permissions.</p>}
            <ul className="flex flex-col gap-2">
              {review.permissions.map((p) => (
                <li key={p.id} className="rounded-md border border-border px-3 py-2">
                  <span className="font-mono text-xs text-muted-foreground">{p.id}</span>
                  <div>{p.description}</div>
                </li>
              ))}
            </ul>
            <div className="text-xs text-muted-foreground">Adds: {adds(review).join(', ') || 'nothing yet'}.{review.folder ? ` Installed in ${review.folder}.` : ''}</div>
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={allowed} onChange={(e) => setAllowed(e.target.checked)} />
              I allow these
            </label>
          </div>
        )}
      </Dialog>

      <Dialog
        open={adding}
        onClose={() => setAdding(false)}
        title="Add a catalog"
        description="You are trusting whoever holds the matching secret key to list runtimes and plugins here."
        footer={
          <>
            <Button variant="ghost" onClick={() => setAdding(false)}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null || !form.name || !form.url || !form.key}
              onClick={() =>
                run('addc', async () => {
                  const r = await runCommand({ type: 'add_catalog_source', name: form.name, url: form.url, public_key: form.key })
                  if (r.type !== 'catalog_sources') return
                  setCatalogs(r.catalogs)
                  setAdding(false)
                  setForm({ name: '', url: '', key: '' })
                  const id = r.catalogs.find((c) => c.source.name === form.name.trim())?.source.id ?? null
                  const f = await runCommand({ type: 'refresh_catalogs', id })
                  if (f.type === 'catalog_sources') setCatalogs(f.catalogs)
                })
              }
            >
              Add and download
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <Field label="Name">
            <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="Company catalog" />
          </Field>
          <Field label="Address" hint="https:// address of the catalog file; its signature is expected at the same address plus .minisig">
            <Input value={form.url} onChange={(e) => setForm({ ...form, url: e.target.value })} placeholder="https://example.com/catalog.json" />
          </Field>
          <Field label="Publisher's public key" hint="The minisign public key (the RW… line, or the whole .pub file)">
            <Textarea rows={3} value={form.key} onChange={(e) => setForm({ ...form, key: e.target.value })} />
          </Field>
        </div>
      </Dialog>
    </div>
  )
}
