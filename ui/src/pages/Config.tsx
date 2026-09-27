import { save } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, Download, FolderOpen, History, Plus, RotateCcw, Save, ShieldCheck, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { CodeEditor, DiffView, type EditorLanguage } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type ConfigFile, type ConfigVersion, type Domain, type Ownership, type SiteBlocks, type WebConfig, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmThen } from '@/lib/confirm'

const OWNERSHIP_INFO: Record<Ownership, { label: string; blurb: string }> = {
  managed: { label: 'Managed', blurb: 'OpenLocalServer writes this file from the site settings. Hand edits are reported as drift.' },
  advanced: { label: 'Advanced', blurb: 'OpenLocalServer writes the file and includes your own snippet inside it. Your snippet is never overwritten.' },
  manual: { label: 'Manual', blurb: 'You own the whole file. OpenLocalServer validates and reloads it but never rewrites it.' },
}

const fileKey = (f: ConfigFile) => `${f.hostname ?? ''}|${f.part}`

function languageFor(server: string): EditorLanguage {
  return server === 'nginx' ? 'nginx' : server === 'apache' ? 'apache' : 'text'
}

export function ConfigPage() {
  const [files, setFiles] = useState<ConfigFile[]>([])
  const [cfg, setCfg] = useState<WebConfig | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [historyTick, setHistoryTick] = useState(0)
  const { error, setError } = useAction()

  const file = useMemo(() => files.find((f) => fileKey(f) === selected) ?? null, [files, selected])
  const history = useConfigHistory(file?.hostname ?? null)

  async function refreshList(keep?: string) {
    const [l, c] = await Promise.all([runCommand({ type: 'list_web_configs' }), runCommand({ type: 'get_web_config' })])
    if (l.type === 'configs') {
      setFiles(l.files)
      if (!keep && !selected && l.files.length > 0) setSelected(fileKey(l.files[0]))
    }
    if (c.type === 'web_config') setCfg(c.config)
  }

  useEffect(() => {
    refreshList().catch((e) => setError(e))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const grouped = files.filter((f) => f.part !== 'custom')

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Web config</h1>
        <p className="text-sm text-muted-foreground">
          Every site's generated config for {cfg?.server ?? 'the web server'}. Read it, customise it safely, and roll back any change (§23–29).
        </p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="grid gap-4 lg:grid-cols-[16rem_1fr]">
        <div className="flex min-w-0 flex-col gap-4">
        <Card className="h-fit">
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Files</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1 p-2 pt-0">
            {grouped.map((f) => {
              const custom = files.find((x) => x.hostname === f.hostname && x.part === 'custom')
              return (
                <div key={fileKey(f)}>
                  <FileRow f={f} active={selected === fileKey(f)} onClick={() => setSelected(fileKey(f))} label={f.hostname ?? 'Main config'} />
                  {custom && <FileRow f={custom} active={selected === fileKey(custom)} onClick={() => setSelected(fileKey(custom))} label="↳ your snippet" indent />}
                </div>
              )
            })}
            {grouped.length <= 1 && <p className="px-2 py-3 text-xs text-muted-foreground">Add a site and apply the web config to see its file here.</p>}
          </CardContent>
        </Card>

        {file?.hostname && (
          <Card className="h-fit">
            <CardHeader className="pb-2">
              <CardTitle className="flex items-center gap-1.5 text-sm">
                <History className="size-3.5" /> Timeline
              </CardTitle>
              <CardDescription className="truncate">{file.hostname}</CardDescription>
            </CardHeader>
            <CardContent className="p-2 pt-0">
              <Timeline
                versions={history.versions}
                pick={history.pick}
                onPick={(id) => {
                  history.setPick(id)
                  setHistoryTick((t) => t + 1)
                }}
              />
            </CardContent>
          </Card>
        )}
        </div>

        <div className="flex min-w-0 flex-col gap-3">
          {file ? (
            <ConfigFilePane
              key={selected}
              file={file}
              server={cfg?.server ?? 'nginx'}
              history={history}
              openHistoryTick={historyTick}
              onChanged={async () => {
                await refreshList(selected ?? undefined)
                history.reload()
              }}
            />
          ) : (
            <Card>
              <CardContent className="pt-4 text-sm text-muted-foreground">Select a file.</CardContent>
            </Card>
          )}
        </div>
      </div>
    </div>
  )
}

/**
 * One web-server config file: header, ownership, the editor / structured / history tabs.
 * Used by the Web config page and inside a domain's edit dialog, so both edit the same way.
 * Remount it (`key`) to switch files.
 */
export function ConfigFilePane({
  file,
  server,
  onChanged,
  history,
  openHistoryTick,
}: {
  file: ConfigFile
  server: string
  onChanged: () => void | Promise<void>
  /** Version history owned by the page (shown as its Timeline). Without it the History tab lists versions itself. */
  history?: ConfigHistory
  openHistoryTick?: number
}) {
  const [tab, setTab] = useState<'editor' | 'structured' | 'history'>('editor')
  const [text, setText] = useState('')
  const [saved, setSaved] = useState('')
  const [notice, setNotice] = useState<string | null>(null)
  const [pendingOwnership, setPendingOwnership] = useState<Ownership | null>(null)
  const { busy, error, setError, run } = useAction()
  const ownHistory = useConfigHistory(history ? null : file.hostname)
  const hist = history ?? ownHistory

  useEffect(() => {
    if (openHistoryTick) setTab('history')
  }, [openHistoryTick])

  const dirty = text !== saved
  const editable = !!file.editable

  async function load() {
    const res = await runCommand({ type: 'read_web_config', hostname: file.hostname, part: file.part })
    if (res.type === 'text') {
      setText(res.text)
      setSaved(res.text)
    } else {
      // Never leave a silent blank editor: anything unexpected explains itself.
      setText(`(could not load this file: unexpected response '${res.type}')`)
      setSaved('')
    }
  }

  useEffect(() => {
    load().catch(() => setText('(the file does not exist yet: apply the web config first)'))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file.hostname, file.part])

  async function saveFile() {
    if (!file.hostname) return
    const res = await runCommand({ type: 'write_web_config', hostname: file.hostname, part: file.part, content: text })
    if (res.type === 'text') setNotice(`Saved. The server accepted it and was reloaded.\n${res.text}`)
    setSaved(text)
    await onChanged()
  }

  async function changeOwnership(next: Ownership) {
    if (!file.hostname) return
    await runCommand({ type: 'set_ownership', hostname: file.hostname, ownership: next })
    // Regenerate so the file on disk matches the new ownership immediately.
    await runCommand({ type: 'apply_web', overwrite: next === 'managed' || next === 'advanced' ? [file.hostname] : [] })
    setPendingOwnership(null)
    await onChanged()
    await load()
  }

  async function exportFile() {
    const dest = await save({ title: 'Export config', defaultPath: `${file.hostname ?? 'main'}.conf` })
    if (!dest) return
    if (file.hostname) await runCommand({ type: 'export_web_config', hostname: file.hostname, part: file.part, dest })
  }

  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardContent className="flex flex-wrap items-center justify-between gap-3 pt-4">
          <div className="min-w-0">
            <div className="truncate text-sm font-medium">{file.hostname ?? 'Main config'}{file.part === 'custom' && ' (your snippet)'}</div>
            <div className="truncate font-mono text-xs text-muted-foreground">{file.path}</div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {file.drifted && (
              <Badge variant="warning">
                <AlertTriangle className="size-3" /> edited by hand
              </Badge>
            )}
            {file.hostname && file.ownership && (
              <Select value={file.ownership} onChange={(e) => setPendingOwnership(e.target.value as Ownership)} className="w-40" title="Who owns this file">
                <option value="managed">Managed</option>
                <option value="advanced">Advanced</option>
                <option value="manual">Manual</option>
              </Select>
            )}
            <Button size="sm" variant="ghost" title="Open folder" onClick={() => run('open', () => runCommand({ type: 'open_path', path: file.path.replace(/[\\/][^\\/]*$/, '') }))}>
              <FolderOpen />
            </Button>
            {file.hostname && (
              <Button size="sm" variant="ghost" title="Export" onClick={() => run('export', exportFile)}>
                <Download />
              </Button>
            )}
            <AiButton
              label="Ask AI"
              variant="secondary"
              ask={() => ({
                title: 'Explain or change this config',
                description: 'Reads the text in the editor. A suggested change comes back as a diff for you to apply by hand.',
                request: { feature: 'config', kind: 'web', title: `${file.hostname ?? 'Main'} ${server} config`, text },
                question: 'optional',
                placeholder: 'What do you want? e.g. redirect www to non-www, add gzip',
              })}
            />
            <Button
              size="sm"
              variant="secondary"
              disabled={busy !== null}
              onClick={() =>
                run('validate', async () => {
                  const r = await runCommand({ type: 'validate_web' })
                  if (r.type === 'text') setNotice(`The server accepts the config on disk.\n${r.text}`)
                })
              }
            >
              <ShieldCheck /> Validate
            </Button>
            {editable && (
              <Button size="sm" disabled={!dirty || busy !== null} onClick={() => run('save', saveFile)}>
                <Save /> Save and reload
              </Button>
            )}
          </div>
        </CardContent>
      </Card>

      {notice && <pre className="whitespace-pre-wrap rounded-lg border border-success/40 bg-success/5 p-3 text-xs">{notice}</pre>}

      {file.hostname && file.ownership && (
        <p className="text-xs text-muted-foreground">
          <b>{OWNERSHIP_INFO[file.ownership].label}.</b> {OWNERSHIP_INFO[file.ownership].blurb}
          {!editable && file.part === 'site' && ' To edit this file directly, switch it to Manual; to add your own directives, switch it to Advanced.'}
        </p>
      )}

      <Tabs
        tabs={[
          { id: 'editor', label: 'Editor' },
          ...(file.hostname && file.part === 'site' ? [{ id: 'structured' as const, label: 'Structured' }] : []),
          ...(file.hostname ? [{ id: 'history' as const, label: 'History' }] : []),
        ]}
        value={tab}
        onChange={setTab}
      />

      {tab === 'editor' && <CodeEditor value={text} onChange={setText} readOnly={!editable} language={languageFor(server)} height="480px" />}
      {tab === 'structured' && file.hostname && (
        <StructuredEditor
          hostname={file.hostname}
          onApplied={() => {
            void onChanged()
            void load()
          }}
        />
      )}
      {tab === 'history' && file.hostname && (
        <HistoryPanel
          hostname={file.hostname}
          history={hist}
          listed={!history}
          current={text}
          language={languageFor(server)}
          canRestore={editable}
          onRestored={() => {
            hist.reload()
            void onChanged()
            void load()
          }}
        />
      )}

      {/* §27 protection dialog */}
      <Dialog
        open={pendingOwnership !== null}
        onClose={() => setPendingOwnership(null)}
        title={`Change ownership to ${pendingOwnership ? OWNERSHIP_INFO[pendingOwnership].label : ''}?`}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPendingOwnership(null)}>
              Cancel
            </Button>
            <Button disabled={busy !== null} onClick={() => run('ownership', () => changeOwnership(pendingOwnership!))}>
              Change ownership
            </Button>
          </>
        }
      >
        {pendingOwnership && (
          <div className="flex flex-col gap-2 text-sm">
            <p>{OWNERSHIP_INFO[pendingOwnership].blurb}</p>
            {pendingOwnership === 'manual' && <p>The current generated file becomes yours to edit. It will no longer be regenerated when you change the site's settings.</p>}
            {(pendingOwnership === 'managed' || pendingOwnership === 'advanced') && (
              <p className="text-warning">
                The file is regenerated now. Anything you wrote in the whole file is replaced. A copy is kept in History so you can get it back.
              </p>
            )}
          </div>
        )}
      </Dialog>
    </>
  )
}

