import { AlertTriangle, Check, Lock, Play, RefreshCw, RotateCcw, ShieldCheck, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

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
import { NumberInput } from '@/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type CertInfo, type DomainSummary, type ServerPorts, type WebConfig, type WebStatus, runCommand } from '@/core'
import { confirmAction, confirmThen } from '@/lib/confirm'
import { useWeb } from '@/lib/web'
import { waitForWebStopped } from '@/lib/wait'
import { ConfigPage } from '@/pages/Config'
import { cn } from '@/lib/utils'

/** Baseline configuration against which dirty state is computed */
function computeAppliedBaseline(cfg: WebConfig | null, status: WebStatus | null): WebConfig | null {
  if (!cfg) return null
  if (!status || !status.running) {
    return JSON.parse(JSON.stringify(cfg))
  }
  const servers: Record<string, ServerPorts> = {}
  for (const s of status.servers) {
    servers[s.id] = { http: s.http_port, https: s.https_port }
  }
  for (const [id, ports] of Object.entries(cfg.servers)) {
    if (!servers[id]) servers[id] = { ...ports }
  }

  return {
    default_server: status.default_server || cfg.default_server,
    php_workers: cfg.php_workers,
    dns_port: status.dns_running ? status.dns_port : cfg.dns_port,
    servers,
  }
}

/** Deep comparison between draft settings and applied baseline */
function computeServerChanges(current: WebConfig | null, base: WebConfig | null): string[] {
  if (!current || !base) return []
  const changes: string[] = []
  if (current.default_server !== base.default_server) {
    changes.push(`Default server: ${base.default_server} → ${current.default_server}`)
  }
  if (current.php_workers !== base.php_workers) {
    changes.push(`PHP workers: ${base.php_workers} → ${current.php_workers}`)
  }
  if (current.dns_port !== base.dns_port) {
    changes.push(`DNS port: ${base.dns_port} → ${current.dns_port}`)
  }
  const allServerIds = new Set([...Object.keys(base.servers), ...Object.keys(current.servers)])
  for (const id of allServerIds) {
    if (id === current.default_server) continue
    const cPorts = current.servers[id] ?? { http: 80, https: 443 }
    const bPorts = base.servers[id] ?? { http: 80, https: 443 }
    if (cPorts.http !== bPorts.http) {
      changes.push(`${id} HTTP port: ${bPorts.http} → ${cPorts.http}`)
    }
    if (cPorts.https !== bPorts.https) {
      changes.push(`${id} HTTPS port: ${bPorts.https} → ${cPorts.https}`)
    }
  }
  return changes
}

interface ValidationResult {
  error: string | null
  invalidFields: Set<string>
}

/** Validates inputs and port collisions */
function validateWebDraft(draft: WebConfig | null, status: WebStatus | null): ValidationResult {
  if (!draft) return { error: null, invalidFields: new Set() }
  const invalidFields = new Set<string>()

  // 1. PHP workers range (1-16)
  if (typeof draft.php_workers !== 'number' || isNaN(draft.php_workers) || draft.php_workers < 1 || draft.php_workers > 16) {
    invalidFields.add('php_workers')
    return {
      error: 'PHP workers per version must be between 1 and 16.',
      invalidFields,
    }
  }

  // 2. Wildcard DNS port (1-65535)
  if (typeof draft.dns_port !== 'number' || isNaN(draft.dns_port) || draft.dns_port < 1 || draft.dns_port > 65535) {
    invalidFields.add('dns_port')
    return {
      error: 'Wildcard DNS port must be between 1 and 65535.',
      invalidFields,
    }
  }

  // 3. Server ports range (1-65535)
  for (const [id, ports] of Object.entries(draft.servers)) {
    if (id === draft.default_server) continue
    if (typeof ports.http !== 'number' || isNaN(ports.http) || ports.http < 1 || ports.http > 65535) {
      invalidFields.add(`${id}:http`)
      return {
        error: `${id} HTTP port must be between 1 and 65535.`,
        invalidFields,
      }
    }
    if (typeof ports.https !== 'number' || isNaN(ports.https) || ports.https < 1 || ports.https > 65535) {
      invalidFields.add(`${id}:https`)
      return {
        error: `${id} HTTPS port must be between 1 and 65535.`,
        invalidFields,
      }
    }
  }

  // 4. Duplicate port clashes
  const clashes = portClashes(draft)
  if (clashes.length > 0) {
    for (const [id, ports] of Object.entries(draft.servers)) {
      if (id !== draft.default_server) {
        if (ports.http === 80 || ports.http === 443) invalidFields.add(`${id}:http`)
        if (ports.https === 80 || ports.https === 443) invalidFields.add(`${id}:https`)
      }
    }
    return {
      error: clashes[0],
      invalidFields,
    }
  }

  // 5. Port conflicts with other system processes
  if (status && status.port_conflicts.length > 0 && !status.running) {
    return {
      error: status.port_conflicts[0],
      invalidFields,
    }
  }

  return { error: null, invalidFields }
}

export function WebServerPage({
  initialTab = 'server',
  onTabChange,
}: {
  initialTab?: 'server' | 'config' | 'certs'
  onTabChange?: (tab: 'server' | 'config' | 'certs') => void
}) {
  const web = useWeb()
  const { status, cfg, certs, ca, projects, busy, error, setError, run, refresh, refreshConfig, apply } = web
  const [tab, setTab] = useState<'server' | 'config' | 'certs'>(initialTab)
  const [certDetail, setCertDetail] = useState<CertInfo | null>(null)
  const [savedWhileStopped, setSavedWhileStopped] = useState(false)

  // Track draft settings and baseline for deep dirty-checking
  const [draft, setDraft] = useState<WebConfig | null>(() => (cfg ? JSON.parse(JSON.stringify(cfg)) : null))
  const [baseline, setBaseline] = useState<WebConfig | null>(null)

  // Establish initial baseline and sync draft when baseline is established
  useEffect(() => {
    if (cfg && status && !baseline) {
      const b = computeAppliedBaseline(cfg, status)
      setBaseline(b)
      setDraft(JSON.parse(JSON.stringify(b)))
    }
  }, [cfg, status, baseline])

  // Track changes across Server tab, Web config, and pending settings
  const serverChanges = useMemo(() => computeServerChanges(draft, baseline), [draft, baseline])
  const configChanges = useMemo(() => web.driftedHosts.map((h) => `Config drift: ${h}`), [web.driftedHosts])
  const externalPending = useMemo(() => {
    if (!status?.running || !cfg || !baseline) return 0
    return cfg.default_server !== baseline.default_server || cfg.php_workers !== baseline.php_workers || cfg.dns_port !== baseline.dns_port ? 1 : 0
  }, [status, cfg, baseline])

  const totalPendingChanges = serverChanges.length + configChanges.length + externalPending + (savedWhileStopped ? 1 : 0)
  const hasChanges = totalPendingChanges > 0
  const validation = useMemo(() => validateWebDraft(draft, status), [draft, status])

  // Warn before leaving the page with unapplied changes
  useEffect(() => {
    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      if (serverChanges.length > 0) {
        e.preventDefault()
        e.returnValue = ''
      }
    }
    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => window.removeEventListener('beforeunload', handleBeforeUnload)
  }, [serverChanges.length])

  // Warn before switching tabs with unapplied changes
  async function handleTabChange(next: 'server' | 'config' | 'certs') {
    if (tab === 'server' && next !== 'server' && serverChanges.length > 0) {
      const ok = await confirmAction(
        `You have ${serverChanges.length} unapplied change(s) in server settings. Leave this tab anyway?`
      )
      if (!ok) return
    }
    setTab(next)
    onTabChange?.(next)
  }

  async function saveSettings(currentDraft: WebConfig) {
    await runCommand({ type: 'set_setting', key: 'web.default_server', value: currentDraft.default_server })
    await runCommand({ type: 'set_setting', key: 'web.php_workers', value: currentDraft.php_workers })
    await runCommand({ type: 'set_setting', key: 'web.dns_port', value: currentDraft.dns_port })
    for (const [id, ports] of Object.entries(currentDraft.servers)) {
      await runCommand({ type: 'set_setting', key: `web.servers.${id}.http_port`, value: ports.http })
      await runCommand({ type: 'set_setting', key: `web.servers.${id}.https_port`, value: ports.https })
    }
    await refreshConfig()
  }

  async function handleApply() {
    if (validation.error) return
    await run('apply', async () => {
      if (draft && serverChanges.length > 0) {
        await saveSettings(draft)
      }
      setSavedWhileStopped(false)
      await apply()
      if (draft) {
        setBaseline(JSON.parse(JSON.stringify(draft)))
      }
    })
  }

  function handleDiscard() {
    if (baseline) {
      setDraft(JSON.parse(JSON.stringify(baseline)))
    }
  }

  const isApplying = busy === 'apply'
  const applyButtonDisabled =
    isApplying ||
    !!validation.error ||
    (status?.running ? !hasChanges : false)

  const applyButtonTooltip = validation.error
    ? validation.error
    : status?.running
      ? hasChanges
        ? `Apply ${totalPendingChanges} pending change${totalPendingChanges === 1 ? '' : 's'}`
        : 'No changes to apply'
      : 'Start the web server with current settings'

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Web server</h1>
          <p className="text-sm text-muted-foreground">
            The server that serves your sites, its ports, and the local HTTPS certificates. Sites themselves live on the Sites page (§44–53).
          </p>
        </div>
        <div className="flex flex-col items-end gap-2">
          <div className="flex items-center gap-2">
            {/* Discard changes option when draft has unapplied changes */}
            {serverChanges.length > 0 && status?.running && (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy !== null}
                onClick={handleDiscard}
                title="Discard all unapplied server settings changes"
                className="h-9 px-2.5 text-xs text-muted-foreground hover:text-foreground cursor-pointer"
              >
                <RotateCcw className="size-3.5 mr-1" /> Discard
              </Button>
            )}

            {/* Apply changes / Start web server button */}
            <Button
              variant={status?.running && hasChanges && !validation.error ? 'default' : 'secondary'}
              size="sm"
              disabled={applyButtonDisabled}
              title={applyButtonTooltip}
              onClick={() => void handleApply()}
              className={cn(
                'h-9 px-3.5 text-xs font-medium transition-all gap-1.5',
                status?.running && hasChanges && !validation.error && 'ring-1 ring-primary/40 shadow-sm cursor-pointer',
                applyButtonDisabled && 'opacity-50 cursor-not-allowed'
              )}
            >
              {isApplying ? (
                <Spinner className="size-3.5" />
              ) : status?.running ? (
                <Play className="size-3.5" />
              ) : (
                <Play className="size-3.5" />
              )}
              {isApplying
                ? status?.running
                  ? 'Applying…'
                  : 'Starting…'
                : status?.running
                  ? hasChanges
                    ? `Apply changes (${totalPendingChanges})`
                    : 'Apply changes'
                  : 'Start web server'}
            </Button>

            {status?.running && (
              <Button
                variant="outline"
                size="sm"
                disabled={busy !== null}
                onClick={() =>
                  run('stop', async () => {
                    await runCommand({ type: 'stop_web' })
                    await waitForWebStopped()
                    await refresh()
                  })
                }
                className="h-9 px-3 text-xs gap-1.5"
              >
                {busy === 'stop' ? <Spinner className="size-3.5" /> : <StopIcon className="size-3.5" />}
                {busy === 'stop' ? 'Stopping…' : 'Stop'}
              </Button>
            )}
          </div>

          {savedWhileStopped && !status?.running && (
            <p className="max-w-[18rem] text-right text-xs text-muted-foreground">
              Saved. The web server is stopped, so these settings apply the next time you start it or choose Start web server.
            </p>
          )}

          {validation.error && (
            <p className="max-w-[22rem] text-right text-xs text-destructive font-medium">
              {validation.error}
            </p>
          )}
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {status?.running && hasChanges && (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-warning/40 bg-warning/10 p-3.5 text-sm shadow-xs">
          <div className="flex items-center gap-2.5 min-w-0">
            <AlertTriangle className="size-4 shrink-0 text-warning" />
            <span className="font-semibold text-warning shrink-0">Unapplied server changes</span>
            <span className="text-xs text-muted-foreground truncate">
              You have {totalPendingChanges} unapplied change{totalPendingChanges === 1 ? '' : 's'} in server configuration.
            </span>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {serverChanges.length > 0 && (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy !== null}
                onClick={handleDiscard}
                className="h-7 px-2.5 text-xs text-muted-foreground hover:text-foreground cursor-pointer"
              >
                <RotateCcw className="size-3 mr-1" /> Discard
              </Button>
            )}
            <Button
              size="sm"
              variant="secondary"
              disabled={applyButtonDisabled}
              onClick={() => void handleApply()}
              className="h-7 px-2.5 text-xs font-medium bg-warning/20 hover:bg-warning/30 text-foreground border border-warning/30 cursor-pointer"
            >
              {isApplying ? <Spinner className="size-3 mr-1" /> : <Play className="size-3 mr-1" />}
              Apply changes
            </Button>
          </div>
        </div>
      )}

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
          {
            id: 'server',
            label: 'Server',
            badge: serverChanges.length > 0 ? serverChanges.length : undefined,
          },
          {
            id: 'config',
            label: 'Web config',
            badge: configChanges.length > 0 ? configChanges.length : undefined,
          },
          { id: 'certs', label: 'Certificates', badge: certs.length },
        ]}
        value={tab}
        onChange={(next) => void handleTabChange(next)}
      />

      {tab === 'server' && draft && status && (
        <ServerPanel
          draft={draft}
          setDraft={setDraft}
          status={status}
          domains={web.domains}
          invalidFields={validation.invalidFields}
          serverChanges={serverChanges}
          onSave={handleApply}
          onDiscard={handleDiscard}
          busyKey={busy}
        />
      )}

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
                    onClick={() =>
                      confirmThen('Remove the CA from the Windows trust store? HTTPS sites will show warnings.', () =>
                        run('untrust', () => runCommand({ type: 'untrust_ca' }).then(refresh))
                      )
                    }
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
                          <Button
                            size="sm"
                            variant="ghost"
                            title="Regenerate"
                            disabled={busy !== null}
                            onClick={() => run('regen', () => runCommand({ type: 'regenerate_certificate', hostname: c.hostname }).then(refresh))}
                          >
                            <RefreshCw className="size-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            title="Revoke (delete)"
                            onClick={() =>
                              confirmThen(`Revoke the certificate for ${c.hostname}? The site can't use HTTPS until a new one is generated.`, () =>
                                run('revoke', () => runCommand({ type: 'revoke_certificate', hostname: c.hostname }).then(refresh))
                              )
                            }
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

