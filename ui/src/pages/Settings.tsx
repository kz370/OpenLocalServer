import { open } from '@tauri-apps/plugin-dialog'
import { Archive, ExternalLink, FolderPlus, FolderSearch, FolderTree, Gauge, Globe, History, Info, Power, Settings2, ShieldCheck, Sparkles, Stethoscope, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { AiCard } from '@/components/ai/AiSettings'
import { ErrorCard } from '@/components/ErrorCard'
import { ExcludedSitesCard } from '@/components/ExcludedSitesCard'
import { ApiCard, SystemCard, UpdatesCard } from '@/components/ReleaseCards'
import { AboutCard, AutoBackupCard, ResourcesCard, SettingsBackupsCard } from '@/components/SettingsExtras'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Select, SettingRow, SwitchRow, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type EditorInfo, type HelperService, type ServiceStatus, type StartupSettings, runCommand } from '@/core'
import { confirmThen } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'
import { invalidateDefaultTldCache } from '@/lib/sites'
import { cn } from '@/lib/utils'

type Section = 'general' | 'sites' | 'roots' | 'deleted' | 'startup' | 'diagnostics' | 'ai' | 'resources' | 'backups' | 'about'
const SECTIONS: { id: Section; label: string; icon: typeof Globe }[] = [
  { id: 'general', label: 'General', icon: Settings2 },
  { id: 'sites', label: 'Sites & domains', icon: Globe },
  { id: 'roots', label: 'Root folders', icon: FolderTree },
  { id: 'deleted', label: 'Excluded sites', icon: History },
  { id: 'startup', label: 'Startup & tray', icon: Power },
  { id: 'diagnostics', label: 'Diagnostics', icon: Stethoscope },
  { id: 'ai', label: 'AI', icon: Sparkles },
  { id: 'resources', label: 'Resources', icon: Gauge },
  { id: 'backups', label: 'Backups', icon: Archive },
  { id: 'about', label: 'About', icon: Info },
]

async function getString(key: string): Promise<string> {
  const r = await runCommand({ type: 'get_setting', key })
  return r.type === 'setting' && typeof r.value === 'string' ? r.value : ''
}

