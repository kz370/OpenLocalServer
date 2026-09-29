import { open } from '@tauri-apps/plugin-dialog'
import {
  Activity,
  AlertTriangle,
  Check,
  Copy,
  ExternalLink,
  FolderSearch,
  Globe2,
  KeyRound,
  Lock,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Send,
  Trash2,
} from 'lucide-react'
import { useCallback, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs, Textarea, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import {
  type DomainSummary,
  type Project,
  type RecordedRequest,
  type TunnelConfig,
  type TunnelProvider,
  type TunnelStatus,
  runCommand,
} from '@/core'
import { confirmAction } from '@/lib/confirm'
import { formatBytes, timeAgo, useAction, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const EMPTY: TunnelConfig = {
  id: '',
  project_id: null,
  name: '',
  provider: 'cloudflare',
  target: '',
  auth_user: null,
  allow_internal: false,
  public_hostname: null,
  acknowledged: false,
  autostart: false,
}

const STATE_BADGE: Record<TunnelStatus['state'], { label: string; variant: 'success' | 'warning' | 'destructive' | 'secondary' }> = {
  connected: { label: 'Public', variant: 'destructive' },
  starting: { label: 'Connecting…', variant: 'warning' },
  failed: { label: 'Failed', variant: 'destructive' },
  needs_confirmation: { label: 'Needs confirmation', variant: 'warning' },
  stopped: { label: 'Stopped', variant: 'secondary' },
}

/** §56–60, §110–111: public tunnels, their traffic, and a webhook tester. */
export function TunnelsPage() {
  const [tunnels, setTunnels] = useState<TunnelStatus[]>([])
  const [providers, setProviders] = useState<TunnelProvider[]>([])
  const [domains, setDomains] = useState<DomainSummary[]>([])
  const [projects, setProjects] = useState<Project[]>([])
  const [editing, setEditing] = useState<TunnelConfig | null>(null)
  const [confirming, setConfirming] = useState<TunnelStatus | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const refresh = useCallback(async () => {
    const r = await runCommand({ type: 'list_tunnels' })
    if (r.type === 'tunnels') setTunnels(r.tunnels)
  }, [])

  const loadAll = useCallback(async () => {
    const [p, d, pr] = await Promise.all([runCommand({ type: 'list_tunnel_providers' }), runCommand({ type: 'list_domains' }), runCommand({ type: 'list_projects' })])
    if (p.type === 'tunnel_providers') setProviders(p.providers)
    if (d.type === 'domains') setDomains(d.domains)
    if (pr.type === 'projects') setProjects(pr.projects)
    await refresh()
  }, [refresh])

  usePoll(refresh, 2000)
  usePoll(loadAll, 30000)

  async function start(t: TunnelStatus, confirm: boolean) {
    await run(`start:${t.config.id}`, async () => {
      const r = await runCommand({ type: 'start_tunnel', id: t.config.id, confirm_exposure: confirm })
      if (r.type === 'tunnel' && r.tunnel.state === 'needs_confirmation') setConfirming(r.tunnel)
      else setSelected(t.config.id)
      await refresh()
    })
  }

  async function stop(t: TunnelStatus) {
    await run(`stop:${t.config.id}`, async () => {
      await runCommand({ type: 'stop_tunnel', id: t.config.id })
      await refresh()
    })
  }

  async function remove(t: TunnelStatus) {
    if (!(await confirmAction(`Delete the tunnel "${t.config.name}"? It stops first if it is running.`))) return
    await run(`remove:${t.config.id}`, async () => {
      await runCommand({ type: 'remove_tunnel', id: t.config.id })
      if (selected === t.config.id) setSelected(null)
      await refresh()
    })
  }

  const publicCount = tunnels.filter((t) => t.state === 'connected' || t.state === 'starting').length
  const current = tunnels.find((t) => t.config.id === selected) ?? null

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Tunnels</h1>
          <p className="text-sm text-muted-foreground">Make a local site reachable from the internet for webhooks, OAuth callbacks or a quick demo. Nothing is ever made public without you starting it.</p>
        </div>
        <Button size="sm" onClick={() => setEditing({ ...EMPTY, target: domains[0] ? domains[0].url.replace(/\/$/, '') : '' })}>
          <Plus /> New tunnel
        </Button>
      </div>

      {publicCount > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm" role="status">
          <span className="flex items-center gap-2 font-medium text-destructive">
            <Globe2 className="size-4" />
            {publicCount === 1 ? 'A site is public right now.' : `${publicCount} sites are public right now.`}
          </span>
          <Button
            size="sm"
            variant="destructive"
            onClick={() =>
              run('stop-all', async () => {
                for (const t of tunnels.filter((x) => x.state !== 'stopped')) await runCommand({ type: 'stop_tunnel', id: t.config.id })
                await refresh()
              })
            }
          >
            <StopIcon /> Stop all tunnels
          </Button>
        </div>
      )}

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {tunnels.length === 0 ? (
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-10 text-center">
            <Globe2 className="size-8 text-muted-foreground" />
            <p className="font-medium">No tunnels yet</p>
            <p className="max-w-md text-sm text-muted-foreground">A tunnel gives one of your sites a temporary public address. Cloudflare needs no account; ngrok needs a free one.</p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-3 lg:grid-cols-2">
          {tunnels.map((t) => (
            <TunnelCard
              key={t.config.id}
              t={t}
              busy={busy}
              selected={selected === t.config.id}
              project={projects.find((p) => p.id === t.config.project_id)}
              onSelect={() => setSelected(selected === t.config.id ? null : t.config.id)}
              onStart={() => start(t, false)}
              onStop={() => stop(t)}
              onEdit={() => setEditing(t.config)}
              onRemove={() => remove(t)}
              onCheck={() =>
                run(`check:${t.config.id}`, async () => {
                  await runCommand({ type: 'check_tunnel', id: t.config.id })
                  await refresh()
                })
              }
            />
          ))}
        </div>
      )}

      {current && current.state !== 'stopped' && <Inspector key={current.config.id} tunnel={current} />}

      <Providers providers={providers} onChanged={loadAll} />

      {editing && (
        <TunnelEditor
          initial={editing}
          domains={domains}
          projects={projects}
          providers={providers}
          onClose={() => setEditing(null)}
          onSaved={async (id) => {
            setEditing(null)
            setSelected(id)
            await refresh()
          }}
        />
      )}

      <Dialog
        open={confirming !== null}
        onClose={() => setConfirming(null)}
        title="Make this site public?"
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirming(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={async () => {
                const t = confirming
                setConfirming(null)
                if (t) await start(t, true)
              }}
            >
              <Globe2 /> Yes, make it public
            </Button>
          </>
        }
      >
        {confirming && (
          <div className="flex flex-col gap-3 text-sm">
            <p className="flex gap-2 rounded-lg border border-warning/40 bg-warning/10 p-3">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
              <span>{confirming.exposure}</span>
            </p>
            <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
              <li>Databases, caches, mail and debugger ports are never exposed by a tunnel.</li>
              <li>{confirming.has_password ? 'Visitors must enter the username and password you set.' : 'There is no password on this tunnel. You can add one in its settings.'}</li>
              <li>You are asked this once per tunnel; changing its target asks again.</li>
            </ul>
          </div>
        )}
      </Dialog>
    </div>
  )
}

