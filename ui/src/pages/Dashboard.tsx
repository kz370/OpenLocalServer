import { AlertTriangle, CheckCircle2, Code2, ExternalLink, Lock, Play, Rocket, XCircle } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DashboardData, type HealthItem, runCommand } from '@/core'
import { useAction, usePoll } from '@/lib/hooks'
import { waitForService, waitForWebStopped } from '@/lib/wait'
import { confirmAction } from '@/lib/confirm'

/** §173 / §116 / §101: what's running, what's wrong, and one-click ways to act on it. */
export function DashboardPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [data, setData] = useState<DashboardData | null>(null)
  const { busy, error, setError, run } = useAction()

  usePoll(async () => {
    try {
      const res = await runCommand({ type: 'get_dashboard' })
      if (res.type === 'dashboard') setData(res.data)
    } catch {
      /* the core is busy or restarting; the next poll retries */
    }
  }, 3000)

  const refresh = async () => {
    const res = await runCommand({ type: 'get_dashboard' })
    if (res.type === 'dashboard') setData(res.data)
  }

  const web = data?.web
  const problems = data?.health.filter((h) => h.status !== 'ok') ?? []

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
          <p className="text-sm text-muted-foreground">Your local environment at a glance.</p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => onNavigate('quickapps')}>
            <Rocket /> New from Quick App
          </Button>
          <Button
            disabled={busy !== null}
            onClick={() =>
              run('apply', async () => {
                await runCommand({ type: 'apply_web', overwrite: [] })
                await refresh()
              })
            }
          >
            {busy === 'apply' ? <Spinner /> : <Play />} {busy === 'apply' ? (web?.running ? 'Applying…' : 'Starting…') : web?.running ? 'Re-apply web config' : 'Start web server'}
          </Button>
          {web?.running && (
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

      {problems.length > 0 && (
        <Card className="border-warning/40">
          <CardHeader className="pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <AlertTriangle className="size-4 text-warning" /> Needs attention
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {problems.map((h) => (
              <HealthRow key={h.id + h.detail} item={h} />
            ))}
          </CardContent>
        </Card>
      )}

      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
            <div>
              <CardTitle className="text-sm">Sites</CardTitle>
              <CardDescription>
                {data ? `${data.domains.length} domain${data.domains.length === 1 ? '' : 's'}, ${data.project_count} project${data.project_count === 1 ? '' : 's'}` : 'Loading…'}
              </CardDescription>
            </div>
            <Button size="sm" variant="ghost" onClick={() => onNavigate('domains')}>
              Manage
            </Button>
          </CardHeader>
          <CardContent className="flex flex-col">
            {data?.domains.length === 0 && (
              <p className="text-sm text-muted-foreground">
                No sites yet. Create one from a Quick App, or add a domain for an existing project.
              </p>
            )}
            {!!data?.domains.length && (
              <div className="max-h-96 divide-y divide-border overflow-y-auto rounded-lg border border-border">
                {data.domains.map((d) => (
                  <div key={d.hostname} className={`group flex h-9 items-center gap-2.5 px-3 text-sm ${d.enabled ? '' : 'opacity-50'}`}>
                    <TechIcon id={d.kind} className="size-3.5" />
                    {d.https ? <Lock className="size-3 shrink-0 text-success" /> : <span className="w-3" />}
                    <button
                      className="min-w-0 flex-1 truncate text-left font-medium hover:text-primary hover:underline disabled:pointer-events-none"
                      disabled={!d.enabled || !web?.running}
                      title={`Open ${d.url}`}
                      onClick={() => run('open', () => runCommand({ type: 'open_url', url: d.url }))}
                    >
                      {d.hostname}
                    </button>
                    <span className="text-xs text-muted-foreground">
                      {d.kind}
                      {d.has_app && ' + app'}
                      {!d.enabled && ' · disabled'}
                    </span>
                    <span className="flex opacity-0 transition-opacity group-hover:opacity-100">
                      <Button size="sm" variant="ghost" className="h-7 px-2" title="Open in your code editor" onClick={() => run('code', () => runCommand({ type: 'open_in_editor', path: d.folder }))}>
                        <Code2 className="size-3.5" />
                      </Button>
                      <Button size="sm" variant="ghost" className="h-7 px-2" title="Open in browser" disabled={!d.enabled || !web?.running} onClick={() => run('open', () => runCommand({ type: 'open_url', url: d.url }))}>
                        <ExternalLink className="size-3.5" />
                      </Button>
                    </span>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader className="flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm">Web server</CardTitle>
              {web?.running ? <Badge variant="success">● Running</Badge> : <Badge variant="secondary">Stopped</Badge>}
            </CardHeader>
            <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
              <div>
                {web?.servers.find((s) => s.active)?.name ?? '…'} · ports {web?.http_port} / {web?.https_port}
              </div>
              {web?.php_pools.map((p) => (
                <div key={p.version}>
                  PHP {p.version} · {p.ports.length} worker{p.ports.length === 1 ? '' : 's'} {p.running ? '' : '(stopped)'}
                </div>
              ))}
              {web?.dns_running && <div>Wildcard DNS on port {web.dns_port}</div>}
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">Services</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-1.5">
              {data?.services
                .filter((s) => s.installed)
                .map((s) => (
                  <div key={s.id} className="flex items-center justify-between text-sm">
                    <span>{s.name}</span>
                    <span className="flex items-center gap-2">
                      {s.running ? <Badge variant="success">● Running</Badge> : <Badge variant="secondary">Stopped</Badge>}
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={busy !== null}
                        onClick={async () => {
                          if (s.running && !(await confirmAction(`Stop ${s.name}? Anything connected to it will be disconnected.`))) return
                          void run(s.id, async () => {
                            await runCommand({ type: s.running ? 'stop_service' : 'start_service', id: s.id })
                            await waitForService(s.id, s.running ? 'stopped' : 'running')
                            await refresh()
                          })
                        }}
                      >
                        {busy === s.id ? <Spinner className="size-3.5" /> : s.running ? <StopIcon className="size-3.5" /> : <Play className="size-3.5" />}
                      </Button>
                    </span>
                  </div>
                ))}
              {data && data.services.every((s) => !s.installed) && (
                <p className="text-sm text-muted-foreground">No services installed. Add some from Runtimes.</p>
              )}
            </CardContent>
          </Card>
        </div>
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Environment health</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-1.5 md:grid-cols-2">
          {data?.health.map((h) => <HealthRow key={h.id + h.detail} item={h} />)}
        </CardContent>
      </Card>
    </div>
  )
}

function HealthRow({ item }: { item: HealthItem }) {
  const Icon = item.status === 'ok' ? CheckCircle2 : item.status === 'warn' ? AlertTriangle : XCircle
  const color = item.status === 'ok' ? 'text-success' : item.status === 'warn' ? 'text-warning' : 'text-destructive'
  return (
    <div className="flex items-start gap-2 text-sm">
      <Icon className={`mt-0.5 size-4 shrink-0 ${color}`} />
      <div>
        <span className="font-medium">{item.label}</span>
        <span className="text-muted-foreground"> · {item.detail}</span>
        {item.fix && item.status !== 'ok' && <div className="text-xs text-muted-foreground">{item.fix}</div>}
      </div>
    </div>
  )
}
