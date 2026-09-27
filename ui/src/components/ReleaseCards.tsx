import { save } from '@tauri-apps/plugin-dialog'
import { Copy, Download, ExternalLink, FileText, KeyRound, RefreshCw, ShieldCheck, TerminalSquare, Wifi } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Select } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type ApiStatus, type NetworkStatus, type ShellMenuStatus, type UpdateInfo, type UpdaterStatus, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

/** §137: the local HTTP API. Off by default, token only, read-only unless switched to operate. */
export function ApiCard() {
  const [status, setStatus] = useState<ApiStatus | null>(null)
  const [port, setPort] = useState('7420')
  const [token, setToken] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_api_status' })
    if (r.type === 'api_status') {
      setStatus(r.status)
      setPort(String(r.status.settings.port))
    }
  }, [])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])
  if (!status) return null

  const apply = (enabled: boolean, mode = status.settings.mode) =>
    run('api', async () => {
      const r = await runCommand({ type: 'set_api_settings', enabled, port: parseInt(port, 10) || 7420, mode })
      if (r.type === 'api_status') setStatus(r.status)
    })

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <KeyRound className="size-4" /> Local API
          {status.running ? <Badge variant="success">on</Badge> : <Badge variant="outline">off</Badge>}
        </CardTitle>
        <CardDescription>
          Lets scripts and CI drive OpenLocalServer over HTTP on this computer only. Every request needs the token; web pages are refused. Secrets, settings, plugins and running arbitrary programs are never available through it.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {status.error && <p className="text-sm text-warning">{status.error}</p>}
        <div className="grid gap-4 sm:grid-cols-3">
          <Field label="Port">
            <Input type="number" min={1024} max={65535} value={port} onChange={(e) => setPort(e.target.value)} />
          </Field>
          <Field label="What it may do" hint="Read-only can list and look; operate can also start, stop and apply.">
            <Select value={status.settings.mode} onChange={(e) => apply(status.settings.enabled, e.target.value as 'read_only' | 'operate')}>
              <option value="read_only">Read only</option>
              <option value="operate">Operate</option>
            </Select>
          </Field>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="secondary"
            disabled={busy !== null}
            onClick={async () => {
              if (status.token_set && !(await confirmAction('The current token will stop working at once.', 'Make a new token'))) return
              await run('token', async () => {
                const r = await runCommand({ type: 'rotate_api_token' })
                if (r.type === 'text') setToken(r.text)
                await load()
              })
            }}
          >
            <KeyRound /> {status.token_set ? 'New token' : 'Make a token'}
          </Button>
          {status.settings.enabled ? (
            <Button variant="secondary" disabled={busy !== null} onClick={() => apply(false)}>
              Turn off
            </Button>
          ) : (
            <Button disabled={busy !== null || !status.token_set} onClick={() => apply(true)} title={status.token_set ? '' : 'Make a token first'}>
              Turn on
            </Button>
          )}
          {status.settings.enabled && (
            <Button variant="ghost" disabled={busy !== null} onClick={() => apply(true)}>
              Apply port
            </Button>
          )}
        </div>
        {token && (
          <div className="rounded-md border border-warning/40 bg-warning/10 p-3 text-sm">
            <p className="mb-2">Copy this token now. It is not stored and can't be shown again.</p>
            <div className="flex items-center gap-2">
              <code className="min-w-0 flex-1 break-all rounded bg-muted px-2 py-1 text-xs">{token}</code>
              <Button size="sm" variant="secondary" onClick={() => navigator.clipboard.writeText(token)}>
                <Copy /> Copy
              </Button>
            </div>
          </div>
        )}
        <p className="text-xs text-muted-foreground">
          Example: <code>curl -H "Authorization: Bearer TOKEN" -H "Content-Type: application/json" -d "{'{"type":"list_projects"}'}" {status.url}/v1/command</code>
        </p>
      </CardContent>
    </Card>
  )
}