function TunnelCard({
  t,
  busy,
  selected,
  project,
  onSelect,
  onStart,
  onStop,
  onEdit,
  onRemove,
  onCheck,
}: {
  t: TunnelStatus
  busy: string | null
  selected: boolean
  project: Project | undefined
  onSelect: () => void
  onStart: () => void
  onStop: () => void
  onEdit: () => void
  onRemove: () => void
  onCheck: () => void
}) {
  const [copied, setCopied] = useState(false)
  const [showLog, setShowLog] = useState(false)
  const [log, setLog] = useState<string[]>([])
  const running = t.state !== 'stopped' && t.state !== 'needs_confirmation'
  const badge = STATE_BADGE[t.state]
  const id = t.config.id

  usePoll(async () => {
    if (!showLog) return
    const r = await runCommand({ type: 'tunnel_log', id })
    if (r.type === 'lines') setLog(r.lines)
  }, 2000)

  return (
    <Card className={cn('transition-colors', t.state === 'connected' && 'border-destructive/50', selected && 'ring-1 ring-primary')}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-2 text-base">
              <span className="truncate">{t.config.name}</span>
              <Badge variant={badge.variant}>{badge.label}</Badge>
              {t.has_password && (
                <span title="Visitors need a username and password">
                  <Lock className="size-3.5 text-muted-foreground" />
                </span>
              )}
            </CardTitle>
            <CardDescription className="truncate">
              {t.config.target}
              {project && ` · ${project.name}`}
            </CardDescription>
          </div>
          <div className="flex shrink-0 gap-1">
            <Button size="icon" variant="ghost" className="size-8" onClick={onEdit} title="Settings">
              <Pencil />
            </Button>
            <Button size="icon" variant="ghost" className="size-8" onClick={onRemove} title="Delete">
              <Trash2 />
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {t.public_url ? (
          <div className="flex min-w-0 items-center gap-2 rounded-lg bg-muted/60 px-3 py-2">
            <Globe2 className="size-4 shrink-0 text-destructive" />
            <span className="min-w-0 flex-1 truncate font-mono text-sm" title={t.public_url}>
              {t.public_url}
            </span>
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              title="Copy the public address"
              onClick={async () => {
                await navigator.clipboard.writeText(t.public_url ?? '')
                setCopied(true)
                setTimeout(() => setCopied(false), 1500)
              }}
            >
              {copied ? <Check /> : <Copy />}
            </Button>
            <Button size="icon" variant="ghost" className="size-7" title="Open" onClick={() => runCommand({ type: 'open_url', url: t.public_url ?? '' })}>
              <ExternalLink />
            </Button>
          </div>
        ) : t.state === 'starting' ? (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Waiting for {t.config.provider} to hand out a public address…
          </p>
        ) : null}

        {t.error && <p className="text-sm text-destructive">{t.error}</p>}

        {running && (
          <dl className="grid grid-cols-3 gap-2 text-xs">
            <div>
              <dt className="text-muted-foreground">Requests</dt>
              <dd className="font-medium tabular-nums">{t.requests}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Last request</dt>
              <dd className="font-medium">{t.last_request_ms ? timeAgo(t.last_request_ms) : '—'}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Latency</dt>
              <dd className="font-medium tabular-nums">{t.latency_ms !== null ? `${t.latency_ms} ms` : '—'}</dd>
            </div>
          </dl>
        )}

        <div className="flex flex-wrap items-center gap-2">
          {running ? (
            <Button size="sm" variant="destructive" onClick={onStop} disabled={busy === `stop:${id}`}>
              {busy === `stop:${id}` ? <Spinner /> : <StopIcon />} Stop
            </Button>
          ) : (
            <Button size="sm" onClick={onStart} disabled={busy === `start:${id}`}>
              {busy === `start:${id}` ? <Spinner /> : <Play />} Start
            </Button>
          )}
          {running && (
            <>
              <Button size="sm" variant="secondary" onClick={onSelect}>
                <Activity /> {selected ? 'Hide traffic' : 'Traffic'}
              </Button>
              <Button size="sm" variant="ghost" onClick={onCheck} disabled={!t.public_url || busy === `check:${id}`}>
                {busy === `check:${id}` ? <Spinner /> : <RefreshCw />} Check
              </Button>
            </>
          )}
          {t.config.provider !== 'mock' && (
            <Button size="sm" variant="ghost" onClick={() => setShowLog(!showLog)}>
              <ScrollText /> Logs
            </Button>
          )}
        </div>

        {showLog && (
          <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all rounded-md bg-muted p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
            {log.length ? log.join('\n') : 'No output yet.'}
          </pre>
        )}
      </CardContent>
    </Card>
  )
}

