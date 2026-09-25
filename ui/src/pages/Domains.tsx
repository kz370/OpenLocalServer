import { open } from '@tauri-apps/plugin-dialog'
import {
  Activity,
  Check,
  Code2,
  Copy,
  ExternalLink,
  FileCode2,
  FolderSearch,
  Lock,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechTile } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  type ApplyReport,
  type CaInfo,
  type CatalogEntry,
  type CertInfo,
  type Domain,
  type DomainSummary,
  type HealthReport,
  type Project,
  type WebConfig,
  type WebStatus,
  runCommand,
} from '@/core'
import { useAction, usePoll } from '@/lib/hooks'
import { waitForWebStopped } from '@/lib/wait'
import { confirmAction, confirmThen } from '@/lib/confirm'

const emptyBlocks = { headers: [], redirects: [], mappings: [], upstreams: [], includes: [] }

export function splitArgs(line: string): string[] {
  const out: string[] = []
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(line))) out.push(m[1] ?? m[2] ?? m[3])
  return out
}

function newDomain(): Domain {
  return {
    hostname: '',
    project_id: null,
    root: '',
    kind: { type: 'static' },
    https: true,
    redirect_https: true,
    wildcard: false,
    enabled: true,
    ownership: 'managed',
    app: null,
    blocks: emptyBlocks,
    generated_hashes: {},
  }
}

