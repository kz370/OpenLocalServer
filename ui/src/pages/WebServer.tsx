import { Lock, Play, RefreshCw, ShieldCheck, Trash2, Check } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechTile } from '@/components/TechIcon'
import { ApplyReportCard, DriftDialog } from '@/components/site/WebApply'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CertInfo, type WebConfig, type WebStatus, runCommand } from '@/core'
import { confirmThen } from '@/lib/confirm'
import { useWeb } from '@/lib/web'
import { waitForWebStopped } from '@/lib/wait'
import { ConfigPage } from '@/pages/Config'

export function WebServerPage({ initialTab = 'server' }: { initialTab?: 'server' | 'config' | 'certs' }) {
  const web = useWeb()
  const { status, cfg, certs, ca, projects, busy, error, setError, run, refresh, apply } = web
  const [tab, setTab] = useState<'server' | 'config' | 'certs'>(initialTab)
  const [certDetail, setCertDetail] = useState<CertInfo | null>(null)

  async function saveSettings(next: WebConfig, serverChanged: boolean) {
    await runCommand({ type: 'set_setting', key: 'web.default_server', value: next.default_server })
    for (const id of ['nginx', 'apache', 'caddy']) {
      const ports = next.servers[id]
      if (!ports) continue
      await runCommand({ type: 'set_setting', key: `web.servers.${id}.http_port`, value: ports.http })
      await runCommand({ type: 'set_setting', key: `web.servers.${id}.https_port`, value: ports.https })
    }
    await runCommand({ type: 'set_setting', key: 'web.php_workers', value: next.php_workers })
    await runCommand({ type: 'set_setting', key: 'web.dns_port', value: next.dns_port })
    if (serverChanged || status?.running) await apply()
    else await refresh()
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Web server</h1>
          <p className="text-sm text-muted-foreground">
            The server that serves your sites, its ports, and the local HTTPS certificates. Sites themselves live on the Sites page (§44–53).
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

      <ApplyReportCard web={web} />

      <Tabs
        tabs={[
          { id: 'server', label: 'Server' },
          { id: 'config', label: 'Web config' },
          { id: 'certs', label: 'Certificates', badge: certs.length },
        ]}
        value={tab}
        onChange={setTab}
      />

      {tab === 'server' && cfg && status && <ServerPanel cfg={cfg} status={status} onSave={(c, changed) => run('settings', () => saveSettings(c, changed))} busy={busy !== null} />}

      {tab === 'config' && <ConfigPage />}

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
                        <div className="flex flex-wrap justify-end gap-0.5">
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

      <DriftDialog web={web} />

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
  const setPorts = (id: string, patch: Partial<ServerPorts>) =>
    setDraft({
      ...draft,
      servers: { ...draft.servers, [id]: { ...(draft.servers[id] ?? { http: 80, https: 443 }), ...patch } },
    })
  const defaultChanged = draft.default_server !== cfg.default_server

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
          <div>
            <CardTitle className="text-sm">Default web server</CardTitle>
            <CardDescription>
              The default server always binds ports 80 and 443 and serves every site that hasn't picked one for itself. The other
              servers run at the same time on their own ports.
            </CardDescription>
          </div>
          {status.running ? <Badge variant="success">● Running</Badge> : <Badge variant="secondary">Stopped</Badge>}
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label="Default web server">
            {status.servers.map((s) => {
              const selected = draft.default_server === s.id
              return (
                <button
                  key={s.id}
                  role="radio"
                  aria-checked={selected}
                  disabled={!s.installed}
                  title={s.installed ? undefined : 'Install it from the Runtimes page first'}
                  onClick={() => setDraft({ ...draft, default_server: s.id })}
                  className={`relative flex items-center gap-3 rounded-xl border p-3 text-left transition-all disabled:cursor-not-allowed disabled:opacity-50 ${
                    selected ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-foreground/20 hover:bg-accent/50'
                  }`}
                >
                  <TechTile id={s.id} />
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-1.5 text-sm font-medium">
                      {s.name}
                      {s.running && <span className="size-1.5 rounded-full bg-emerald-500" title="Running" />}
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
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="PHP workers per version">
              <Input type="number" min={1} max={16} value={draft.php_workers} onChange={(e) => setDraft({ ...draft, php_workers: num(e.target.value, 3) })} />
            </Field>
            <Field label="Wildcard DNS port" hint="Windows only routes DNS rules to port 53">
              <Input type="number" value={draft.dns_port} onChange={(e) => setDraft({ ...draft, dns_port: num(e.target.value, 53) })} />
            </Field>
          </div>
          {dirty && (
            <div>
              <Button size="sm" disabled={busy} onClick={() => onSave(draft, defaultChanged)}>
                {defaultChanged ? 'Switch default and apply' : 'Save settings'}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Servers and ports</CardTitle>
          <CardDescription>
            Each server starts and stops on its own from the Services page. A site is rendered on exactly one server — the one it
            picks in its own settings, or the default.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          {status.servers.map((s) => {
            const isDefault = s.id === draft.default_server
            const ports = draft.servers[s.id] ?? { http: 80, https: 443 }
            return (
              <div key={s.id} className="grid items-end gap-3 rounded-lg border p-3 sm:grid-cols-[1fr_8rem_8rem_7rem]">
                <div className="flex items-center gap-2">
                  <TechTile id={s.id} />
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 text-sm font-medium">
                      {s.name}
                      {s.running && <span className="size-1.5 rounded-full bg-emerald-500" title="Running" />}
                      {isDefault && <Badge variant="outline">Default</Badge>}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">
                      {s.installed ? `Binds ${s.http_port} / ${s.https_port} right now` : 'Not installed'}
                    </div>
                  </div>
                </div>
                <Field label="HTTP port">
                  <Input
                    type="number"
                    disabled={isDefault}
                    title={isDefault ? 'The default server always binds port 80' : undefined}
                    value={isDefault ? 80 : ports.http}
                    onChange={(e) => setPorts(s.id, { http: num(e.target.value, 80) })}
                  />
                </Field>
                <Field label="HTTPS port">
                  <Input
                    type="number"
                    disabled={isDefault}
                    title={isDefault ? 'The default server always binds port 443' : undefined}
                    value={isDefault ? 443 : ports.https}
                    onChange={(e) => setPorts(s.id, { https: num(e.target.value, 443) })}
                  />
                </Field>
                <div className="pb-2 text-xs text-muted-foreground">
                  {isDefault ? 'Locked: 80/443' : `Sites: ${domainsOn(status, s.id)}`}
                </div>
              </div>
            )
          })}
          {dirty && (
            <div>
              <Button size="sm" disabled={busy} onClick={() => onSave(draft, defaultChanged)}>
                {defaultChanged ? 'Switch default and apply' : 'Save settings'}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

/** How many sites are currently rendered on a server — the rest moved elsewhere. */
function domainsOn(status: WebStatus, id: string): number {
  return status.servers.find((s) => s.id === id)?.active ? 0 : 0
}

const SERVER_BLURB: Record<string, string> = {
  nginx: 'Fast and light · default',
  apache: 'Honours .htaccess files',
  caddy: 'Simple, HTTPS-first',
}
