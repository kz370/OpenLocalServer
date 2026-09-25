import { open } from '@tauri-apps/plugin-dialog'
import { FolderSearch } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type ServiceStatus, type StartupSettings, runCommand } from '@/core'
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
  const [projectsDir, setProjectsDir] = useState('')
  const [version, setVersion] = useState('')
  const [saved, setSaved] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'get_startup_settings' }).then((r) => r.type === 'startup' && setStartup(r.settings))
    runCommand({ type: 'list_services' }).then((r) => r.type === 'services' && setServices(r.services))
    runCommand({ type: 'ping' }).then((r) => r.type === 'pong' && setVersion(r.version))
    void getString('editor.command').then(setEditor)
    void getString('quickapps.projects_dir').then(setProjectsDir)
  }, [])

  async function saveAll() {
    if (!startup) return
    const r = await runCommand({ type: 'set_startup_settings', settings: startup })
    if (r.type === 'startup') setStartup(r.settings)
    await runCommand({ type: 'set_setting', key: 'editor.command', value: editor })
    await runCommand({ type: 'set_setting', key: 'quickapps.projects_dir', value: projectsDir })
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
          <Field label="Code editor" hint="Full path to your editor (or a .cmd shim like code.cmd). Blank uses VS Code if found, else Notepad.">
            <div className="flex gap-2">
              <Input value={editor} onChange={(e) => setEditor(e.target.value)} placeholder="C:\Program Files\Notepad++\notepad++.exe" />
              <Button variant="secondary" onClick={async () => { const p = await open({ filters: [{ name: 'Program', extensions: ['exe', 'cmd', 'bat'] }] }); if (p && !Array.isArray(p)) setEditor(p) }}>
                <FolderSearch /> Browse
              </Button>
            </div>
          </Field>
        </CardContent>
      </Card>

      <div className="flex items-center gap-3">
        <Button disabled={busy !== null} onClick={() => run('save', saveAll)}>Save settings</Button>
        {saved && <span className="text-sm text-success">{saved}</span>}
      </div>
    </div>
  )
}
