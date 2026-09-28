import { Play, RefreshCw, RotateCw } from 'lucide-react'
import { type ReactNode, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { type ServiceStatus, runCommand } from '@/core'
import { usePoll } from '@/lib/hooks'
import type { Web } from '@/lib/web'
import { waitForService } from '@/lib/wait'

/**
 * Start, stop and restart what a site runs on without leaving its settings: the web server
 * (with its PHP processes), the site's own app process, and the database, cache and mail services.
 */
export function ServersPanel({ web, hostname }: { web: Web; hostname: string | null }) {
  const { status, busy, run, apply, refresh } = web
  const [services, setServices] = useState<ServiceStatus[]>([])

  usePoll(async () => {
    const r = await runCommand({ type: 'list_services' }).catch(() => null)
    // The web servers get their own rows above, with the ports they actually bind.
    if (r?.type === 'services') setServices(r.services.filter((s) => s.installed && s.kind !== 'web'))
  }, 4000)

  // This site's own server: what it pinned, or the default.
  const siteServer = (hostname ? web.domains.find((d) => d.hostname === hostname)?.server : null) ?? status?.default_server
  const server = status?.servers.find((s) => s.id === siteServer)
  const app = hostname ? status?.apps.find((a) => a.hostname === hostname) : undefined

  const restartWeb = () =>
    run('web:restart', async () => {
      if (server?.running) {
        await runCommand({ type: 'stop_service', id: server.id })
        await waitForService(server.id, 'stopped')
      }
      await apply()
    })

  async function serviceAction(s: ServiceStatus, action: 'start' | 'stop' | 'restart') {
    await run(`svc:${s.id}`, async () => {
      await runCommand({ type: `${action}_service`, id: s.id })
      await waitForService(s.id, action === 'stop' ? 'stopped' : 'running')
      const r = await runCommand({ type: 'list_services' })
      if (r.type === 'services') setServices(r.services.filter((x) => x.installed && x.kind !== 'web'))
    })
  }

  return (
    <div className="flex flex-col gap-5">
      <Section title="Web server" hint="This site's own server. Reloading keeps the other web servers running.">
        {status && server && (
          <ServerRow
            icon={server.id}
            name={server.name}
            detail={`HTTP ${server.http_port} · HTTPS ${server.https_port}${server.active ? ' · default (80/443)' : ''}`}
            running={server.running}
            busy={busy?.startsWith('web:') ?? false}
            actions={
              server.running ? (
                <>
                  <IconButton title="Reload config (apply changes without dropping connections)" disabled={busy !== null} onClick={() => run('web:reload', () => apply())}>
                    <RefreshCw />
                  </IconButton>
                  <IconButton title="Restart" disabled={busy !== null} onClick={restartWeb}>
                    {busy === 'web:restart' ? <Spinner /> : <RotateCw />}
                  </IconButton>
                  <IconButton
                    title="Stop"
                    disabled={busy !== null}
                    onClick={() =>
                      run('web:stop', async () => {
                        await runCommand({ type: 'stop_service', id: server.id })
                        await waitForService(server.id, 'stopped')
                        await refresh()
                      })
                    }
                  >
                    {busy === 'web:stop' ? <Spinner /> : <StopIcon />}
                  </IconButton>
                </>
              ) : (
                <IconButton title="Start" disabled={busy !== null} onClick={() => run('web:start', () => apply())}>
                  {busy === 'web:start' ? <Spinner /> : <Play />}
                </IconButton>
              )
            }
          />
        )}
        {status?.php_pools.map((p) => (
          <ServerRow key={p.version} icon="php" name={`PHP ${p.version}`} detail={`FastCGI on ${p.ports.join(', ')}`} running={p.running} busy={false} actions={null} />
        ))}
        {app && (
          <ServerRow
            icon="node"
            name="Site app"
            detail="The start command from this site's settings"
            running={app.running}
            busy={busy === 'app:restart'}
            actions={
              <IconButton title="Restart" disabled={busy !== null} onClick={() => run('app:restart', () => runCommand({ type: 'restart_site_app', hostname: hostname! }))}>
                {busy === 'app:restart' ? <Spinner /> : <RotateCw />}
              </IconButton>
            }
          />
        )}
      </Section>

      <Section title="Services" hint="Shared by every site. Install more on the Runtimes page.">
        {services.length === 0 && <p className="text-sm text-muted-foreground">No services installed.</p>}
        {services.map((s) => (
          <ServerRow
            key={s.id}
            icon={s.id}
            name={s.name}
            detail={[s.version, s.port ? `port ${s.port}` : null].filter(Boolean).join(' · ')}
            running={s.running}
            busy={busy === `svc:${s.id}`}
            actions={
              s.running ? (
                <>
                  <IconButton title="Restart" disabled={busy !== null} onClick={() => serviceAction(s, 'restart')}>
                    {busy === `svc:${s.id}` ? <Spinner /> : <RotateCw />}
                  </IconButton>
                  <IconButton title="Stop" disabled={busy !== null} onClick={() => serviceAction(s, 'stop')}>
                    <StopIcon />
                  </IconButton>
                </>
              ) : (
                <IconButton title="Start" disabled={busy !== null} onClick={() => serviceAction(s, 'start')}>
                  {busy === `svc:${s.id}` ? <Spinner /> : <Play />}
                </IconButton>
              )
            }
          />
        ))}
      </Section>
    </div>
  )
}

function Section({ title, hint, children }: { title: string; hint: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div>
        <h3 className="text-sm font-medium">{title}</h3>
        <p className="text-xs text-muted-foreground">{hint}</p>
      </div>
      <div className="flex flex-col rounded-lg border border-border">{children}</div>
    </div>
  )
}

function ServerRow({ icon, name, detail, running, busy, actions }: { icon: string; name: string; detail: string; running: boolean; busy: boolean; actions: ReactNode }) {
  const label = busy ? 'Working…' : running ? 'Running' : 'Stopped'
  return (
    <div className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-b-0">
      <TechTile id={icon} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-sm font-medium">
          {name}
          <span title={label} aria-label={label} role="img" className={`size-2 rounded-full ${running ? 'bg-emerald-500' : 'bg-muted-foreground/50'}`} />
        </div>
        <div className="truncate text-xs text-muted-foreground">{detail}</div>
      </div>
      <div className="flex shrink-0 items-center gap-1">{actions}</div>
    </div>
  )
}

function IconButton({ title, disabled, onClick, children }: { title: string; disabled: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <Button size="sm" variant="ghost" className="h-8 w-8 px-0 [&_svg]:size-3.5" title={title} aria-label={title} disabled={disabled} onClick={onClick}>
      {children}
    </Button>
  )
}