/**
 * The ports each server will actually bind, and the ones used twice. The default always
 * takes 80/443 whatever is stored for it, so a custom port on either is a clash too.
 */
function portClashes(cfg: WebConfig): string[] {
  const claims: { port: number; who: string }[] = [
    { port: 80, who: 'the default server on 80' },
    { port: 443, who: 'the default server on 443' },
  ]
  for (const [id, ports] of Object.entries(cfg.servers)) {
    if (id === cfg.default_server) continue
    claims.push({ port: ports.http, who: `${id} HTTP` }, { port: ports.https, who: `${id} HTTPS` })
  }
  const out: string[] = []
  for (let i = 0; i < claims.length; i++) {
    for (let j = i + 1; j < claims.length; j++) {
      const a = claims[i]
      const b = claims[j]
      if (a.port === b.port && a.who !== b.who) {
        out.push(`${a.who} and ${b.who} are both on port ${a.port} — two servers can't share a port.`)
      }
    }
  }
  return out
}

function ServerPanel({
  draft,
  setDraft,
  status,
  domains,
  invalidFields,
  serverChanges,
  onSave,
  onDiscard,
  busyKey,
}: {
  draft: WebConfig
  setDraft: React.Dispatch<React.SetStateAction<WebConfig | null>>
  status: WebStatus
  domains: DomainSummary[]
  invalidFields: Set<string>
  serverChanges: string[]
  onSave: () => void
  onDiscard: () => void
  busyKey: string | null
}) {
  const enabledDomains = domains.filter((d) => d.enabled)
  const setPorts = (id: string, patch: Partial<ServerPorts>) =>
    setDraft((prev) => {
      if (!prev) return prev
      return {
        ...prev,
        servers: { ...prev.servers, [id]: { ...(prev.servers[id] ?? { http: 80, https: 443 }), ...patch } },
      }
    })

  const busy = busyKey !== null
  const clashes = portClashes(draft)

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex-row items-start justify-between gap-3 space-y-0 pb-2">
          <div className="min-w-0">
            <CardTitle className="text-sm">Default web server</CardTitle>
            <CardDescription>
              The default server always binds ports 80 and 443 and serves every site that hasn't picked one for itself. The other
              servers run at the same time on their own ports.
            </CardDescription>
          </div>
          {status.running ? (
            <Badge variant="success" className="shrink-0 whitespace-nowrap">
              ● Running
            </Badge>
          ) : (
            <Badge variant="secondary" className="shrink-0 whitespace-nowrap">
              Stopped
            </Badge>
          )}
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
                  onClick={() => setDraft((prev) => (prev ? { ...prev, default_server: s.id } : prev))}
                  className={cn(
                    'relative flex items-center gap-3 rounded-xl border p-3 text-left transition-all disabled:cursor-not-allowed disabled:opacity-50 cursor-pointer',
                    selected ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-foreground/20 hover:bg-accent/50'
                  )}
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
              <NumberInput
                label="PHP workers per version"
                min={1}
                max={16}
                value={draft.php_workers}
                className={cn(invalidFields.has('php_workers') && 'border-destructive ring-1 ring-destructive')}
                onChange={(n) => setDraft((prev) => (prev ? { ...prev, php_workers: n ?? 3 } : prev))}
              />
            </Field>
            <Field label="Wildcard DNS port" hint="Windows only routes DNS rules to port 53">
              <NumberInput
                label="wildcard DNS port"
                value={draft.dns_port}
                className={cn(invalidFields.has('dns_port') && 'border-destructive ring-1 ring-destructive')}
                onChange={(n) => setDraft((prev) => (prev ? { ...prev, dns_port: n ?? 53 } : prev))}
              />
            </Field>
          </div>
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
            const siteCount = enabledDomains.filter((d) => d.server === s.id).length
            const isHttpInvalid = invalidFields.has(`${s.id}:http`)
            const isHttpsInvalid = invalidFields.has(`${s.id}:https`)

            return (
              <div key={s.id} className="grid items-end gap-3 rounded-lg border p-3 sm:grid-cols-[minmax(0,1fr)_5.5rem_5.5rem_7rem]">
                <div className="flex min-w-0 items-center gap-2">
                  <TechTile id={s.id} />
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 text-sm font-medium">
                      {s.name}
                      {s.running && <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" title="Running" />}
                      {isDefault && <Badge variant="outline">Default</Badge>}
                    </div>
                    <div className="truncate text-xs text-muted-foreground">
                      {s.installed ? `Binds ${s.http_port} / ${s.https_port} right now` : 'Not installed'}
                    </div>
                  </div>
                </div>
                <Field label="HTTP port">
                  <NumberInput
                    label={`${s.name} HTTP port`}
                    className={cn('px-2.5 tabular-nums', isHttpInvalid && 'border-destructive ring-1 ring-destructive')}
                    disabled={isDefault}
                    title={isDefault ? 'The default server always binds port 80' : undefined}
                    value={isDefault ? 80 : ports.http}
                    onChange={(n) => setPorts(s.id, { http: n ?? 80 })}
                  />
                </Field>
                <Field label="HTTPS port">
                  <NumberInput
                    label={`${s.name} HTTPS port`}
                    className={cn('px-2.5 tabular-nums', isHttpsInvalid && 'border-destructive ring-1 ring-destructive')}
                    disabled={isDefault}
                    title={isDefault ? 'The default server always binds port 443' : undefined}
                    value={isDefault ? 443 : ports.https}
                    onChange={(n) => setPorts(s.id, { https: n ?? 443 })}
                  />
                </Field>
                <div className="flex h-9 min-w-0 items-center text-xs text-muted-foreground">
                  <span className="truncate">{isDefault ? 'Locked: 80/443' : `${siteCount} site(s)`}</span>
                </div>
              </div>
            )
          })}

          {clashes.length > 0 && (
            <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
              <div className="font-medium text-destructive">Two servers want the same port</div>
              <ul className="mt-1 list-disc pl-5 text-muted-foreground">
                {clashes.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
            </div>
          )}

          {serverChanges.length > 0 && (
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-primary/30 bg-primary/5 p-3 text-xs">
              <div className="text-foreground">
                <span className="font-semibold">{serverChanges.length} pending change(s):</span>{' '}
                <span className="text-muted-foreground">{serverChanges.join(', ')}</span>
              </div>
              <div className="flex items-center gap-2">
                <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={onDiscard} disabled={busy}>
                  Discard
                </Button>
                <Button size="sm" className="h-7 text-xs" onClick={onSave} disabled={busy || clashes.length > 0}>
                  {busy ? <Spinner className="size-3" /> : null} Apply now
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

const SERVER_BLURB: Record<string, string> = {
  nginx: 'Fast and light · default',
  apache: 'Honours .htaccess files',
  caddy: 'Simple, HTTPS-first',
}