async function getStringList(key: string): Promise<string[]> {
  const r = await runCommand({ type: 'get_setting', key })
  return r.type === 'setting' && Array.isArray(r.value) ? r.value.filter((value): value is string => typeof value === 'string') : []
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
  const [watchSites, setWatchSites] = useState(true)
  const [autoFixDiagnostics, setAutoFixDiagnostics] = useState(true)
  const [sitesDir, setSitesDir] = useState('')
  const [rootFolders, setRootFolders] = useState<string[]>([])
  const [defaultTld, setDefaultTld] = useState('local')
  const [customTld, setCustomTld] = useState('')
  const [helper, setHelper] = useState<HelperService | null>(null)
  const [version, setVersion] = useState('')
  const [saved, setSaved] = useState<string | null>(null)
  const [section, setSection] = useState<Section>('general')
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    runCommand({ type: 'get_startup_settings' }).then((r) => r.type === 'startup' && setStartup(r.settings))
    runCommand({ type: 'list_services' }).then((r) => r.type === 'services' && setServices(r.services))
    runCommand({ type: 'ping' }).then((r) => r.type === 'pong' && setVersion(r.version))
    runCommand({ type: 'list_editors' }).then((r) => r.type === 'editors' && setEditors(r.editors))
    runCommand({ type: 'get_helper_service' }).then((r) => r.type === 'helper_service' && setHelper(r))
    void Promise.all([getString('editor'), getString('editor.command')]).then(([id, cmd]) => {
      setEditor(cmd)
      setEditorId(cmd ? 'custom' : id || 'vscode')
    })
    void getString('quickapps.projects_dir').then(setProjectsDir)
    void getString('paths.sites_dir').then(setSitesDir)
    void getStringList('projects.roots').then(setRootFolders)
    void runCommand({ type: 'get_setting', key: 'projects.watch' }).then((r) => r.type === 'setting' && setWatchSites(r.value !== false))
    void runCommand({ type: 'get_setting', key: 'domains.auto' }).then((r) => r.type === 'setting' && setAutoDomains(r.value !== false))
    void runCommand({ type: 'get_setting', key: 'diagnostics.auto_fix' }).then((r) => r.type === 'setting' && setAutoFixDiagnostics(r.value !== false))
    void getString('domains.default_tld').then((val) => {
      const tld = val.trim().replace(/^\./, '') || 'local'
      const presets = ['local', 'test', 'localhost']
      if (presets.includes(tld)) {
        setDefaultTld(tld)
        setCustomTld('')
      } else {
        setDefaultTld('custom')
        setCustomTld(tld)
      }
    })
  }, [])

  async function saveAll() {
    if (!startup) return
    const r = await runCommand({ type: 'set_startup_settings', settings: startup })
    if (r.type === 'startup') setStartup(r.settings)
    await runCommand({ type: 'set_setting', key: 'editor', value: editorId === 'custom' ? '' : editorId })
    await runCommand({ type: 'set_setting', key: 'editor.command', value: editorId === 'custom' ? editor : '' })
    await runCommand({ type: 'set_setting', key: 'quickapps.projects_dir', value: projectsDir })
    await runCommand({ type: 'set_setting', key: 'domains.auto', value: autoDomains })
    await runCommand({ type: 'set_setting', key: 'projects.watch', value: watchSites })
    const tldValue = defaultTld === 'custom' ? customTld.trim().replace(/^\./, '') : defaultTld
    await runCommand({ type: 'set_setting', key: 'domains.default_tld', value: tldValue || 'local' })
    invalidateDefaultTldCache()
    if (autoDomains) await runCommand({ type: 'sync_auto_domains' })
    setSaved('Saved.')
    setTimeout(() => setSaved(null), 2500)
  }

  async function addRootFolder() {
    const picked = await open({ directory: true, title: 'Select a root projects folder' })
    if (!picked || Array.isArray(picked) || rootFolders.some((root) => root.toLowerCase() === picked.toLowerCase())) return
    const next = [...rootFolders, picked]
    await run('add root folder', async () => {
      const result = await runCommand({ type: 'set_setting', key: 'projects.roots', value: next })
      if (result.type === 'ok') {
        setRootFolders(next)
        if (autoDomains) await runCommand({ type: 'sync_auto_domains' })
      }
    })
  }

  async function removeRootFolder(root: string) {
    const next = rootFolders.filter((folder) => folder !== root)
    await run('remove root folder', async () => {
      const result = await runCommand({ type: 'set_setting', key: 'projects.roots', value: next })
      if (result.type === 'ok') setRootFolders(next)
    })
  }

  if (!startup) return null
  const set = (patch: Partial<StartupSettings>) => setStartup({ ...startup, ...patch })
  const toggleService = (id: string, on: boolean) =>
    set({ autostart_services: on ? [...startup.autostart_services, id] : startup.autostart_services.filter((s) => s !== id) })

  const editorField = (
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
  )

  const helperField = (
    <div className="flex items-center gap-3">
      {helper === null ? (
        <Badge variant="secondary">Checking…</Badge>
      ) : helper.outdated ? (
        <Badge variant="warning">Installed · version {helper.version} (app is {version})</Badge>
      ) : helper.installed ? (
        <Badge variant="success">Installed · no more prompts</Badge>
      ) : (
        <Badge variant="outline">Not installed</Badge>
      )}
      {/* The service runs a *copy* of the helper, and nothing refreshes that copy on an
          update or a move, so an out-of-date one is offered the same reinstall as a
          missing one — it is the same fix and the same single prompt. */}
      {helper !== null && (!helper.installed || helper.outdated) && (
        <Button size="sm" disabled={busy !== null} onClick={() => run('helper', async () => { const r = await runCommand({ type: 'install_helper_service' }); if (r.type === 'helper_service') setHelper(r) })}>
          {busy === 'helper' ? <Spinner /> : <ShieldCheck />} {helper.outdated ? 'Update (asks once)' : 'Install (asks once)'}
        </Button>
      )}
      {helper?.installed && (
        <Button
          size="sm"
          variant="ghost"
          disabled={busy !== null}
          onClick={() => confirmThen('Remove the administrator helper?\nWindows will ask for approval again on every domain change.', () => run('helper', async () => { const r = await runCommand({ type: 'uninstall_helper_service' }); if (r.type === 'helper_service') setHelper(r) }))}
        >
          {busy === 'helper' ? <Spinner /> : null} Remove
        </Button>
      )}
    </div>
  )

  const showSave = section === 'general' || section === 'sites' || section === 'startup'

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">OLS {version && `v${version}`}</p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="flex flex-col gap-6 md:flex-row md:items-start">
        <nav className="flex shrink-0 gap-0.5 overflow-x-auto md:sticky md:top-0 md:w-52 md:flex-col" aria-label="Settings sections">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              onClick={() => setSection(s.id)}
              className={cn(
                'flex items-center gap-2.5 whitespace-nowrap rounded-md px-3 py-2 text-left text-sm font-medium transition-colors',
                section === s.id ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
              )}
            >
              <s.icon className="size-4 shrink-0" />
              {s.label}
            </button>
          ))}
        </nav>

        <div className="flex min-w-0 max-w-3xl flex-1 flex-col gap-5">
          {section === 'general' && (
            <>
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">Defaults</CardTitle>
                </CardHeader>
                <CardContent>
                  <SettingRow title="Code editor" hint="Sites, projects and config files open here. Only editors installed on this PC are listed.">
                    {editorField}
                  </SettingRow>
                  <SettingRow
                    title="Administrator helper"
                    hint="Windows only lets administrators change how domain names resolve. The helper is a small background service that does it for the app, so Windows asks you once, when it's installed, instead of on every change."
                  >
                    {helperField}
                  </SettingRow>
                </CardContent>
              </Card>
              <UpdatesCard />
              <ApiCard />
              <SystemCard />
            </>
          )}

          {section === 'sites' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Sites and domains</CardTitle>
              </CardHeader>
              <CardContent>
                <SettingRow stacked title="Default sites folder" hint="New projects use this folder when no custom location is set.">
                  <div className="flex gap-2">
                    <Input value={sitesDir} readOnly />
                    <Button variant="secondary" onClick={() => run('open', () => runCommand({ type: 'open_path', path: sitesDir }))}>
                      <ExternalLink /> Open
                    </Button>
                  </div>
                </SettingRow>
                <SettingRow stacked title="Where new projects are created" hint={`Quick Apps, Git clone and imports default here. Leave blank to use ${sitesDir || '<install>\\sites'}. Each dialog can still change folder name or location.`}>
                  <div className="flex gap-2">
                    {/* The blank case is the common one, and it resolves to the real
                        installed sites folder — the row above already shows it — so that
                        is what the placeholder names, not a sample user path. */}
                    <Input value={projectsDir} onChange={(e) => setProjectsDir(e.target.value)} placeholder={sitesDir || undefined} />
                    <Button variant="secondary" onClick={async () => { const p = await open({ directory: true }); if (p && !Array.isArray(p)) setProjectsDir(p) }}>
                      <FolderSearch /> Browse
                    </Button>
                  </div>
                </SettingRow>
                <SwitchRow
                  checked={autoDomains}
                  onChange={setAutoDomains}
                  title="Create domains automatically"
                  hint="Like Laragon: every folder in a scanned projects folder gets <folder>.<tld> with HTTPS. Domains you delete stay deleted."
                />
                <SettingRow
                  title="Default top-level domain"
                  hint={`New sites and auto-created domains use this TLD (e.g. shop.${defaultTld === 'custom' ? (customTld || 'local') : defaultTld}). Existing sites are not changed.`}
                >
                  <div className="flex flex-col gap-2">
                    <Select
                      value={defaultTld}
                      onChange={(e) => {
                        setDefaultTld(e.target.value)
                        if (e.target.value !== 'custom') setCustomTld('')
                      }}
                      className="w-48"
                    >
                      <option value="local">.local</option>
                      <option value="test">.test</option>
                      <option value="localhost">.localhost</option>
                      <option value="custom">Custom…</option>
                    </Select>
                    {defaultTld === 'custom' && (
                      <div className="flex items-center gap-2">
                        <span className="text-sm text-muted-foreground">.</span>
                        <Input
                          value={customTld}
                          onChange={(e) => setCustomTld(e.target.value.replace(/^\./, '').replace(/\s/g, ''))}
                          placeholder="mycompany"
                          className="w-40"
                        />
                      </div>
                    )}
                  </div>
                </SettingRow>
                <SwitchRow checked={watchSites} onChange={setWatchSites} title="Watch sites folder" hint="New and removed project folders are detected automatically." />
              </CardContent>
            </Card>
          )}

          {section === 'roots' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Root folders</CardTitle>
                <CardDescription>Only these folders are watched for immediate child project folders. Projects inside them are detected automatically.</CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-3">
                <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
                  <div className="min-w-0">
                    <p className="text-sm font-medium">Default sites folder</p>
                    <p className="truncate text-xs text-muted-foreground">{sitesDir}</p>
                  </div>
                  <Badge variant="secondary">Default</Badge>
                </div>
                {rootFolders.filter((root) => root.toLowerCase() !== sitesDir.toLowerCase()).map((root) => (
                  <div key={root} className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
                    <p className="min-w-0 truncate text-sm">{root}</p>
                    <Button size="icon" variant="ghost" title="Remove root folder" aria-label={`Remove ${root}`} onClick={() => removeRootFolder(root)}>
                      <Trash2 />
                    </Button>
                  </div>
                ))}
                <Button variant="secondary" className="self-start" onClick={addRootFolder}>
                  <FolderPlus /> Add root folder
                </Button>
              </CardContent>
            </Card>
          )}

          {section === 'deleted' && <ExcludedSitesCard />}

          {section === 'startup' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Startup and tray</CardTitle>
                <CardDescription>Servers keep running from the system tray after you close the window.</CardDescription>
              </CardHeader>
              <CardContent>
                <SwitchRow checked={startup.with_windows} onChange={(v) => set({ with_windows: v })} title="Start with Windows" hint="Adds OLS to your account's startup list (no administrator rights needed)." />
                <SwitchRow checked={startup.start_minimized} onChange={(v) => set({ start_minimized: v })} title="Start minimized to the tray" hint="Every launch, whether you open it yourself or Windows starts it with your account." />
                <SwitchRow checked={startup.close_to_tray} onChange={(v) => set({ close_to_tray: v })} title="Closing the window keeps running in the tray" hint="Use Quit in the tray menu to stop everything." />
                <SwitchRow checked={startup.notifications} onChange={(v) => set({ notifications: v })} title="Desktop notifications" hint="When a process crashes, an install finishes, or a Quick App completes." />
                <SettingRow stacked title="Start these services automatically" hint="A web server listed here has its config applied before it starts, so your sites answer as soon as the app opens.">
                  <div className="flex flex-col gap-2.5">
                    {services.filter((s) => s.installed).map((s) => (
                      <Toggle key={s.id} checked={startup.autostart_services.includes(s.id)} onChange={(v) => toggleService(s.id, v)} label={s.name} />
                    ))}
                    {services.every((s) => !s.installed) && <span className="text-sm text-muted-foreground">No services installed yet.</span>}
                  </div>
                </SettingRow>
              </CardContent>
            </Card>
          )}

          {section === 'diagnostics' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Diagnostics</CardTitle>
                <CardDescription>Automatically apply safe fixes when a problem is found.</CardDescription>
              </CardHeader>
              <CardContent>
                <SwitchRow
                  checked={autoFixDiagnostics}
                  onChange={(value) => {
                    setAutoFixDiagnostics(value)
                    void run('auto-fix', async () => { await runCommand({ type: 'set_setting', key: 'diagnostics.auto_fix', value }) })
                  }}
                  title="Automatically fix safe problems"
                  hint="Runs at startup and every five minutes. Fixes that may replace or remove data always ask first."
                />
              </CardContent>
            </Card>
          )}

          {section === 'ai' && <AiCard />}
          {section === 'resources' && <ResourcesCard />}
          {section === 'backups' && (
            <>
              <AutoBackupCard />
              <SettingsBackupsCard />
            </>
          )}
          {section === 'about' && <AboutCard />}

          {showSave && (
            <div className="flex items-center gap-3">
              <Button disabled={busy !== null} onClick={() => run('save', saveAll)}>Save settings</Button>
              {saved && <span className="text-sm text-success">{saved}</span>}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
