import { save } from '@tauri-apps/plugin-dialog'
import { ArrowDownToLine, BookOpen, CheckCircle, Copy, Download, ExternalLink, FileText, KeyRound, RefreshCw, Rocket, Settings2, ShieldCheck, TerminalSquare, Wifi } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Select } from '@/components/ui/form'
import { Input, NumberInput } from '@/components/ui/input'
import { Markdown } from '@/components/ui/Markdown'
import { type ApiStatus, type NetworkStatus, type ShellMenuStatus, type UpdateInfo, type UpdaterStatus, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

/** Where the prose lives. The card states what the API is and how to switch it on; the
 * endpoint contract, the mode lists and the security rules are documented, because a
 * settings card is the wrong place for a reference and a wall of text there is a wall
 * of text every user has to scroll past. */
const DOCS_URL = 'https://github.com/kz370/OpenLocalServer/blob/main/docs/API.md'

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
          Drives OLS over HTTP from a script or a CI job, with no desktop window open. Off by default, and on 127.0.0.1 only.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {status.error && <p className="text-sm text-warning">{status.error}</p>}
        <div className="grid gap-4 sm:grid-cols-3">
          <Field label="Port" hint="Loopback only. Changing it needs Apply port.">
            <NumberInput
              label="API port"
              min={1024}
              max={65535}
              value={port === '' ? null : Number(port)}
              onChange={(n) => setPort(n === null ? '' : String(n))}
            />
          </Field>
          <Field
            label="What it may do"
            hint="Read only answers questions; operate also starts, stops, applies and installs. Nothing else is ever reachable."
          >
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
            <Button
              disabled={busy !== null || !status.token_set}
              onClick={() => apply(true)}
              title={status.token_set ? '' : 'Make a token first — the API cannot start without one'}
            >
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
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[13px] text-muted-foreground">
          <span>
            One endpoint: <code className="font-mono text-xs">POST {status.url}/v1/command</code>, same JSON the CLI and this app use.
          </span>
          <Button
            variant="link"
            className="h-auto gap-1 p-0 text-[13px] font-normal"
            onClick={() => runCommand({ type: 'open_url', url: DOCS_URL }).catch(setError)}
          >
            <BookOpen aria-hidden="true" className="size-3.5" /> Read the docs
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

/** §145 (Stage 17) — Signed update check, download and install.
 *  The backend fetches and verifies the minisign manifest; nothing is downloaded or run
 *  unless the signature matches. The public key must be compiled in (OLS_UPDATE_PUBKEY)
 *  or entered in Settings for a check to succeed. */
export function UpdatesCard() {
  const isDev = import.meta.env.DEV
  const DEFAULT_ENDPOINT = 'https://github.com/kz370/OpenLocalServer/releases/latest/download/latest.json'

  const [status, setStatus] = useState<UpdaterStatus | null>(null)
  const [info, setInfo] = useState<UpdateInfo | null>(null)
  const [checking, setChecking] = useState(false)
  const [lastCheckResult, setLastCheckResult] = useState<'checked' | null>(null)
  const [showConfig, setShowConfig] = useState(false)
  const [endpointInput, setEndpointInput] = useState('')
  const [publicKeyInput, setPublicKeyInput] = useState('')
  const { busy, error, setError, run } = useAction()

  const RELEASES_URL = 'https://github.com/kz370/OpenLocalServer/releases/latest'

  // Load the last known state on mount.
  useEffect(() => {
    runCommand({ type: 'get_updater_status' })
      .then((r) => {
        if (r.type === 'updater_status') {
          setStatus(r.status)
          setEndpointInput(r.status.settings.endpoint || DEFAULT_ENDPOINT)
          setPublicKeyInput(r.status.settings.public_key || '')
          setInfo(r.status.last ?? null)
        }
      })
      .catch(setError)
  }, [setError])

  const checkForUpdates = () =>
    run('check', async () => {
      setError(null)
      setChecking(true)
      setLastCheckResult(null)
      try {
        const r = await runCommand({ type: 'check_update' })
        if (r.type === 'update') {
          setInfo(r.update)
          setLastCheckResult('checked')
        }
      } finally {
        setChecking(false)
      }
    })

  const downloadUpdate = () =>
    run('download', async () => {
      setError(null)
      const r = await runCommand({ type: 'download_update' })
      if (r.type === 'update') setInfo(r.update)
    })

  const installUpdate = async () => {
    const ok = await confirmAction(
      'The installer will start and this window will close. Save any unsaved work first.',
      'Install update now?',
      'Install',
    )
    if (!ok) return
    await run('install', async () => {
      await runCommand({ type: 'install_update' })
    })
  }

  const saveSettings = () =>
    run('save_settings', async () => {
      setError(null)
      const r = await runCommand({
        type: 'set_updater_settings',
        endpoint: endpointInput.trim() || DEFAULT_ENDPOINT,
        public_key: publicKeyInput.trim(),
      })
      if (r.type === 'updater_status') {
        setStatus(r.status)
        setEndpointInput(r.status.settings.endpoint)
        setPublicKeyInput(r.status.settings.public_key)
        setInfo(r.status.last ?? null)
        setShowConfig(false)
      }
    })

  const resetSettings = () =>
    run('save_settings', async () => {
      setError(null)
      const r = await runCommand({
        type: 'set_updater_settings',
        endpoint: DEFAULT_ENDPOINT,
        public_key: '',
      })
      if (r.type === 'updater_status') {
        setStatus(r.status)
        setEndpointInput(DEFAULT_ENDPOINT)
        setPublicKeyInput('')
        setInfo(null)
        setShowConfig(false)
      }
    })

  const fmt = (bytes: number) => {
    if (bytes === 0) return ''
    const mb = bytes / 1024 / 1024
    return mb >= 1 ? ` · ${mb.toFixed(1)} MB` : ` · ${(bytes / 1024).toFixed(0)} KB`
  }

  const hasUpdate = info?.status === 'update_available'
  const isDownloaded = !!info?.downloaded

  // Friendly error message instead of raw crypto / internal network errors
  const displayError = error
    ? {
      problem: 'Unable to check for updates right now.',
      cause: 'There was a problem contacting the update service or verifying the release files.',
      fix: 'Please check your internet connection, or download the installer manually from the GitHub Releases page.',
    }
    : null

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <Download className="size-4" /> Updates
          {isDownloaded ? (
            <Badge variant="success">Ready to install</Badge>
          ) : info?.status === 'development_version' ? (
            <Badge variant="destructive">Development version</Badge>
          ) : info?.status === 'update_available' ? (
            <Badge variant="warning">Update available</Badge>
          ) : info?.status === 'up_to_date' ? (
            <Badge variant="success">Up to date</Badge>
          ) : null}
        </CardTitle>
        <CardDescription>
          {status ? `Current version: v${status.current}` : 'New versions, installers and checksums are published on the releases page.'}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={displayError} onDismiss={() => setError(null)} />

        {/* Checking state — visible while the backend fetches and verifies the manifest */}
        {checking && (
          <div className="rounded-lg border border-info/40 bg-info/10 p-3.5 text-sm flex items-center gap-2">
            <RefreshCw className="size-4 animate-spin shrink-0" />
            <span>Checking for updates…</span>
          </div>
        )}

        {/* Last check result — persists after the check finishes so the user sees feedback */}
        {!checking && lastCheckResult === 'checked' && info && (
          <div className="rounded-lg border border-info/40 bg-info/10 p-3.5 text-sm flex items-center gap-2">
            <CheckCircle className="size-4 shrink-0 text-info" />
            <span>
              Checked: v{info.latest} — {info.status.replace('_', ' ')}
            </span>
          </div>
        )}

        {/* When downloaded and verified: show ONE unified ready banner */}
        {isDownloaded && info && (
          <div className="rounded-lg border border-success/40 bg-success/10 p-3.5 text-sm">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <CheckCircle className="size-4 shrink-0 text-success" />
                <span className="font-semibold text-success">
                  v{info.latest} is verified and ready to install
                </span>
                <span className="text-xs text-muted-foreground">{fmt(info.size)}</span>
              </div>
              {info.date && <span className="text-xs text-muted-foreground">Released {info.date}</span>}
            </div>
            {info.notes && <Markdown content={info.notes} className="mt-2 text-xs leading-relaxed text-foreground/80" />}
          </div>
        )}

        {/* When update available but NOT yet downloaded: show ONE available banner */}
        {hasUpdate && !isDownloaded && info && (
          <div className="rounded-lg border border-warning/40 bg-warning/10 p-3.5 text-sm">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <ArrowDownToLine className="size-4 shrink-0 text-warning" />
                <span className="font-semibold text-warning">v{info.latest} is available</span>
                <span className="text-xs text-muted-foreground">{fmt(info.size)}</span>
              </div>
              {info.date && <span className="text-xs text-muted-foreground">Released {info.date}</span>}
            </div>
            {info.notes && <Markdown content={info.notes} className="mt-2 text-xs leading-relaxed text-foreground/80" />}
          </div>
        )}

        {/* Up-to-date banner (only when checked and no update is available) */}
        {info?.status === 'up_to_date' && !isDownloaded && (
          <div className="flex items-center gap-2 text-sm text-success">
            <CheckCircle className="size-4" />
            You are on the latest version (v{info.latest}).
          </div>
        )}

        {/* Source configuration panel — development only */}
        {isDev && showConfig && (
          <div className="flex flex-col gap-3 rounded-lg border border-border/70 bg-accent/20 p-3.5 text-sm">
            <div className="flex items-center justify-between">
              <span className="font-medium text-foreground">Update Source Settings (Dev Only)</span>
              <Button
                variant="ghost"
                size="sm"
                onClick={resetSettings}
                disabled={busy !== null}
              >
                Reset to default
              </Button>
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs text-muted-foreground" htmlFor="updater-endpoint">
                Update Manifest URL (HTTPS)
              </label>
              <Input
                id="updater-endpoint"
                value={endpointInput}
                onChange={(e) => setEndpointInput(e.target.value)}
                placeholder="https://..."
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label className="text-xs text-muted-foreground" htmlFor="updater-pubkey">
                Minisign Public Key (leave empty to use built-in app key)
              </label>
              <Input
                id="updater-pubkey"
                value={publicKeyInput}
                onChange={(e) => setPublicKeyInput(e.target.value)}
                placeholder="Using built-in official key (leave empty)"
              />
            </div>
            <div className="flex justify-end gap-2 pt-1">
              <Button size="sm" variant="secondary" onClick={() => setShowConfig(false)}>
                Cancel
              </Button>
              <Button size="sm" onClick={saveSettings} disabled={busy !== null}>
                {busy === 'save_settings' ? <Spinner /> : null} Save settings
              </Button>
            </div>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          {/* Check */}
          <Button
            variant="secondary"
            disabled={busy !== null}
            onClick={checkForUpdates}
            id="check-for-updates-btn"
          >
            {busy === 'check' ? <Spinner /> : <RefreshCw />}
            Check for updates
          </Button>

          {/* Download — only when an update is available and not yet downloaded */}
          {hasUpdate && !isDownloaded && (
            <Button
              variant="secondary"
              disabled={busy !== null}
              onClick={downloadUpdate}
              id="download-update-btn"
            >
              {busy === 'download' ? <Spinner /> : <ArrowDownToLine />}
              Download update
            </Button>
          )}

          {/* Install — only when the installer is ready */}
          {isDownloaded && (
            <Button
              disabled={busy !== null}
              onClick={installUpdate}
              id="install-update-btn"
            >
              {busy === 'install' ? <Spinner /> : <Rocket />}
              Install now
            </Button>
          )}

          {/* Configure source toggle — development only */}
          {isDev && (
            <Button
              variant="ghost"
              onClick={() => setShowConfig((v) => !v)}
              id="configure-update-source-btn"
            >
              <Settings2 className="size-4" /> {showConfig ? 'Hide source' : 'Configure source'}
            </Button>
          )}

          {/* Always offer a direct link */}
          <Button
            variant="ghost"
            onClick={() => runCommand({ type: 'open_url', url: RELEASES_URL }).catch(() => undefined)}
          >
            <ExternalLink /> Releases page
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
        <CardDescription>OLS sends nothing anywhere: there is no telemetry and no analytics code in this build.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {menu?.supported && (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-sm">
              <div className="flex items-center gap-2 font-medium">
                <TerminalSquare className="size-4" /> Explorer right-click menu {menu.installed ? <Badge variant="success">installed</Badge> : <Badge variant="outline">off</Badge>}
              </div>
              <p className="text-muted-foreground">Adds "Add to OLS" and "Set up with OLS" to a folder's menu. Only for your user; no administrator prompt.</p>
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
              const dest = await save({ title: 'Save the support bundle', defaultPath: 'ols-support.zip', filters: [{ name: 'Zip', extensions: ['zip'] }] })
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
    const check = () => runCommand({ type: 'check_network', force: false }).then((r) => alive && r.type === 'network' && setStatus(r.status)).catch(() => { })
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