function statusColor(s: number) {
  if (s >= 500 || s === 0) return 'text-destructive'
  if (s >= 400) return 'text-warning'
  if (s >= 300) return 'text-muted-foreground'
  return 'text-success'
}

/** §110 traffic inspector and §111 webhook tester for one running tunnel. */
function Inspector({ tunnel }: { tunnel: TunnelStatus }) {
  const [tab, setTab] = useState<'traffic' | 'webhook'>('traffic')
  const [requests, setRequests] = useState<RecordedRequest[]>([])
  const [openId, setOpenId] = useState<number | null>(null)
  const { busy, error, setError, run } = useAction()
  const id = tunnel.config.id

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'list_tunnel_requests', id }).catch(() => null)
    if (r?.type === 'tunnel_requests') setRequests(r.requests)
  }, [id])
  usePoll(load, 1500)

  const detail = requests.find((r) => r.id === openId) ?? null

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between gap-3">
          <div>
            <CardTitle className="text-sm">Traffic · {tunnel.config.name}</CardTitle>
            <CardDescription>Every request through the tunnel. Passwords, tokens, cookies and signatures are hidden.</CardDescription>
          </div>
          <Button
            size="sm"
            variant="ghost"
            onClick={() =>
              run('clear', async () => {
                await runCommand({ type: 'clear_tunnel_requests', id })
                setOpenId(null)
                await load()
              })
            }
          >
            <Trash2 /> Clear
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <Tabs
          tabs={[
            { id: 'traffic', label: 'Requests', badge: requests.length },
            { id: 'webhook', label: 'Webhook tester' },
          ]}
          value={tab}
          onChange={setTab}
        />
        <ErrorCard error={error} onDismiss={() => setError(null)} />

        {tab === 'traffic' && (
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <div className="max-h-[28rem] overflow-y-auto rounded-lg border border-border">
              {requests.length === 0 && <p className="p-4 text-sm text-muted-foreground">No requests yet. Open the public address, or send one from the webhook tester.</p>}
              {requests.map((r) => (
                <button
                  key={r.id}
                  onClick={() => setOpenId(r.id === openId ? null : r.id)}
                  className={cn('grid w-full grid-cols-[3.5rem_minmax(0,1fr)_3rem_4rem] items-center gap-2 border-b border-border px-3 py-2 text-left text-xs last:border-b-0 hover:bg-accent/50', openId === r.id && 'bg-accent')}
                >
                  <span className="font-mono font-medium">{r.method}</span>
                  <span className="truncate font-mono" title={r.path}>
                    {r.replay && <RotateCcw className="mr-1 inline size-3 text-muted-foreground" />}
                    {r.path}
                  </span>
                  <span className={cn('font-mono tabular-nums', statusColor(r.status))}>{r.status || 'ERR'}</span>
                  <span className="text-right tabular-nums text-muted-foreground">{r.duration_ms} ms</span>
                </button>
              ))}
            </div>
            {detail ? (
              <RequestDetail
                r={detail}
                tunnel={tunnel}
                busy={busy === 'replay'}
                onReplay={() =>
                  run('replay', async () => {
                    const res = await runCommand({ type: 'replay_tunnel_request', id, request_id: detail.id })
                    if (res.type === 'tunnel_request') setOpenId(res.request.id)
                    await load()
                  })
                }
              />
            ) : (
              <p className="hidden rounded-lg border border-dashed border-border p-4 text-sm text-muted-foreground xl:block">Pick a request to see its headers and body.</p>
            )}
          </div>
        )}

        {tab === 'webhook' && (
          <WebhookTester
            tunnel={tunnel}
            onSent={async (rid) => {
              await load()
              setOpenId(rid)
              setTab('traffic')
            }}
          />
        )}
      </CardContent>
    </Card>
  )
}