/** A domain's own config files (site file plus the Advanced snippet), for its edit dialog. */
export function SiteConfigTab({ hostname }: { hostname: string }) {
  const [files, setFiles] = useState<ConfigFile[]>([])
  const [server, setServer] = useState('nginx')
  const [part, setPart] = useState<'site' | 'custom'>('site')
  const [missing, setMissing] = useState(false)

  async function refresh() {
    const [l, c] = await Promise.all([runCommand({ type: 'list_web_configs' }), runCommand({ type: 'get_web_config' })])
    if (l.type === 'configs') {
      const mine = l.files.filter((f) => f.hostname === hostname)
      setFiles(mine)
      setMissing(mine.length === 0)
    }
    if (c.type === 'web_config') setServer(c.config.server)
  }

  useEffect(() => {
    refresh().catch(() => setMissing(true))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostname])

  const file = files.find((f) => f.part === part) ?? files.find((f) => f.part === 'site') ?? null
  if (missing) return <p className="text-sm text-muted-foreground">This site has no config file yet. Save it (or apply the web config) first.</p>
  if (!file) return null
  const hasSnippet = files.some((f) => f.part === 'custom')

  return (
    <div className="flex flex-col gap-3">
      {hasSnippet && (
        <Tabs
          tabs={[
            { id: 'site', label: 'Site file' },
            { id: 'custom', label: 'Your snippet' },
          ]}
          value={part}
          onChange={setPart}
        />
      )}
      <ConfigFilePane key={`${file.hostname}|${file.part}`} file={file} server={server} onChanged={refresh} />
    </div>
  )
}

