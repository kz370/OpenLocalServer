import { open } from '@tauri-apps/plugin-dialog'
import { FolderSearch, ShieldCheck } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { ApiCard, SystemCard, UpdatesCard } from '@/components/ReleaseCards'
import { ResourcesCard, SettingsBackupsCard } from '@/components/SettingsExtras'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type EditorInfo, type ServiceStatus, type StartupSettings, runCommand } from '@/core'
import { confirmThen } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

async function getString(key: string): Promise<string> {
  const r = await runCommand({ type: 'get_setting', key })
  return r.type === 'setting' && typeof r.value === 'string' ? r.value : ''
}

/** §119–121 startup, tray and notification behavior, plus the everyday defaults. */
export function SettingsPage() {
  const [startup, setStartup] = useState<StartupSettings | null>(null)
  const [services, setServices] = useState<ServiceStatus[]>([])
  const [editor, setEditor] = useState('')
  const [editorId, setEditorId] = useState('vscode')
  const [editors, setEditors] = useState<EditorInfo[]>([])
  const [projectsDir, setProjectsDir] = useState('')
  const [autoDomains, setAutoDomains] = useState(true)
  const [helper, setHelper] = useState<boolean | null>(null)
  const [version, setVersion] = useState('')
  const [saved, setSaved] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'get_startup_settings' }).then((r) => r.type === 'startup' && setStartup(r.settings))
    runCommand({ type: 'list_services' }).then((r) => r.type === 'services' && setServices(r.services))
    runCommand({ type: 'ping' }).then((r) => r.type === 'pong' && setVersion(r.version))
    runCommand({ type: 'list_editors' }).then((r) => r.type === 'editors' && setEditors(r.editors))
    runCommand({ type: 'get_helper_service' }).then((r) => r.type === 'helper_service' && setHelper(r.installed))
    void Promise.all([getString('editor'), getString('editor.command')]).then(([id, cmd]) => {
      setEditor(cmd)
      setEditorId(cmd ? 'custom' : id || 'vscode')
    })
    void getString('quickapps.projects_dir').then(setProjectsDir)
    void runCommand({ type: 'get_setting', key: 'domains.auto' }).then((r) => r.type === 'setting' && setAutoDomains(r.value !== false))
  }, [])

  async function saveAll() {
    if (!startup) return
    const r = await runCommand({ type: 'set_startup_settings', settings: startup })
    if (r.type === 'startup') setStartup(r.settings)
    await runCommand({ type: 'set_setting', key: 'editor', value: editorId === 'custom' ? '' : editorId })
    await runCommand({ type: 'set_setting', key: 'editor.command', value: editorId === 'custom' ? editor : '' })
    await runCommand({ type: 'set_setting', key: 'quickapps.projects_dir', value: projectsDir })
    await runCommand({ type: 'set_setting', key: 'domains.auto', value: autoDomains })
    if (autoDomains) await runCommand({ type: 'sync_auto_domains' })
    setSaved('Saved.')
    setTimeout(() => setSaved(null), 2500)
  }

  if (!startup) return null
  const set = (patch: Partial<StartupSettings>) => setStartup({ ...startup, ...patch })
  const toggleService = (id: string, on: boolean) =>
    set({ autostart_services: on ? [...startup.autostart_services, id] : startup.autostart_services.filter((s) => s !== id) })

  return (
    <div className="flex max-w-3xl flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">OpenLocalServer {version && `v${version}`}</p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Startup and tray</CardTitle>
          <CardDescription>Servers keep running from the system tray after you close the window.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Toggle checked={startup.with_windows} onChange={(v) => set({ with_windows: v })} label="Start with Windows" hint="Adds OpenLocalServer to your account's startup list (no administrator rights needed)." />
          <Toggle checked={startup.start_minimized} onChange={(v) => set({ start_minimized: v })} label="Start minimized to the tray" hint="Only applies when it was started automatically with Windows." />
          <Toggle checked={startup.close_to_tray} onChange={(v) => set({ close_to_tray: v })} label="Closing the window keeps running in the tray" hint="Use Quit in the tray menu to stop everything." />
          <Toggle checked={startup.autostart_web} onChange={(v) => set({ autostart_web: v })} label="Start the web server and your sites when the app starts" />
          <div>
            <div className="mb-1.5 text-xs font-medium text-muted-foreground">Start these services automatically</div>
            <div className="flex flex-wrap gap-4">
              {services.filter((s) => s.installed).map((s) => (
                <Toggle key={s.id} checked={startup.autostart_services.includes(s.id)} onChange={(v) => toggleService(s.id, v)} label={s.name} />
              ))}
              {services.every((s) => !s.installed) && <span className="text-sm text-muted-foreground">No services installed yet.</span>}
            </div>
          </div>
          <Toggle checked={startup.notifications} onChange={(v) => set({ notifications: v })} label="Desktop notifications" hint="When a process crashes, an install finishes, or a Quick App completes." />
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Defaults</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <Field label="Where new projects are created" hint="Quick Apps default to this folder. Leave blank for Sites in your user folder.">
            <div className="flex gap-2">
              <Input value={projectsDir} onChange={(e) => setProjectsDir(e.target.value)} placeholder="C:\Users\you\Sites" />
              <Button variant="secondary" onClick={async () => { const p = await open({ directory: true }); if (p && !Array.isArray(p)) setProjectsDir(p) }}>
                <FolderSearch /> Browse
              </Button>
            </div>
          </Field>
          <Field
            label="Administrator helper"
            hint="Windows only lets administrators change how domain names resolve. The helper is a small background service that does it for the app, so Windows asks you once, when it's installed, instead of on every change."
          >
            <div className="flex items-center gap-3">
              {helper === null ? (
                <Badge variant="secondary">Checking…</Badge>
              ) : helper ? (
                <Badge variant="success">Installed · no more prompts</Badge>
              ) : (
                <Badge variant="outline">Not installed</Badge>
              )}
              {helper === false && (
                <Button size="sm" disabled={busy !== null} onClick={() => run('helper', async () => { const r = await runCommand({ type: 'install_helper_service' }); if (r.type === 'helper_service') setHelper(r.installed) })}>
                  {busy === 'helper' ? <Spinner /> : <ShieldCheck />} Install (asks once)
                </Button>
              )}
              {helper && (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => confirmThen('Remove the administrator helper?\nWindows will ask for approval again on every domain change.', () => run('helper', async () => { const r = await runCommand({ type: 'uninstall_helper_service' }); if (r.type === 'helper_service') setHelper(r.installed) }))}
                >
                  {busy === 'helper' ? <Spinner /> : null} Remove
                </Button>
              )}
            </div>
          </Field>
          <Toggle
            checked={autoDomains}
            onChange={setAutoDomains}
            label="Create domains automatically"
            hint="Like Laragon: every folder in a scanned projects folder gets <folder>.test with HTTPS. Domains you delete stay deleted."
          />
          <Field label="Code editor" hint="Sites, projects and config files open here. Only editors installed on this PC are listed.">
            <div className="flex flex-col gap-2">
              <Select value={editorId} onChange={(e) => setEditorId(e.target.value)} className="w-72">
                {editors
                  .filter((e) => e.path)
                  .map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.name}
                      {e.id === 'vscode' ? ' (default)' : ''}
                    </option>
                  ))}
                {editorId !== 'custom' && !editors.some((e) => e.id === editorId && e.path) && (
                  <option value={editorId}>{editors.find((e) => e.id === editorId)?.name ?? editorId} (not found)</option>
                )}
                <option value="custom">Another program…</option>
              </Select>
              {editorId === 'custom' && (
                <div className="flex gap-2">
                  <Input value={editor} onChange={(e) => setEditor(e.target.value)} placeholder="C:\Program Files\JetBrains\PhpStorm\bin\phpstorm64.exe" />
                  <Button variant="secondary" onClick={async () => { const p = await open({ filters: [{ name: 'Program', extensions: ['exe', 'cmd', 'bat'] }] }); if (p && !Array.isArray(p)) setEditor(p) }}>
                    <FolderSearch /> Browse
                  </Button>
                </div>
              )}
            </div>
          </Field>
        </CardContent>
      </Card>

      <div className="flex items-center gap-3">
        <Button disabled={busy !== null} onClick={() => run('save', saveAll)}>Save settings</Button>
        {saved && <span className="text-sm text-success">{saved}</span>}
      </div>

      <ResourcesCard />
      <SettingsBackupsCard />
      <UpdatesCard />
      <ApiCard />
      <SystemCard />
    </div>
  )
}