export function DomainsPage() {
  const [tab, setTab] = useState<'sites' | 'certs'>('sites')
  const [status, setStatus] = useState<WebStatus | null>(null)
  const [cfg, setCfg] = useState<WebConfig | null>(null)
  const [domains, setDomains] = useState<DomainSummary[]>([])
  const [certs, setCerts] = useState<CertInfo[]>([])
  const [ca, setCa] = useState<CaInfo | null>(null)
  const [projects, setProjects] = useState<Project[]>([])
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [customPhp, setCustomPhp] = useState<string[]>([])
  const { busy, error, setError, run } = useAction()

  const [report, setReport] = useState<ApplyReport | null>(null)
  const [driftOpen, setDriftOpen] = useState(false)
  const [editing, setEditing] = useState<Domain | null>(null)
  const [editingIsNew, setEditingIsNew] = useState(false)
  const [health, setHealth] = useState<HealthReport | null>(null)
  const [certDetail, setCertDetail] = useState<CertInfo | null>(null)
  const [dupOf, setDupOf] = useState<string | null>(null)
  const [dupName, setDupName] = useState('')

  async function refresh() {
    const [s, c, d, k, a] = await Promise.all([
      runCommand({ type: 'get_web_status' }),
      runCommand({ type: 'get_web_config' }),
      runCommand({ type: 'list_domains' }),
      runCommand({ type: 'list_certificates' }),
      runCommand({ type: 'get_ca_info' }),
    ])
    if (s.type === 'web_status') setStatus(s.status)
    if (c.type === 'web_config') setCfg((prev) => prev ?? c.config)
    if (d.type === 'domains') setDomains(d.domains)
    if (k.type === 'certificates') setCerts(k.certs)
    if (a.type === 'ca_info') setCa(a.info)
  }

  usePoll(() => refresh().catch(() => undefined), 4000)
  useEffect(() => {
    runCommand({ type: 'list_projects' }).then((r) => r.type === 'projects' && setProjects(r.projects))
    // Laragon-style: new folders in your projects folder show up as <name>.test.
    runCommand({ type: 'sync_auto_domains' })
      .then((r) => {
        if (r.type === 'count' && r.count > 0) void refresh()
      })
      .catch(() => undefined)
    runCommand({ type: 'list_runtime_catalog' }).then((r) => r.type === 'runtime_catalog' && setCatalog(r.entries))
    // PHP versions registered from elsewhere (Laragon, XAMPP, ...) serve sites too.
    runCommand({ type: 'list_custom_installs' }).then(
      (r) => r.type === 'custom_installs' && setCustomPhp(r.entries.filter((c) => c.id === 'php' && c.label).map((c) => c.label)),
    )
  }, [])

  async function apply(overwrite: string[] = []) {
    const res = await runCommand({ type: 'apply_web', overwrite })
    if (res.type === 'applied') {
      setReport(res.report)
      if (res.report.drifted.length > 0) setDriftOpen(true)
    }
    await refresh()
  }

  async function saveSettings(next: WebConfig, serverChanged: boolean) {
    await runCommand({ type: 'set_setting', key: 'web.server', value: next.server })
    await runCommand({ type: 'set_setting', key: 'web.http_port', value: next.http_port })
    await runCommand({ type: 'set_setting', key: 'web.https_port', value: next.https_port })
    await runCommand({ type: 'set_setting', key: 'web.php_workers', value: next.php_workers })
    await runCommand({ type: 'set_setting', key: 'web.dns_port', value: next.dns_port })
    if (serverChanged || status?.running) await apply()
    else await refresh()
  }

  async function openEditor(hostname: string | null) {
    if (hostname === null) {
      setEditing(newDomain())
      setEditingIsNew(true)
      return
    }
    const res = await runCommand({ type: 'get_domain', hostname })
    if (res.type === 'domain') {
      setEditing(res.domain)
      setEditingIsNew(false)
    }
  }

  async function saveDomain(d: Domain) {
    // Renaming first, so the update below finds the site under its new name.
    if (!editingIsNew && editing && editing.hostname !== d.hostname) {
      await runCommand({ type: 'rename_domain', hostname: editing.hostname, new_hostname: d.hostname })
    }
    await runCommand({ type: editingIsNew ? 'add_domain' : 'update_domain', domain: d })
    setEditing(null)
    await apply()
  }

  const installedPhp = [...new Set([...catalog.filter((c) => c.id === 'php' && c.installed).map((c) => c.version), ...customPhp])]

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Domains &amp; HTTPS</h1>
          <p className="text-sm text-muted-foreground">
            Local domains, trusted certificates and the web server that serves them (§44–53).
          </p>
        </div>
        <div className="flex gap-2">
          <Button disabled={busy !== null} onClick={() => run('apply', () => apply())}>
            {busy === 'apply' ? <Spinner /> : <Play />} {busy === 'apply' ? (status?.running ? 'Applying…' : 'Starting…') : status?.running ? 'Apply changes' : 'Start web server'}
          </Button>
          {status?.running && (
            <Button
              variant="outline"
              disabled={busy !== null}
              onClick={() =>
                run('stop', async () => {
                  await runCommand({ type: 'stop_web' })
                  await waitForWebStopped()
                  await refresh()
                })
              }
            >
              {busy === 'stop' ? <Spinner /> : <StopIcon />} {busy === 'stop' ? 'Stopping…' : 'Stop'}
            </Button>
          )}
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {status && status.port_conflicts.length > 0 && !status.running && (
        <Card className="border-warning/40">
          <CardContent className="pt-4 text-sm">
            {status.port_conflicts.map((c) => (
              <div key={c}>{c}. Stop that program or pick other ports below.</div>
            ))}
          </CardContent>
        </Card>
      )}

      {report && (
        <Card className="border-success/40">
          <CardHeader className="flex-row items-start justify-between space-y-0 pb-2">
            <CardTitle className="text-sm">Applied to {report.server}</CardTitle>
            <button className="text-xs text-muted-foreground" onClick={() => setReport(null)}>
              Dismiss
            </button>
          </CardHeader>
          <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
            <div>
              {report.started ? 'Server started. ' : report.reloaded ? 'Server reloaded. ' : ''}
              {report.written.length > 0 ? `Updated: ${report.written.join(', ')}. ` : 'Config already up to date. '}
              {report.hosts_updated && 'Hosts file updated.'}
            </div>
            {report.warnings.map((w) => (
              <div key={w} className="text-warning">
                {w}
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      {cfg && status && (
        <ServerPanel cfg={cfg} status={status} onSave={(c, changed) => run('settings', () => saveSettings(c, changed))} busy={busy !== null} />
      )}

      <Tabs
        tabs={[
          { id: 'sites', label: 'Sites', badge: domains.length },
          { id: 'certs', label: 'Certificates', badge: certs.length },
        ]}
        value={tab}
        onChange={setTab}
      />

      {tab === 'sites' && (
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm">Sites</CardTitle>
            <Button size="sm" onClick={() => openEditor(null)}>
              <Plus /> Add domain
            </Button>
          </CardHeader>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Domain</TableHead>
                  <TableHead>Serves</TableHead>
                  <TableHead>HTTPS</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {domains.map((d) => (
                  <TableRow key={d.hostname}>
                    <TableCell className="font-medium">
                      {d.hostname}
                      {d.project_id && <span className="ml-2 text-xs text-muted-foreground">{projects.find((p) => p.id === d.project_id)?.name}</span>}
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {d.kind}
                      {d.has_app && ' + app'}
                    </TableCell>
                    <TableCell>{d.https ? <Badge variant="success">HTTPS</Badge> : <Badge variant="outline">HTTP</Badge>}</TableCell>
                    <TableCell>{d.enabled ? <Badge variant="secondary">enabled</Badge> : <Badge variant="warning">disabled</Badge>}</TableCell>
                    <TableCell>
                      <div className="flex justify-end gap-0.5">
                        <Button size="sm" variant="ghost" title="Open in browser" disabled={!d.enabled || !status?.running} onClick={() => run('open', () => runCommand({ type: 'open_url', url: d.url }))}>
                          <ExternalLink className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Health check (DNS → TCP → TLS → certificate → trust → HTTP)"
                          onClick={() =>
                            run('health', async () => {
                              const r = await runCommand({ type: 'health_check', hostname: d.hostname })
                              if (r.type === 'health') setHealth(r.report)
                            })
                          }
                        >
                          <Activity className="size-3.5" />
                        </Button>
                        <Button size="sm" variant="ghost" title={`Open ${d.folder} in your code editor`} onClick={() => run('code', () => runCommand({ type: 'open_in_editor', path: d.folder }))}>
                          <Code2 className="size-3.5" />
                        </Button>
                        <Button size="sm" variant="ghost" title="Edit" onClick={() => openEditor(d.hostname)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          title={d.enabled ? 'Disable' : 'Enable'}
                          onClick={() =>
                            run(`toggle:${d.hostname}`, async () => {
                              await runCommand({ type: 'set_domain_enabled', hostname: d.hostname, enabled: !d.enabled })
                              await apply()
                            })
                          }
                        >
                          {busy === `toggle:${d.hostname}` ? <Spinner className="size-3.5" /> : d.enabled ? <StopIcon className="size-3.5" /> : <Play className="size-3.5" />}
                        </Button>
                        {d.has_app && (
                          <Button size="sm" variant="ghost" title="Restart app process" onClick={() => run('restart', () => runCommand({ type: 'restart_site_app', hostname: d.hostname }))}>
                            <RefreshCw className="size-3.5" />
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Duplicate"
                          onClick={() => {
                            setDupOf(d.hostname)
                            setDupName(`copy.${d.hostname}`)
                          }}
                        >
                          <Copy className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          title="Delete"
                          onClick={async () => {
                            if (!(await confirmAction(`Delete ${d.hostname}? Its certificate is revoked and its config removed.`))) return
                            void run('delete', async () => {
                              await runCommand({ type: 'remove_domain', hostname: d.hostname })
                              await apply()
                            })
                          }}
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
                {domains.length === 0 && (
                  <TableRow>
                    <TableCell colSpan={5} className="text-center text-sm text-muted-foreground">
                      No domains yet. Add one, or create a project from Quick Apps.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {tab === 'certs' && (
        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0">
              <div>
                <CardTitle className="text-sm">Local certificate authority</CardTitle>
                <CardDescription>
                  {ca?.exists
                    ? ca.trusted
                      ? 'Trusted by Windows: Edge and Chrome accept your local HTTPS sites.'
                      : 'Created but not trusted yet. Browsers will warn until you trust it.'
                    : 'Created automatically the first time a site uses HTTPS.'}
                </CardDescription>
              </div>
              <div className="flex items-center gap-2">
                {ca?.trusted ? <Badge variant="success">● Trusted</Badge> : <Badge variant="warning">Not trusted</Badge>}
                {ca?.trusted ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy !== null}
                    onClick={() => confirmThen('Remove the CA from the Windows trust store? HTTPS sites will show warnings.', () => run('untrust', () => runCommand({ type: 'untrust_ca' }).then(refresh)))}
                  >
                    Untrust
                  </Button>
                ) : (
                  <Button size="sm" disabled={busy !== null} onClick={() => run('trust', () => runCommand({ type: 'trust_ca' }).then(refresh))}>
                    <ShieldCheck /> Trust CA
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent className="text-xs text-muted-foreground">
              Windows asks you to confirm before it adds a root certificate. The private key stays on this computer and is never shown or sent anywhere (§142).
            </CardContent>
          </Card>

          <Card>
            <CardContent className="p-0">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Domain</TableHead>
                    <TableHead>Covers</TableHead>
                    <TableHead>Expires</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="text-right">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {certs.map((c) => (
                    <TableRow key={c.hostname}>
                      <TableCell className="font-medium">{c.hostname}</TableCell>
                      <TableCell className="text-xs text-muted-foreground">{c.sans.join(', ')}</TableCell>
                      <TableCell className="text-muted-foreground">{new Date(c.expires_at * 1000).toLocaleDateString()} ({c.days_left}d)</TableCell>
                      <TableCell>
                        {c.status === 'valid' && <Badge variant="success">Valid</Badge>}
                        {c.status === 'expiring' && <Badge variant="warning">Expiring</Badge>}
                        {c.status === 'expired' && <Badge variant="destructive">Expired</Badge>}
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end gap-0.5">
                          <Button size="sm" variant="ghost" title="Details" onClick={() => setCertDetail(c)}>
                            <Lock className="size-3.5" />
                          </Button>
                          <Button size="sm" variant="ghost" title="Regenerate" disabled={busy !== null} onClick={() => run('regen', () => runCommand({ type: 'regenerate_certificate', hostname: c.hostname }).then(refresh))}>
                            <RefreshCw className="size-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            title="Revoke (delete)"
                            onClick={() => confirmThen(`Revoke the certificate for ${c.hostname}? The site can't use HTTPS until a new one is generated.`, () => run('revoke', () => runCommand({ type: 'revoke_certificate', hostname: c.hostname }).then(refresh)))}
                          >
                            <Trash2 className="size-3.5" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                  {certs.length === 0 && (
                    <TableRow>
                      <TableCell colSpan={5} className="text-center text-sm text-muted-foreground">
                        No certificates yet. They are generated when you apply a site with HTTPS on.
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </div>
      )}

      {/* §27 protection dialog */}
      <Dialog
        open={driftOpen && !!report && report.drifted.length > 0}
        onClose={() => setDriftOpen(false)}
        title="Config files were edited by hand"
        description="These sites' generated config no longer matches what OpenLocalServer wrote, so it left them alone."
        footer={
          <>
            <Button variant="ghost" onClick={() => setDriftOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="secondary"
              onClick={() =>
                run('keep', async () => {
                  for (const h of report?.drifted ?? []) await runCommand({ type: 'set_ownership', hostname: h, ownership: 'manual' })
                  setDriftOpen(false)
                  await apply()
                })
              }
            >
              Keep my edits (switch to Manual)
            </Button>
            <Button
              onClick={() =>
                run('overwrite', async () => {
                  setDriftOpen(false)
                  await apply(report?.drifted ?? [])
                })
              }
            >
              Overwrite (a copy stays in history)
            </Button>
          </>
        }
      >
        <ul className="list-disc pl-5 text-sm">
          {report?.drifted.map((h) => (
            <li key={h}>{h}</li>
          ))}
        </ul>
      </Dialog>

      <DomainDialog
        domain={editing}
        isNew={editingIsNew}
        projects={projects}
        installedPhp={installedPhp}
        onClose={() => setEditing(null)}
        onSave={(d) => run('save', () => saveDomain(d))}
        busy={busy !== null}
      />

      <Dialog open={!!health} onClose={() => setHealth(null)} title={`Health: ${health?.hostname ?? ''}`} description={health?.ok ? 'Every link in the chain works.' : 'Something in the chain is broken. The first failing step is the one to fix.'}>
        <div className="flex flex-col gap-2">
          {health?.steps.map((s) => (
            <div key={s.name} className="flex items-start gap-2 text-sm">
              <span className={s.skipped ? 'text-muted-foreground' : s.ok ? 'text-success' : 'text-destructive'}>{s.skipped ? '–' : s.ok ? '✓' : '✗'}</span>
              <div>
                <span className="font-medium">{s.name}</span>
                <div className="text-xs text-muted-foreground">{s.detail}</div>
              </div>
            </div>
          ))}
        </div>
      </Dialog>

      <Dialog open={!!certDetail} onClose={() => setCertDetail(null)} title={`Certificate: ${certDetail?.hostname ?? ''}`}>
        {certDetail && (
          <dl className="grid grid-cols-[8rem_1fr] gap-x-3 gap-y-2 text-sm">
            <dt className="text-muted-foreground">Issuer</dt>
            <dd>{certDetail.issuer}</dd>
            <dt className="text-muted-foreground">Domains</dt>
            <dd>{certDetail.sans.join(', ')}</dd>
            <dt className="text-muted-foreground">Issued</dt>
            <dd>{new Date(certDetail.issued_at * 1000).toLocaleString()}</dd>
            <dt className="text-muted-foreground">Expires</dt>
            <dd>
              {new Date(certDetail.expires_at * 1000).toLocaleString()} ({certDetail.days_left} days)
            </dd>
            <dt className="text-muted-foreground">Trust</dt>
            <dd>{certDetail.trusted ? 'Trusted (issuing CA is in the Windows store)' : 'Not trusted'}</dd>
            <dt className="text-muted-foreground">Project</dt>
            <dd>{projects.find((p) => p.id === certDetail.project_id)?.name ?? '—'}</dd>
            <dt className="text-muted-foreground">Certificate</dt>
            <dd className="break-all font-mono text-xs">{certDetail.cert_path}</dd>
            <dt className="text-muted-foreground">Key file</dt>
            <dd className="break-all font-mono text-xs">{certDetail.key_path} <span className="text-muted-foreground">(never displayed)</span></dd>
          </dl>
        )}
      </Dialog>

      <Dialog
        open={dupOf !== null}
        onClose={() => setDupOf(null)}
        title={`Duplicate ${dupOf ?? ''}`}
        description="The copy gets the same settings, but its own hostname (and no app process)."
        footer={
          <>
            <Button variant="ghost" onClick={() => setDupOf(null)}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null}
              onClick={() =>
                run('dup', async () => {
                  await runCommand({ type: 'duplicate_domain', hostname: dupOf!, new_hostname: dupName })
                  setDupOf(null)
                  await apply()
                })
              }
            >
              Duplicate
            </Button>
          </>
        }
      >
        <Field label="New domain">
          <Input value={dupName} onChange={(e) => setDupName(e.target.value)} />
        </Field>
      </Dialog>
    </div>
  )
}

function ServerPanel({
  cfg,
  status,
  onSave,
  busy,
}: {
  cfg: WebConfig
  status: WebStatus
  onSave: (c: WebConfig, serverChanged: boolean) => void
  busy: boolean
}) {
  const [draft, setDraft] = useState(cfg)
  useEffect(() => setDraft(cfg), [cfg])
  const dirty = JSON.stringify(draft) !== JSON.stringify(cfg)
  const num = (v: string, fallback: number) => (Number.isFinite(Number(v)) && v !== '' ? Number(v) : fallback)

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-sm">Web server</CardTitle>
          <CardDescription>One server at a time serves your sites; switching regenerates every site's config.</CardDescription>
        </div>
        {status.running ? <Badge variant="success">● Running</Badge> : <Badge variant="secondary">Stopped</Badge>}
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Web server">
          {status.servers.map((s) => {
            const selected = draft.server === s.id
            const live = s.id === cfg.server && status.running
            return (
              <button
                key={s.id}
                role="radio"
                aria-checked={selected}
                disabled={!s.installed}
                title={s.installed ? undefined : 'Install it from the Runtimes page first'}
                onClick={() => setDraft({ ...draft, server: s.id })}
                className={`relative flex items-center gap-3 rounded-xl border p-3 text-left transition-all disabled:cursor-not-allowed disabled:opacity-50 ${
                  selected ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-foreground/20 hover:bg-accent/50'
                }`}
              >
                <TechTile id={s.id} />
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5 text-sm font-medium">
                    {s.name}
                    {live && <span className="size-1.5 rounded-full bg-emerald-500" title="Running" />}
                  </span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {s.installed ? SERVER_BLURB[s.id] : 'Not installed'}
                  </span>
                </span>
                {selected && (
                  <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground">
                    <Check className="size-3" />
                  </span>
                )}
              </button>
            )
          })}
        </div>
        <div className="grid gap-3 sm:grid-cols-4">
          <Field label="HTTP port">
            <Input type="number" value={draft.http_port} onChange={(e) => setDraft({ ...draft, http_port: num(e.target.value, 80) })} />
          </Field>
          <Field label="HTTPS port">
            <Input type="number" value={draft.https_port} onChange={(e) => setDraft({ ...draft, https_port: num(e.target.value, 443) })} />
          </Field>
          <Field label="PHP workers per version">
            <Input type="number" min={1} max={16} value={draft.php_workers} onChange={(e) => setDraft({ ...draft, php_workers: num(e.target.value, 3) })} />
          </Field>
          <Field label="Wildcard DNS port" hint="Windows only routes DNS rules to port 53">
            <Input type="number" value={draft.dns_port} onChange={(e) => setDraft({ ...draft, dns_port: num(e.target.value, 53) })} />
          </Field>
        </div>
        {dirty && (
          <div>
            <Button size="sm" disabled={busy} onClick={() => onSave(draft, draft.server !== cfg.server)}>
              {draft.server !== cfg.server ? 'Switch server and apply' : 'Save settings'}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function DomainDialog({
  domain,
  isNew,
  projects,
  installedPhp,
  onClose,
  onSave,
  busy,
}: {
  domain: Domain | null
  isNew: boolean
  projects: Project[]
  installedPhp: string[]
  onClose: () => void
  onSave: (d: Domain) => void
  busy: boolean
}) {
  const [d, setD] = useState<Domain | null>(domain)
  const [template, setTemplate] = useState('{project}.test')
  const [appLine, setAppLine] = useState('')
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    setD(domain)
    setErr(null)
    setAppLine(domain?.app ? [domain.app.executable, ...domain.app.args].join(' ') : '')
  }, [domain])
  if (!d) return <Dialog open={false} onClose={onClose} title="" children={null} />

  const set = (patch: Partial<Domain>) => setD({ ...d, ...patch })
  const kindType = d.kind.type

  async function pickProject(id: string) {
    const p = projects.find((x) => x.id === id)
    if (!p) return set({ project_id: null })
    let root = p.path
    const detail = await runCommand({ type: 'get_project_detail', id })
    let kind = d!.kind
    if (detail.type === 'project_detail') {
      const fw = detail.detail.detection.framework
      const sub = detail.detail.detection.doc_root ?? (fw === 'laravel' || fw === 'symfony' ? 'public' : null)
      if (sub) root = `${p.path}\\${sub}`
      if (fw === 'laravel' || fw === 'symfony' || fw === 'word_press' || fw === 'generic_php') kind = { type: 'php', version: null }
      else if (fw === 'node' || fw === 'fast_api' || fw === 'django' || fw === 'flask') kind = { type: 'proxy', upstream_port: 3000 }
    }
    const suggested = await runCommand({ type: 'suggest_domain', project_id: id, template })
    setD((cur) => ({ ...cur!, project_id: id, root, kind, hostname: cur!.hostname || (suggested.type === 'text' ? suggested.text : '') }))
  }

  async function browseRoot() {
    const picked = await open({ directory: true, title: 'Document root' })
    if (picked && !Array.isArray(picked)) set({ root: picked })
  }

  function submit() {
    if (!d) return
    const out = { ...d, hostname: d.hostname.trim().toLowerCase() }
    if (out.kind.type === 'proxy' && appLine.trim()) {
      const [executable, ...args] = splitArgs(appLine)
      out.app = { executable, args, cwd: out.app?.cwd || out.root, runtime: out.app?.runtime ?? (executable === 'node' || executable === 'npm' ? 'node' : null) }
    } else {
      out.app = null
    }
    if (!out.https) out.redirect_https = false
    if (!out.hostname) return setErr('Enter a domain like shop.test')
    if (!out.root) return setErr('Choose the site folder')
    onSave(out)
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title={isNew ? 'Add domain' : `Edit ${d.hostname}`}
      description="Changes are applied to the web server as soon as you save."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={submit}>
            {isNew ? 'Add and apply' : 'Save and apply'}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {err && <p className="text-sm text-destructive">{err}</p>}
        {isNew && (
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Project (optional)">
              <Select value={d.project_id ?? ''} onChange={(e) => pickProject(e.target.value)}>
                <option value="">— none —</option>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Name template (§48)">
              <Select value={template} onChange={(e) => setTemplate(e.target.value)}>
                <option>{'{project}.test'}</option>
                <option>{'api.{project}.test'}</option>
                <option>{'admin.{project}.test'}</option>
              </Select>
            </Field>
          </div>
        )}
        <Field label="Domain" hint="Subdomains work too: api.shop.test routes independently of shop.test">
          <Input value={d.hostname} onChange={(e) => set({ hostname: e.target.value })} placeholder="shop.test, myapp.local, api.company.dev…" />
        </Field>
        <div className="flex gap-2">
          <Field label="Site folder">
            <div className="flex gap-2">
              <Input value={d.root} onChange={(e) => set({ root: e.target.value })} placeholder="C:\Sites\shop\public" className="w-96" />
              <Button variant="secondary" onClick={browseRoot}>
                <FolderSearch /> Browse
              </Button>
            </div>
          </Field>
        </div>
        <Field label="Serves">
          <Select
            value={kindType}
            onChange={(e) => {
              const t = e.target.value
              set({ kind: t === 'php' ? { type: 'php', version: null } : t === 'proxy' ? { type: 'proxy', upstream_port: 3000 } : { type: 'static' } })
            }}
          >
            <option value="php">PHP (FastCGI)</option>
            <option value="proxy">Reverse proxy (dev server, Docker, another computer…)</option>
            <option value="static">Static files</option>
          </Select>
        </Field>
        {d.kind.type === 'php' && (
          <Field label="PHP version" hint="“Project default” follows the project's own resolved version">
            <Select value={d.kind.version ?? ''} onChange={(e) => set({ kind: { type: 'php', version: e.target.value || null } })}>
              <option value="">Project default / newest</option>
              {phpOptions(installedPhp, d.kind.version).map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </Select>
          </Field>
        )}
        {d.kind.type === 'proxy' && (
          <>
            <div className="grid gap-3 sm:grid-cols-[1fr_8rem]">
              <Field label="Forward to" hint="Blank = this computer. Or a Docker host, another PC's IP, a hostname.">
                <Input
                  value={d.kind.upstream_host ?? ''}
                  onChange={(e) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000, upstream_host: e.target.value.trim() || null } })}
                  placeholder="127.0.0.1"
                />
              </Field>
              <Field label="Port">
                <Input
                  type="number"
                  value={d.kind.upstream_port}
                  onChange={(e) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: Number(e.target.value) } })}
                />
              </Field>
            </div>
            <Toggle
              checked={!!d.kind.upstream_https}
              onChange={(v) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000, upstream_https: v } })}
              label="The target only speaks HTTPS"
              hint="Self-signed certificates on the target are accepted."
            />
            {!d.kind.upstream_host && (
              <Field label="Start command (optional)" hint="OpenLocalServer supervises it and passes PORT. Example: npm run dev">
                <Input value={appLine} onChange={(e) => setAppLine(e.target.value)} placeholder="npm run dev" />
              </Field>
            )}
          </>
        )}
        <div className="flex flex-col gap-2.5">
          <Toggle checked={d.https} onChange={(v) => set({ https: v, redirect_https: v ? d.redirect_https : false })} label="HTTPS" hint="A certificate from the local CA is created for this domain." />
          <Toggle checked={d.redirect_https} disabled={!d.https} onChange={(v) => set({ redirect_https: v })} label="Redirect HTTP to HTTPS" hint="Turn off for projects that need plain HTTP." />
          <Toggle checked={d.wildcard} onChange={(v) => set({ wildcard: v })} label={`Wildcard (*.${d.hostname || 'domain'})`} hint="Answers every subdomain; needs a one-time Windows DNS rule (you'll be asked to approve it)." />
        </div>
        {!isNew && (
          <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <FileCode2 className="size-3.5" /> Config ownership: <b>{d.ownership}</b>. Change it on the Web config page.
          </p>
        )}
      </div>
    </Dialog>
  )
}

/**
 * One "PHP 8.3" choice per minor version (the server picks the newest 8.3.x), plus each
 * exact version when several share a minor, so a specific build can be pinned.
 */
function phpOptions(installed: string[], current: string | null): { value: string; label: string }[] {
  const key = (v: string) => v.split('.').map((p) => parseInt(p, 10) || 0)
  const desc = (a: string, b: string) => {
    const [ka, kb] = [key(a), key(b)]
    for (let i = 0; i < Math.max(ka.length, kb.length); i++) if ((kb[i] ?? 0) !== (ka[i] ?? 0)) return (kb[i] ?? 0) - (ka[i] ?? 0)
    return 0
  }
  const byMinor = new Map<string, string[]>()
  for (const v of [...installed].sort(desc)) {
    const minor = v.split('.').slice(0, 2).join('.')
    byMinor.set(minor, [...(byMinor.get(minor) ?? []), v])
  }
  const options: { value: string; label: string }[] = []
  for (const [minor, versions] of byMinor) {
    options.push({ value: minor, label: versions.length > 1 ? `PHP ${minor} (newest: ${versions[0]})` : `PHP ${minor} (${versions[0]})` })
    if (versions.length > 1) for (const v of versions) options.push({ value: v, label: `  PHP ${v} exactly` })
  }
  // A saved version that's no longer installed still shows, instead of silently blanking.
  if (current && !options.some((o) => o.value === current)) options.push({ value: current, label: `PHP ${current} (not installed)` })
  return options
}

const SERVER_BLURB: Record<string, string> = {
  nginx: 'Fast and light · default',
  apache: 'Honours .htaccess files',
  caddy: 'Simple, HTTPS-first',
}