function FileRow({ f, active, onClick, label, indent }: { f: ConfigFile; active: boolean; onClick: () => void; label: string; indent?: boolean }) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors ${active ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60'} ${indent ? 'pl-6 text-xs text-muted-foreground' : ''}`}
    >
      <span className="truncate">{label}</span>
      <span className="flex shrink-0 items-center gap-1">
        {f.drifted && <AlertTriangle className="size-3.5 text-warning" />}
        {f.ownership && !indent && <span className="text-[10px] uppercase text-muted-foreground">{f.ownership}</span>}
      </span>
    </button>
  )
}

export interface ConfigHistory {
  versions: ConfigVersion[]
  pick: string | null
  setPick: (id: string | null) => void
  old: string | null
  reload: () => void
}

/** A site's archived config versions, with the picked one's text. Pass null to stay idle. */
function useConfigHistory(hostname: string | null): ConfigHistory {
  const [versions, setVersions] = useState<ConfigVersion[]>([])
  const [pick, setPick] = useState<string | null>(null)
  const [old, setOld] = useState<string | null>(null)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (!hostname) return
    let alive = true
    runCommand({ type: 'list_config_history', hostname }).then((r) => {
      if (!alive || r.type !== 'config_versions') return
      setVersions(r.versions)
      // Open on the most recent version, unless the one being looked at is still there.
      setPick((cur) => (cur && r.versions.some((v) => v.id === cur) ? cur : (r.versions[0]?.id ?? null)))
    })
    return () => {
      alive = false
    }
  }, [hostname, tick])

  useEffect(() => {
    if (!hostname || !pick) return
    let alive = true
    runCommand({ type: 'read_config_history', hostname, id: pick }).then((r) => alive && r.type === 'text' && setOld(r.text))
    return () => {
      alive = false
    }
  }, [pick, hostname])

  return { versions: hostname ? versions : [], pick, setPick, old, reload: () => setTick((t) => t + 1) }
}