function RequestDetail({ r, tunnel, busy, onReplay }: { r: RecordedRequest; tunnel: TunnelStatus; busy: boolean; onReplay: () => void }) {
  const [saved, setSaved] = useState<string | null>(null)
  const project = tunnel.config.project_id
  const base = { feature: 'traffic' as const, tunnel_id: tunnel.config.id, request_ids: [r.id] }
  return (
    <div className="flex min-w-0 flex-col gap-3 rounded-lg border border-border p-3 text-xs">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="break-all font-mono text-sm">
            {r.method} {r.path}
          </p>
          <p className="text-muted-foreground">
            <span className={statusColor(r.status)}>{r.status || 'no answer'}</span> · {r.duration_ms} ms · sent {formatBytes(r.request_size)}, got {formatBytes(r.response_size)}
            {r.client && ` · from ${r.client}`} · {timeAgo(r.time_ms)}
          </p>
        </div>
        <Button size="sm" variant="secondary" onClick={onReplay} disabled={busy} title="Send it to the site again, exactly as it came">
          {busy ? <Spinner /> : <RotateCcw />} Replay
        </Button>
      </div>
      <div className="flex flex-wrap gap-1">
        <AiButton label="Explain" variant="secondary" ask={{ title: 'Explain this request', description: 'Sends this request as recorded (secrets already hidden).', request: { ...base, kind: 'explain' }, question: 'optional' }} />
        <AiButton
          label="Write a handler"
          variant="secondary"
          ask={{ title: 'Write a handler for this webhook', request: { ...base, kind: 'handler' }, question: 'optional', placeholder: 'Language or framework, e.g. Laravel, Express, Flask (default: PHP)' }}
        />
        {project && (
          <AiButton
            label="Write a k6 script"
            variant="secondary"
            ask={{
              title: 'Write a k6 load test from this request',
              request: { ...base, kind: 'k6', project_id: project },
              question: 'optional',
              onScript: (content) =>
                void runCommand({ type: 'load_save_script', project_id: project, name: 'from-traffic.js', content })
                  .then(() => setSaved('Saved as .openlocalserver/k6/from-traffic.js. Run it from the project\'s Load tab.'))
                  .catch(() => setSaved('The script could not be saved.')),
            }}
          />
        )}
      </div>
      {saved && <p className="text-success">{saved}</p>}
      {r.error && <p className="text-destructive">{r.error}</p>}
      <Section title="Request headers" rows={r.request_headers} />
      {r.request_body && <Body title="Request body" text={r.request_body} />}
      <Section title="Response headers" rows={r.response_headers} />
      {r.response_body && <Body title="Response body" text={r.response_body} />}
    </div>
  )
}