/** §145: signed updates. Nothing is checked until the button is pressed. */
export function UpdatesCard() {
  const [status, setStatus] = useState<UpdaterStatus | null>(null)
  const [update, setUpdate] = useState<UpdateInfo | null>(null)
  const { busy, error, setError, run } = useAction()
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_updater_status' })
    if (r.type === 'updater_status') {
      setStatus(r.status)
      setUpdate(r.status.last)
    }
  }, [])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])
  if (!status) return null
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Download className="size-4" /> Updates
        </CardTitle>
        <CardDescription>
          You have {status.current}. An update is trusted only if its manifest carries a valid signature from the update key, and its installer must match the SHA-256 in that manifest. Nothing is checked, downloaded or installed on its own.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {update && (
          <div className="rounded-md border border-border p-3 text-sm">
            {update.available ? (
              <>
                <div className="font-medium">
                  Version {update.latest} is available <Badge variant="success">signature verified</Badge>
                </div>
                {update.notes && <p className="mt-1 whitespace-pre-wrap text-muted-foreground">{update.notes}</p>}
              </>
            ) : (
              <span>You are up to date.</span>
            )}
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" disabled={busy !== null} onClick={() => run('check', async () => { const r = await runCommand({ type: 'check_update' }); if (r.type === 'update') setUpdate(r.update) })}>
            {busy === 'check' ? <Spinner /> : <RefreshCw />} Check for updates
          </Button>
          {update?.available && !update.downloaded && (
            <Button disabled={busy !== null} onClick={() => run('dl', async () => { const r = await runCommand({ type: 'download_update' }); if (r.type === 'update') setUpdate(r.update) })}>
              {busy === 'dl' ? <Spinner /> : <Download />} Download
            </Button>
          )}
          {update?.downloaded && (
            <Button
              disabled={busy !== null}
              onClick={async () => {
                if (await confirmAction('The installer will start and replace this version. Save your work first.', 'Install the update')) await run('inst', () => runCommand({ type: 'install_update' }))
              }}
            >
              Install now
            </Button>
          )}
          <Button variant="ghost" onClick={() => run('src', () => runCommand({ type: 'open_url', url: 'https://github.com/kz370/OpenLocalServer/releases/latest' }))}>
            <ExternalLink /> Update source
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

/** §124 (Explorer menu) and the support bundle, plus what "privacy" means here. */
export function SystemCard() {
  const [menu, setMenu] = useState<ShellMenuStatus | null>(null)
  const [bundle, setBundle] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  useEffect(() => {
    runCommand({ type: 'get_shell_menu' }).then((r) => r.type === 'shell_menu' && setMenu(r.status))
  }, [])
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ShieldCheck className="size-4" /> Windows integration, support and privacy
        </CardTitle>
        <CardDescription>OpenLocalServer sends nothing anywhere: there is no telemetry and no analytics code in this build.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {menu?.supported && (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-sm">
              <div className="flex items-center gap-2 font-medium">
                <TerminalSquare className="size-4" /> Explorer right-click menu {menu.installed ? <Badge variant="success">installed</Badge> : <Badge variant="outline">off</Badge>}
              </div>
              <p className="text-muted-foreground">Adds "Add to OpenLocalServer" and "Set up with OpenLocalServer" to a folder's menu. Only for your user; no administrator prompt.</p>
            </div>
            <Button variant="secondary" disabled={busy !== null || (!menu.installed && !menu.cli_path)} onClick={() => run('menu', async () => { const r = await runCommand({ type: menu.installed ? 'remove_shell_menu' : 'install_shell_menu' }); if (r.type === 'shell_menu') setMenu(r.status) })}>
              {menu.installed ? 'Remove' : 'Add to Explorer'}
            </Button>
          </div>
        )}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-sm">
            <div className="flex items-center gap-2 font-medium">
              <FileText className="size-4" /> Support bundle
            </div>
            <p className="text-muted-foreground">One zip with the doctor's report, findings, settings and recent logs for a bug report. Secrets are redacted and nothing is sent; read it before you share it.</p>
            {bundle && <p className="mt-1 text-success">Saved: {bundle}</p>}
          </div>
          <Button
            variant="secondary"
            disabled={busy !== null}
            onClick={async () => {
              const dest = await save({ title: 'Save the support bundle', defaultPath: 'openlocalserver-support.zip', filters: [{ name: 'Zip', extensions: ['zip'] }] })
              if (dest) await run('bundle', async () => { await runCommand({ type: 'export_support_bundle', dest }); setBundle(dest) })
            }}
          >
            {busy === 'bundle' ? <Spinner /> : <FileText />} Save bundle
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

/** §128: a small notice in the sidebar while the internet can't be reached. */
export function useOnline(): NetworkStatus | null {
  const [status, setStatus] = useState<NetworkStatus | null>(null)
  useEffect(() => {
    let alive = true
    const check = () => runCommand({ type: 'check_network', force: false }).then((r) => alive && r.type === 'network' && setStatus(r.status)).catch(() => {})
    check()
    const t = setInterval(check, 30000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])
  return status
}

export function OfflineNotice({ status }: { status: NetworkStatus | null }) {
  if (!status || status.online) return null
  return (
    <div className="mx-2 mb-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs" title={status.needs_internet.join('\n')}>
      <div className="flex items-center gap-1.5 font-medium text-warning">
        <Wifi className="size-3.5" /> No internet
      </div>
      <div className="mt-0.5 text-sidebar-foreground/70">Downloads, tunnels and updates won't work. Everything local does.</div>
    </div>
  )
}