/** The list of archived versions grouped by day, like VS Code's Timeline view. */
function Timeline({ versions, pick, onPick }: { versions: ConfigVersion[]; pick: string | null; onPick: (id: string) => void }) {
  if (versions.length === 0) return <p className="px-2 py-3 text-xs text-muted-foreground">Nothing has been replaced yet. Every change to this site's config is archived here first.</p>
  return (
    <div className="flex max-h-[45vh] flex-col gap-3 overflow-y-auto">
      {groupByDay(versions).map(([day, items]) => (
        <div key={day} className="flex flex-col gap-0.5">
          <div className="px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">{day}</div>
          {items.map((v) => (
            <button
              key={v.id}
              onClick={() => onPick(v.id)}
              className={`flex items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors ${pick === v.id ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50'}`}
            >
              <span className="tabular-nums">{new Date(v.timestamp_ms).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' })}</span>
              <span className="text-xs text-muted-foreground">
                {v.part === 'custom' ? 'snippet' : 'site'} · {formatSize(v.bytes)}
              </span>
            </button>
          ))}
        </div>
      ))}
    </div>
  )
}

/** Restore bar plus the diff, full width. `listed` also shows the version list beside it (inside a site's dialog). */
function HistoryPanel({
  hostname,
  history,
  listed,
  current,
  language,
  onRestored,
  canRestore,
}: {
  hostname: string
  history: ConfigHistory
  listed: boolean
  current: string
  language: EditorLanguage
  onRestored: () => void
  canRestore: boolean
}) {
  const { versions, pick, setPick, old } = history
  const [changes, setChanges] = useState<number | null>(null)
  const { busy, error, setError, run } = useAction()
  const picked = versions.find((v) => v.id === pick)

  if (versions.length === 0) {
    return (
      <Card>
        <CardContent className="flex items-center gap-3 p-4 text-sm text-muted-foreground">
          <History className="size-4" /> Nothing has been replaced yet. Every change to this site's config is archived here first.
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className={listed ? 'grid gap-3 md:grid-cols-[15rem_minmax(0,1fr)]' : ''}>
        {listed && (
          <Card className="h-fit">
            <CardContent className="p-2">
              <Timeline versions={versions} pick={pick} onPick={setPick} />
            </CardContent>
          </Card>
        )}
        <div className="flex min-w-0 flex-col gap-2">
          {picked && old !== null && (
            <>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-2 text-sm">
                  {changes === 0 ? (
                    <Badge variant="secondary">Identical to the current file</Badge>
                  ) : changes !== null ? (
                    <Badge variant="outline">
                      {changes} changed {changes === 1 ? 'block' : 'blocks'}
                    </Badge>
                  ) : null}
                </div>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={!canRestore || busy !== null || changes === 0}
                  title={canRestore ? '' : 'Only Manual site files and Advanced snippets can be restored'}
                  onClick={() =>
                    confirmThen('Restore this version? The current file is archived first.', () => run('restore', async () => {
                      await runCommand({ type: 'restore_config_history', hostname, id: picked.id })
                      onRestored()
                    }))
                  }
                >
                  <RotateCcw /> Restore this version
                </Button>
              </div>
              <DiffView
                original={old}
                modified={current}
                language={language}
                height="60vh"
                originalLabel={`Archived · ${new Date(picked.timestamp_ms).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })}`}
                modifiedLabel="Current file (with unsaved edits)"
                onChanges={setChanges}
              />
            </>
          )}
        </div>
      </div>
    </div>
  )
}

/** Versions arrive newest first; keep that order within "Today", "Yesterday", dates. */
function groupByDay(versions: ConfigVersion[]): [string, ConfigVersion[]][] {
  const today = new Date()
  const yesterday = new Date(today.getTime() - 86_400_000)
  const label = (d: Date) =>
    d.toDateString() === today.toDateString()
      ? 'Today'
      : d.toDateString() === yesterday.toDateString()
        ? 'Yesterday'
        : d.toLocaleDateString([], { dateStyle: 'medium' })
  const groups = new Map<string, ConfigVersion[]>()
  for (const v of versions) {
    const key = label(new Date(v.timestamp_ms))
    groups.set(key, [...(groups.get(key) ?? []), v])
  }
  return [...groups]
}

function formatSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`
}

/** §23: the common blocks as forms; raw editing stays in the Editor tab. */
function StructuredEditor({ hostname, onApplied }: { hostname: string; onApplied: () => void }) {
  const [domain, setDomain] = useState<Domain | null>(null)
  const { busy, error, setError, run } = useAction()
  const [done, setDone] = useState<string | null>(null)

  useEffect(() => {
    runCommand({ type: 'get_domain', hostname }).then((r) => r.type === 'domain' && setDomain(r.domain))
  }, [hostname])
  if (!domain) return null
  const b = domain.blocks
  const setBlocks = (patch: Partial<SiteBlocks>) => setDomain({ ...domain, blocks: { ...b, ...patch } })
  const manual = domain.ownership === 'manual'

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {manual && (
        <p className="rounded-lg border border-warning/40 bg-warning/5 p-3 text-xs">
          This site is Manual, so these settings are stored but not written to its file. Switch it to Managed or Advanced to use them.
        </p>
      )}

      <Section title="Response headers" onAdd={() => setBlocks({ headers: [...b.headers, { name: '', value: '' }] })}>
        {b.headers.map((h, i) => (
          <Row key={i} onRemove={() => setBlocks({ headers: b.headers.filter((_, j) => j !== i) })}>
            <Input placeholder="X-Frame-Options" value={h.name} onChange={(e) => setBlocks({ headers: b.headers.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)) })} />
            <Input placeholder="DENY" value={h.value} onChange={(e) => setBlocks({ headers: b.headers.map((x, j) => (j === i ? { ...x, value: e.target.value } : x)) })} />
          </Row>
        ))}
      </Section>

      <Section title="Redirects" onAdd={() => setBlocks({ redirects: [...b.redirects, { from: '/old', to: '/new', code: 301 }] })}>
        {b.redirects.map((r, i) => (
          <Row key={i} onRemove={() => setBlocks({ redirects: b.redirects.filter((_, j) => j !== i) })}>
            <Input placeholder="/from" value={r.from} onChange={(e) => setBlocks({ redirects: b.redirects.map((x, j) => (j === i ? { ...x, from: e.target.value } : x)) })} />
            <Input placeholder="/to or https://…" value={r.to} onChange={(e) => setBlocks({ redirects: b.redirects.map((x, j) => (j === i ? { ...x, to: e.target.value } : x)) })} />
            <Select className="w-24" value={r.code} onChange={(e) => setBlocks({ redirects: b.redirects.map((x, j) => (j === i ? { ...x, code: Number(e.target.value) } : x)) })}>
              {[301, 302, 303, 307, 308].map((c) => (
                <option key={c}>{c}</option>
              ))}
            </Select>
          </Row>
        ))}
      </Section>

      <Section title="Reverse-proxy mappings" hint="Send a path on this site to another server, e.g. /api → http://127.0.0.1:8000 (§30)" onAdd={() => setBlocks({ mappings: [...b.mappings, { path: '/api/', upstream: 'http://127.0.0.1:8000' }] })}>
        {b.mappings.map((m, i) => (
          <Row key={i} onRemove={() => setBlocks({ mappings: b.mappings.filter((_, j) => j !== i) })}>
            <Input placeholder="/api/" value={m.path} onChange={(e) => setBlocks({ mappings: b.mappings.map((x, j) => (j === i ? { ...x, path: e.target.value } : x)) })} />
            <Input placeholder="http://127.0.0.1:8000" value={m.upstream} onChange={(e) => setBlocks({ mappings: b.mappings.map((x, j) => (j === i ? { ...x, upstream: e.target.value } : x)) })} />
          </Row>
        ))}
      </Section>

      <Section title="Upstream groups (Nginx)" hint="Named server groups you can proxy_pass to from your own snippet" onAdd={() => setBlocks({ upstreams: [...b.upstreams, { name: 'backend', servers: ['127.0.0.1:9000'] }] })}>
        {b.upstreams.map((u, i) => (
          <Row key={i} onRemove={() => setBlocks({ upstreams: b.upstreams.filter((_, j) => j !== i) })}>
            <Input placeholder="name" value={u.name} onChange={(e) => setBlocks({ upstreams: b.upstreams.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)) })} />
            <Input placeholder="127.0.0.1:9000, 127.0.0.1:9001" value={u.servers.join(', ')} onChange={(e) => setBlocks({ upstreams: b.upstreams.map((x, j) => (j === i ? { ...x, servers: e.target.value.split(',').map((s) => s.trim()).filter(Boolean) } : x)) })} />
          </Row>
        ))}
      </Section>

      <Section title="Includes" hint="Extra config files pulled into this site" onAdd={() => setBlocks({ includes: [...b.includes, ''] })}>
        {b.includes.map((inc, i) => (
          <Row key={i} onRemove={() => setBlocks({ includes: b.includes.filter((_, j) => j !== i) })}>
            <Input placeholder="C:\path\to\extra.conf" value={inc} onChange={(e) => setBlocks({ includes: b.includes.map((x, j) => (j === i ? e.target.value : x)) })} />
          </Row>
        ))}
      </Section>

      <div className="flex items-center gap-3">
        <Button
          disabled={busy !== null}
          onClick={() =>
            run('save-blocks', async () => {
              setDone(null)
              await runCommand({ type: 'update_domain', domain })
              const r = await runCommand({ type: 'apply_web', overwrite: [] })
              if (r.type === 'applied') setDone(r.report.drifted.length ? 'Saved, but the file was edited by hand and was left alone.' : 'Saved and applied. The server accepted the config.')
              onApplied()
            })
          }
        >
          Save and apply
        </Button>
        {done && <span className="text-sm text-muted-foreground">{done}</span>}
      </div>
      <Field label="Field values">
        <CardDescription>Values can't contain quotes, semicolons, braces or line breaks, so they can't break out of the config syntax.</CardDescription>
      </Field>
    </div>
  )
}

function Section({ title, hint, onAdd, children }: { title: string; hint?: string; onAdd: () => void; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-sm">{title}</CardTitle>
          {hint && <CardDescription>{hint}</CardDescription>}
        </div>
        <Button size="sm" variant="secondary" onClick={onAdd}>
          <Plus /> Add
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">{children}</CardContent>
    </Card>
  )
}

function Row({ children, onRemove }: { children: React.ReactNode; onRemove: () => void }) {
  return (
    <div className="flex items-center gap-2">
      {children}
      <Button size="sm" variant="ghost" onClick={() => confirmThen('Remove this entry? It takes effect when you save.', () => onRemove())} title="Remove">
        <Trash2 className="size-3.5" />
      </Button>
    </div>
  )
}