function Section({ title, rows }: { title: string; rows: [string, string][] }) {
  if (rows.length === 0) return null
  return (
    <div>
      <p className="mb-1 font-medium">{title}</p>
      <div className="grid grid-cols-[minmax(6rem,auto)_minmax(0,1fr)] gap-x-3 gap-y-0.5 font-mono">
        {rows.map(([k, v], i) => (
          <div key={`${k}-${i}`} className="contents">
            <span className="text-muted-foreground">{k}</span>
            <span className={cn('break-all', v === '[redacted]' && 'italic text-muted-foreground')}>{v}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function Body({ title, text }: { title: string; text: string }) {
  let shown = text
  try {
    shown = JSON.stringify(JSON.parse(text), null, 2)
  } catch {
    // Not JSON: show as it came.
  }
  return (
    <div>
      <p className="mb-1 font-medium">{title}</p>
      <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded bg-muted p-2 font-mono text-[11px]">{shown}</pre>
    </div>
  )
}

function WebhookTester({ tunnel, onSent }: { tunnel: TunnelStatus; onSent: (id: number) => void }) {
  const [method, setMethod] = useState('POST')
  const [path, setPath] = useState('/webhook')
  const [headers, setHeaders] = useState('Content-Type: application/json')
  const [body, setBody] = useState('{\n  "event": "test",\n  "id": 1\n}')
  const { busy, error, setError, run } = useAction()
  const hookUrl = tunnel.public_url ? `${tunnel.public_url.replace(/\/$/, '')}${path.startsWith('/') ? path : `/${path}`}` : null

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Give this address to the service that sends webhooks (Stripe, GitHub, an OAuth provider). Or send a test request below: it goes through the same path and shows up under Requests.
      </p>
      {hookUrl && (
        <div className="flex items-center gap-2 rounded-lg bg-muted/60 px-3 py-2 font-mono text-sm">
          <span className="min-w-0 flex-1 truncate">{hookUrl}</span>
          <Button size="icon" variant="ghost" className="size-7" onClick={() => navigator.clipboard.writeText(hookUrl)} title="Copy">
            <Copy />
          </Button>
        </div>
      )}
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex flex-wrap gap-2">
        <Select value={method} onChange={(e) => setMethod(e.target.value)} className="w-28" aria-label="Method">
          {['POST', 'GET', 'PUT', 'PATCH', 'DELETE'].map((m) => (
            <option key={m}>{m}</option>
          ))}
        </Select>
        <Input value={path} onChange={(e) => setPath(e.target.value)} className="min-w-0 flex-1 font-mono" aria-label="Path" />
      </div>
      <Field label="Headers" hint="One per line: Name: value">
        <Textarea value={headers} onChange={(e) => setHeaders(e.target.value)} rows={3} />
      </Field>
      {method !== 'GET' && (
        <Field label="Body">
          <Textarea value={body} onChange={(e) => setBody(e.target.value)} rows={6} />
        </Field>
      )}
      <div>
        <Button
          disabled={busy !== null}
          onClick={() =>
            run('send', async () => {
              const list: [string, string][] = headers
                .split('\n')
                .map((l) => l.trim())
                .filter(Boolean)
                .map((l) => {
                  const i = l.indexOf(':')
                  return [l.slice(0, i).trim(), l.slice(i + 1).trim()] as [string, string]
                })
                .filter(([k]) => k.length > 0)
              const r = await runCommand({ type: 'send_tunnel_test_request', id: tunnel.config.id, method, path, headers: list, body: method === 'GET' ? '' : body })
              if (r.type === 'tunnel_request') onSent(r.request.id)
            })
          }
        >
          {busy ? <Spinner /> : <Send />} Send test request
        </Button>
      </div>
    </div>
  )
}

function TunnelEditor({
  initial,
  domains,
  projects,
  providers,
  onClose,
  onSaved,
}: {
  initial: TunnelConfig
  domains: DomainSummary[]
  projects: Project[]
  providers: TunnelProvider[]
  onClose: () => void
  onSaved: (id: string) => void
}) {
  const [t, setT] = useState<TunnelConfig>(initial)
  const [password, setPassword] = useState('')
  const [protect, setProtect] = useState(!!initial.auth_user)
  const { busy, error, setError, run } = useAction()
  const set = (patch: Partial<TunnelConfig>) => setT({ ...t, ...patch })
  const siteTargets = domains.map((d) => d.url.replace(/\/$/, ''))
  const custom = !siteTargets.includes(t.target)
  const provider = providers.find((p) => p.id === t.provider)

  return (
    <Dialog
      open
      onClose={onClose}
      title={initial.id ? `Tunnel settings · ${initial.name}` : 'New tunnel'}
      description="Where the tunnel points and who may use it. Starting it is always a separate step."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={busy !== null || !t.target.trim()}
            onClick={() =>
              run('save', async () => {
                const r = await runCommand({ type: 'save_tunnel', tunnel: { ...t, auth_user: protect ? t.auth_user?.trim() || null : null } })
                if (r.type !== 'tunnel') return
                const id = r.tunnel.config.id
                if (!protect) await runCommand({ type: 'set_tunnel_password', id, password: null })
                else if (password) await runCommand({ type: 'set_tunnel_password', id, password })
                onSaved(id)
              })
            }
          >
            {busy ? <Spinner /> : <Check />} Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="Site">
          <Select
            value={custom ? '__custom' : t.target}
            onChange={(e) => {
              const v = e.target.value
              if (v === '__custom') set({ target: 'http://127.0.0.1:8000' })
              else set({ target: v, project_id: domains.find((d) => d.url.replace(/\/$/, '') === v)?.project_id ?? t.project_id, name: t.name || v.replace(/^https?:\/\//, '') })
            }}
          >
            {siteTargets.map((u) => (
              <option key={u} value={u}>
                {u}
              </option>
            ))}
            <option value="__custom">Another local address…</option>
          </Select>
        </Field>
        {custom && (
          <Field label="Local address" hint="An app on this computer, like http://127.0.0.1:8000.">
            <Input value={t.target} onChange={(e) => set({ target: e.target.value })} className="font-mono" />
          </Field>
        )}
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Name">
            <Input value={t.name} onChange={(e) => set({ name: e.target.value })} placeholder="shop.test" />
          </Field>
          <Field label="Project">
            <Select value={t.project_id ?? ''} onChange={(e) => set({ project_id: e.target.value || null })}>
              <option value="">None</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <Field label="Provider" hint={provider?.note}>
          <Select value={t.provider} onChange={(e) => set({ provider: e.target.value })}>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.path ? '' : ' (not installed)'}
              </option>
            ))}
          </Select>
        </Field>
        {t.provider === 'cloudflare' && provider?.token_saved && (
          <Field label="Public hostname" hint="For a named Cloudflare tunnel: the hostname set up in its dashboard.">
            <Input value={t.public_hostname ?? ''} onChange={(e) => set({ public_hostname: e.target.value || null })} placeholder="dev.example.com" />
          </Field>
        )}
        {t.provider === 'cloudflare' && !!t.public_hostname && <Toggle checked={!!t.autostart} onChange={(v) => set({ autostart: v })} label="Reconnect automatically" hint="Restart the named tunnel after a network interruption or process exit." />}
        <div className="flex flex-col gap-3 rounded-lg border border-border p-3">
          <Toggle checked={protect} onChange={setProtect} label="Ask visitors for a username and password" hint="Checked by OLS itself, so it works with every provider." />
          {protect && (
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Username">
                <Input value={t.auth_user ?? ''} onChange={(e) => set({ auth_user: e.target.value })} autoComplete="off" />
              </Field>
              <Field label="Password" hint={initial.auth_user ? 'Leave blank to keep the saved one.' : undefined}>
                <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
              </Field>
            </div>
          )}
        </div>
        <Toggle
          checked={t.allow_internal}
          onChange={(v) => set({ allow_internal: v })}
          label="Allow a database, cache, mail or debugger port as the target"
          hint="Off by default: those should never be public."
        />
      </div>
    </Dialog>
  )
}

function Providers({ providers, onChanged }: { providers: TunnelProvider[]; onChanged: () => void }) {
  const [tokenFor, setTokenFor] = useState<TunnelProvider | null>(null)
  const [token, setToken] = useState('')
  const { busy, error, setError, run } = useAction()

  async function locate(p: TunnelProvider) {
    const picked = await open({ multiple: false, directory: false, title: `Locate ${p.name}`, filters: [{ name: 'Program', extensions: ['exe', 'cmd'] }] })
    if (!picked || Array.isArray(picked)) return
    const program = { cloudflare: 'cloudflared', ngrok: 'ngrok', localtunnel: 'npx' }[p.id] ?? p.id
    await run(`locate:${p.id}`, async () => {
      await runCommand({ type: 'set_custom_install', id: program, label: '', path: picked })
      onChanged()
    })
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Providers</CardTitle>
        <CardDescription>Tokens are kept in Windows Credential Manager and passed to the provider privately.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {providers.map((p) => (
          <div key={p.id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border px-3 py-2.5">
            <div className="min-w-0">
              <p className="flex items-center gap-2 text-sm font-medium">
                {p.name}
                {p.path ? <Badge variant="success">found</Badge> : <Badge variant="outline">not installed</Badge>}
                {p.uses_token && p.token_saved && <Badge variant="secondary">token saved</Badge>}
              </p>
              <p className="truncate text-xs text-muted-foreground" title={p.path ?? p.install_hint}>
                {p.path ?? p.install_hint}
              </p>
            </div>
            <div className="flex gap-2">
              {p.uses_token && (
                <Button size="sm" variant="secondary" onClick={() => setTokenFor(p)}>
                  <KeyRound /> {p.token_saved ? 'Change token' : 'Add token'}
                </Button>
              )}
              {p.id !== 'localtunnel' && (
                <Button size="sm" variant="ghost" onClick={() => locate(p)} disabled={busy === `locate:${p.id}`}>
                  <FolderSearch /> Locate…
                </Button>
              )}
            </div>
          </div>
        ))}
      </CardContent>

      <Dialog
        open={tokenFor !== null}
        onClose={() => setTokenFor(null)}
        title={tokenFor ? `${tokenFor.name} token` : ''}
        description={tokenFor?.id === 'ngrok' ? 'Your authtoken from dashboard.ngrok.com.' : 'The token of a named tunnel from the Cloudflare dashboard. Without one, a free random address is used.'}
        footer={
          <>
            {tokenFor?.token_saved && (
              <Button
                variant="ghost"
                onClick={() =>
                  run('token', async () => {
                    await runCommand({ type: 'set_tunnel_token', provider: tokenFor.id, token: null })
                    setTokenFor(null)
                    onChanged()
                  })
                }
              >
                Forget saved token
              </Button>
            )}
            <Button
              disabled={!token.trim() || busy !== null}
              onClick={() =>
                run('token', async () => {
                  if (!tokenFor) return
                  await runCommand({ type: 'set_tunnel_token', provider: tokenFor.id, token: token.trim() })
                  setToken('')
                  setTokenFor(null)
                  onChanged()
                })
              }
            >
              <Check /> Save token
            </Button>
          </>
        }
      >
        <Input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="Paste the token" autoComplete="off" />
      </Dialog>
    </Card>
  )
}
