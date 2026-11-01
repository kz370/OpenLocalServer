import { open } from '@tauri-apps/plugin-dialog'
import { Book, Check, Copy, ExternalLink, Eye, EyeOff, FolderPlus, FolderSearch, RefreshCw, ShieldCheck, Sparkles, Trash2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { ExcludedSitesCard } from '@/components/ExcludedSitesCard'
import { IgnoredFoldersCard } from '@/components/IgnoredFoldersCard'
import { Markdown } from '@/components/ui/Markdown'
import { ReleaseNotesView } from '@/components/ReleaseNotesView'
import { ApiCard, SystemCard, UpdatesCard } from '@/components/ReleaseCards'
import { AboutCard, AutoBackupCard, ResourcesCard, SettingsBackupsCard } from '@/components/SettingsExtras'
import { SaveButton } from '@/components/SaveButton'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import releaseNotes from '../../../release-notes/v2.6.0.md?raw'
import { Select, SettingRow, SwitchRow, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type EditorInfo, type HelperService, type ServiceStatus, type StartupSettings, runCommand, requestExit } from '@/core'
import { createPersistentCache } from '@/lib/cache'
import { confirmThen } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'
import { invalidateDefaultTldCache } from '@/lib/sites'
import { cn } from '@/lib/utils'
import {
  SettingsAboutIcon,
  SettingsAppearanceIcon,
  SettingsBackupsIcon,
  SettingsDeletedIcon,
  SettingsDiagnosticsIcon,
  SettingsGeneralIcon,
  SettingsManualIcon,
  SettingsMcpIcon,
  SettingsResourcesIcon,
  SettingsRootsIcon,
  SettingsSitesIcon,
  SettingsStartupIcon,
} from '@/components/layout/SidebarIcons'
import { AppearanceSettings } from '@/components/settings/AppearanceSettings'

type Section = 'general' | 'appearance' | 'sites' | 'roots' | 'deleted' | 'startup' | 'manual' | 'resources' | 'backups' | 'about' | 'mcp'
const SECTIONS: { id: Section; label: string; icon: React.ComponentType<{ className?: string }> }[] = [
  { id: 'general', label: 'General', icon: SettingsGeneralIcon },
  { id: 'appearance', label: 'Appearance', icon: SettingsAppearanceIcon },
  { id: 'mcp', label: 'MCP / AI tools', icon: SettingsMcpIcon },
  { id: 'sites', label: 'Sites & domains', icon: SettingsSitesIcon },
  { id: 'roots', label: 'Root folders', icon: SettingsRootsIcon },
  { id: 'deleted', label: 'Hide/Exclude', icon: SettingsDeletedIcon },
  { id: 'startup', label: 'Startup & tray', icon: SettingsStartupIcon },
  { id: 'manual', label: 'User Manual', icon: SettingsManualIcon },
  { id: 'resources', label: 'Resources', icon: SettingsResourcesIcon },
  { id: 'backups', label: 'Backups', icon: SettingsBackupsIcon },
  { id: 'about', label: 'About', icon: SettingsAboutIcon },
]

async function getString(key: string): Promise<string> {
  const r = await runCommand({ type: 'get_setting', key })
  return r.type === 'setting' && typeof r.value === 'string' ? r.value : ''
}

async function getStringList(key: string): Promise<string[]> {
  const r = await runCommand({ type: 'get_setting', key })
  return r.type === 'setting' && Array.isArray(r.value) ? r.value.filter((value): value is string => typeof value === 'string') : []
}

const startupCache = createPersistentCache<StartupSettings | null>('settings_startup', null)
const servicesCache = createPersistentCache<ServiceStatus[]>('settings_services', [])
const editorsCache = createPersistentCache<EditorInfo[]>('settings_editors', [])
const editorCache = createPersistentCache<string>('settings_editor', '')
const editorIdCache = createPersistentCache<string>('settings_editor_id', 'vscode')
const helperCache = createPersistentCache<HelperService | null>('settings_helper', null)
const versionCache = createPersistentCache<string>('settings_version', '')
const projectsDirCache = createPersistentCache<string>('settings_projects_dir', '')
const sitesDirCache = createPersistentCache<string>('settings_sites_dir', '')
const rootFoldersCache = createPersistentCache<string[]>('settings_root_folders', [])
const defaultTldCache = createPersistentCache<string>('settings_default_tld', 'local')
const customTldCache = createPersistentCache<string>('settings_custom_tld', '')
const watchSitesCache = createPersistentCache<boolean>('settings_watch_sites', true)
const autoDomainsCache = createPersistentCache<boolean>('settings_auto_domains', true)
const autoFixDiagnosticsCache = createPersistentCache<boolean>('settings_auto_fix_diagnostics', true)
const mcpEnabledCache = createPersistentCache<boolean>('settings_mcp_enabled', false)
const mcpTokenCache = createPersistentCache<string>('settings_mcp_token', '')
const mcpConfiguredCache = createPersistentCache<boolean>('settings_mcp_configured', false)
const mcpExpiryModeCache = createPersistentCache<'none' | 'hours' | 'at'>('settings_mcp_expiry_mode', 'none')
const mcpTtlHoursCache = createPersistentCache<number>('settings_mcp_ttl_hours', 0)
const mcpExpiresAtCache = createPersistentCache<string>('settings_mcp_expires_at', '')
const manualCache = createPersistentCache<string | null>('settings_manual', null)

/** §119–121 startup, tray and notification behavior, plus the everyday defaults. */
export function SettingsPage() {
  const [startup, setStartup] = useState<StartupSettings | null>(startupCache.get)
  const [services, setServices] = useState<ServiceStatus[]>(servicesCache.get)
  const [editor, setEditor] = useState(editorCache.get)
  const [editorId, setEditorId] = useState(editorIdCache.get)
  const [editors, setEditors] = useState<EditorInfo[]>(editorsCache.get)
  const [projectsDir, setProjectsDir] = useState(projectsDirCache.get)
  const [autoDomains, setAutoDomains] = useState(autoDomainsCache.get)
  const [watchSites, setWatchSites] = useState(watchSitesCache.get)
  const [autoFixDiagnostics, setAutoFixDiagnostics] = useState(autoFixDiagnosticsCache.get)
  const [sitesDir, setSitesDir] = useState(sitesDirCache.get)
  const [rootFolders, setRootFolders] = useState<string[]>(rootFoldersCache.get)
  const [defaultTld, setDefaultTld] = useState(defaultTldCache.get)
  const [customTld, setCustomTld] = useState(customTldCache.get)
  const [helper, setHelper] = useState<HelperService | null>(helperCache.get)
  const [version, setVersion] = useState(versionCache.get)
  const [saved, setSaved] = useState<string | null>(null)
  const [section, setSection] = useState<Section>(() => {
    try {
      if (typeof window !== 'undefined' && window.location.hash.toLowerCase().includes('appearance')) {
        return 'appearance'
      }
      const stored = sessionStorage.getItem('ols:settings-section')
      if (stored && SECTIONS.some((s) => s.id === stored)) {
        return stored as Section
      }
    } catch {
      // ignore
    }
    return 'general'
  })
  const [manual, setManual] = useState<string | null>(manualCache.get)
  const [manualTab, setManualTab] = useState<'manual' | 'notes'>('manual')
  const [manualCopied, setManualCopied] = useState(false)
  const [mcpEnabled, setMcpEnabled] = useState(mcpEnabledCache.get)
  const [mcpToken, setMcpToken] = useState(mcpTokenCache.get)
  const [mcpTokenVisible, setMcpTokenVisible] = useState(false)
  const [mcpExpiryMode, setMcpExpiryMode] = useState<'none' | 'hours' | 'at'>(mcpExpiryModeCache.get)
  const [mcpTtlHours, setMcpTtlHours] = useState(mcpTtlHoursCache.get)
  const [mcpExpiresAt, setMcpExpiresAt] = useState(mcpExpiresAtCache.get)
  const [mcpConfigured, setMcpConfigured] = useState(mcpConfiguredCache.get)

  const releaseNotesFiltered = useMemo(() => {
    const lines = releaseNotes.split('\n')
    const out: string[] = []
    let skipSection = false
    for (const line of lines) {
      const l = line.toLowerCase()
      const isHeading = l.startsWith('#')
      if (isHeading) {
        skipSection = false
        const heading = l.replace(/^#+\s*/, '').trim()
        if (heading.toLowerCase() === 'upgrading') {
          skipSection = true
          continue
        }
      }
      if (skipSection) continue
      out.push(line)
    }
    return out.join('\n')
  }, [])

  const activeManualMarkdown = manualTab === 'notes' ? releaseNotesFiltered : (manual ?? '')

  async function copyManualContent() {
    try {
      await navigator.clipboard.writeText(activeManualMarkdown)
      setManualCopied(true)
      setTimeout(() => setManualCopied(false), 2000)
    } catch {
      // fallback
    }
  }

  const action = useAction()
  const { busy, error, setError, run } = action

  useEffect(() => {
    void runCommand({ type: 'ping' }).then((r) => {
      if (r.type === 'pong') {
        setVersion(r.version)
        versionCache.set(r.version)
      }
    })
  }, [])

  useEffect(() => {
    if (section !== 'general') return
    void runCommand({ type: 'list_editors' }).then((r) => {
      if (r.type === 'editors') {
        setEditors(r.editors)
        editorsCache.set(r.editors)
      }
    })
    void runCommand({ type: 'get_helper_service' }).then((r) => {
      if (r.type === 'helper_service') {
        setHelper(r)
        helperCache.set(r)
      }
    })
    void Promise.all([getString('editor'), getString('editor.command')]).then(([id, cmd]) => {
      setEditor(cmd)
      editorCache.set(cmd)
      const resolvedId = cmd ? 'custom' : id || 'vscode'
      setEditorId(resolvedId)
      editorIdCache.set(resolvedId)
    })
  }, [section])

  useEffect(() => {
    if (section !== 'general' && section !== 'startup') return
    void runCommand({ type: 'get_startup_settings' }).then((r) => {
      if (r.type === 'startup') {
        setStartup(r.settings)
        startupCache.set(r.settings)
      }
    })
    if (section === 'startup') {
      void runCommand({ type: 'list_services' }).then((r) => {
        if (r.type === 'services') {
          setServices(r.services)
          servicesCache.set(r.services)
        }
      })
    }
  }, [section])

  useEffect(() => {
    if (section !== 'sites' && section !== 'roots') return
    void getString('quickapps.projects_dir').then((v) => {
      setProjectsDir(v)
      projectsDirCache.set(v)
    })
    void getString('paths.sites_dir').then((v) => {
      setSitesDir(v)
      sitesDirCache.set(v)
    })
    void getStringList('projects.roots').then((v) => {
      setRootFolders(v)
      rootFoldersCache.set(v)
    })
    void runCommand({ type: 'get_setting', key: 'projects.watch' }).then((r) => {
      if (r.type === 'setting') {
        const val = r.value !== false
        setWatchSites(val)
        watchSitesCache.set(val)
      }
    })
    void runCommand({ type: 'get_setting', key: 'domains.auto' }).then((r) => {
      if (r.type === 'setting') {
        const val = r.value !== false
        setAutoDomains(val)
        autoDomainsCache.set(val)
      }
    })
    void getString('domains.default_tld').then((val) => {
      const tld = val.trim().replace(/^\./, '') || 'local'
      const presets = ['local', 'test', 'localhost']
      if (presets.includes(tld)) {
        setDefaultTld(tld)
        setCustomTld('')
        defaultTldCache.set(tld)
        customTldCache.set('')
      } else {
        setDefaultTld('custom')
        setCustomTld(tld)
        defaultTldCache.set('custom')
        customTldCache.set(tld)
      }
    })
  }, [section])

  useEffect(() => {
    if (section !== 'general') return
    void runCommand({ type: 'get_setting', key: 'diagnostics.auto_fix' }).then((r) => {
      if (r.type === 'setting') {
        const val = r.value !== false
        setAutoFixDiagnostics(val)
        autoFixDiagnosticsCache.set(val)
      }
    })
  }, [section])

  useEffect(() => {
    if (section !== 'mcp') return
    void runCommand({ type: 'get_setting', key: 'mcp.auth.enabled' }).then((r) => {
      if (r.type === 'setting' && typeof r.value === 'boolean') {
        setMcpEnabled(r.value)
        mcpEnabledCache.set(r.value)
      }
    })
    void runCommand({ type: 'get_secret', key: 'mcp.auth.token' }).then((r) => {
      if (r.type === 'secret') {
        setMcpConfigured(!!r.value)
        setMcpToken(r.value ?? '')
        mcpConfiguredCache.set(!!r.value)
        mcpTokenCache.set(r.value ?? '')
      }
    })
    void runCommand({ type: 'get_secret', key: 'mcp.auth.token.expires_at' }).then((r) => {
      if (r.type === 'secret' && r.value) {
        const ms = parseInt(r.value, 10)
        if (!isNaN(ms)) {
          const val = new Date(ms).toISOString().slice(0, 16)
          setMcpExpiresAt(val)
          setMcpExpiryMode('at')
          mcpExpiresAtCache.set(val)
          mcpExpiryModeCache.set('at')
        }
      }
    })
    void runCommand({ type: 'get_secret', key: 'mcp.auth.token.ttl_ms' }).then((r) => {
      if (r.type === 'secret' && r.value) {
        const ttl = parseInt(r.value, 10)
        if (!isNaN(ttl) && ttl > 0) {
          const hours = ttl / (3600 * 1000)
          setMcpTtlHours(hours)
          setMcpExpiryMode('hours')
          mcpTtlHoursCache.set(hours)
          mcpExpiryModeCache.set('hours')
        }
      }
    })
  }, [section])

  useEffect(() => {
    if (section !== 'manual') return
    void runCommand({ type: 'get_app_guide', version: version || '2.6.0' }).then((r) => {
      if (r.type === 'app_guide') {
        setManual(r.markdown)
        manualCache.set(r.markdown)
      }
    })
  }, [section, version])

  async function saveAll() {
    if (startup) {
      const r = await runCommand({ type: 'set_startup_settings', settings: startup })
      if (r.type === 'startup') {
        setStartup(r.settings)
        startupCache.set(r.settings)
      }
    }
    const targetEditorId = editorId === 'custom' ? '' : editorId
    const targetEditorCmd = editorId === 'custom' ? editor : ''
    await runCommand({ type: 'set_setting', key: 'editor', value: targetEditorId })
    await runCommand({ type: 'set_setting', key: 'editor.command', value: targetEditorCmd })
    editorCache.set(targetEditorCmd)
    editorIdCache.set(editorId)

    await runCommand({ type: 'set_setting', key: 'quickapps.projects_dir', value: projectsDir })
    projectsDirCache.set(projectsDir)

    await runCommand({ type: 'set_setting', key: 'domains.auto', value: autoDomains })
    autoDomainsCache.set(autoDomains)

    await runCommand({ type: 'set_setting', key: 'projects.watch', value: watchSites })
    watchSitesCache.set(watchSites)

    const tldValue = defaultTld === 'custom' ? customTld.trim().replace(/^\./, '') : defaultTld
    await runCommand({ type: 'set_setting', key: 'domains.default_tld', value: tldValue || 'local' })
    defaultTldCache.set(defaultTld)
    customTldCache.set(customTld)

    invalidateDefaultTldCache()
    if (autoDomains) await runCommand({ type: 'sync_auto_domains' })
    setSaved('Saved.')
    setTimeout(() => setSaved(null), 2500)
  }

  async function saveMcp() {
    await runCommand({ type: 'set_setting', key: 'mcp.auth.enabled', value: mcpEnabled })
    mcpEnabledCache.set(mcpEnabled)
    if (mcpToken) {
      await runCommand({ type: 'set_secret', key: 'mcp.auth.token', value: mcpToken })
      mcpTokenCache.set(mcpToken)
      mcpConfiguredCache.set(true)
      setMcpConfigured(true)
      const now = Date.now()
      await runCommand({ type: 'set_secret', key: 'mcp.auth.token.created_at', value: String(now) })
      if (mcpExpiryMode === 'hours' && mcpTtlHours > 0) {
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.ttl_ms', value: String(mcpTtlHours * 3600 * 1000) })
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.expires_at', value: '' })
        mcpExpiryModeCache.set('hours')
        mcpTtlHoursCache.set(mcpTtlHours)
      } else if (mcpExpiryMode === 'at' && mcpExpiresAt) {
        const expiresMs = new Date(mcpExpiresAt).getTime()
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.ttl_ms', value: '' })
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.expires_at', value: String(expiresMs) })
        mcpExpiryModeCache.set('at')
        mcpExpiresAtCache.set(mcpExpiresAt)
      } else {
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.ttl_ms', value: '' })
        await runCommand({ type: 'set_secret', key: 'mcp.auth.token.expires_at', value: '' })
        mcpExpiryModeCache.set('none')
      }
    }
    setSaved('MCP settings saved.')
    setTimeout(() => setSaved(null), 2500)
  }

  function generateMcpToken() {
    const bytes = new Uint8Array(32)
    crypto.getRandomValues(bytes)
    setMcpToken(Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join(''))
  }

  async function addRootFolder() {
    const picked = await open({ directory: true, title: 'Select a root projects folder' })
    if (!picked || Array.isArray(picked) || rootFolders.some((root) => root.toLowerCase() === picked.toLowerCase())) return
    const next = [...rootFolders, picked]
    await run('add root folder', async () => {
      const result = await runCommand({ type: 'set_setting', key: 'projects.roots', value: next })
      if (result.type === 'ok') {
        setRootFolders(next)
        rootFoldersCache.set(next)
        if (autoDomains) await runCommand({ type: 'sync_auto_domains' })
      }
    })
  }

  async function removeRootFolder(root: string) {
    const next = rootFolders.filter((folder) => folder !== root)
    await run('remove root folder', async () => {
      const result = await runCommand({ type: 'set_setting', key: 'projects.roots', value: next })
      if (result.type === 'ok') {
        setRootFolders(next)
        rootFoldersCache.set(next)
      }
    })
  }

  const defaultStartup: StartupSettings = {
    with_windows: false,
    start_minimized: false,
    autostart_services: [],
    notifications: true,
    close_to_tray: true,
  }
  const currentStartup = startup ?? defaultStartup

  const set = (patch: Partial<StartupSettings>) => {
    setStartup((prev) => {
      const next = { ...(prev ?? defaultStartup), ...patch }
      startupCache.set(next)
      return next
    })
  }
  const toggleService = (id: string, on: boolean) => {
    setStartup((prev) => {
      const base = prev ?? defaultStartup
      const next = {
        ...base,
        autostart_services: on ? [...base.autostart_services, id] : base.autostart_services.filter((s) => s !== id),
      }
      startupCache.set(next)
      return next
    })
  }

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
    <div className="flex flex-wrap items-center gap-3">
      {helper === null ? (
        <Badge variant="secondary">Checking…</Badge>
      ) : helper.status === 'development_version' ? (
        <Badge variant="destructive">Development version · service {helper.version}, app {version}</Badge>
      ) : helper.status === 'update_available' ? (
        <Badge variant="warning">Installed · version {helper.version} (app is {version})</Badge>
      ) : helper.installed ? (
        <Badge variant="success">Installed · no more prompts</Badge>
      ) : (
        <Badge variant="outline">Not installed</Badge>
      )}
      {/* The service runs a *copy* of the helper, and nothing refreshes that copy on an
          update or a move, so an out-of-date one is offered the same reinstall as a
          missing one — it is the same fix and the same single prompt. A service newer
          than the app (dev build) is a development version, not something to update. */}
      {helper !== null && (!helper.installed || helper.status === 'update_available') && (
        <Button size="sm" disabled={busy !== null} onClick={() => run('helper', async () => { const r = await runCommand({ type: 'install_helper_service' }); if (r.type === 'helper_service') setHelper(r) })}>
          {busy === 'helper' ? <Spinner /> : <ShieldCheck />} {helper.status === 'update_available' ? 'Update (asks once)' : 'Install (asks once)'}
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

  const showSave = section === 'general' || section === 'sites' || section === 'startup' || section === 'mcp'

  return (
    <div className="flex flex-col gap-5">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">OLS {version && `v${version}`}</p>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="flex flex-col gap-6 md:flex-row md:items-start">
        <nav className="flex shrink-0 gap-1 overflow-x-auto md:sticky md:top-0 md:w-56 md:flex-col" aria-label="Settings sections">
          {SECTIONS.map((s) => {
            const isActive = section === s.id
            return (
              <button
                key={s.id}
                onClick={() => {
                  setSection(s.id)
                  try {
                    sessionStorage.setItem('ols:settings-section', s.id)
                  } catch {
                    // ignore
                  }
                }}
                className={cn(
                  'group relative flex items-center gap-3 whitespace-nowrap rounded-xl px-3.5 py-2.5 text-left text-sm font-medium transition-all duration-150',
                  isActive
                    ? 'border border-primary/70 bg-gradient-to-r from-primary/15 via-primary/5 to-transparent text-foreground font-semibold shadow-xs dark:border-primary/80 dark:from-primary/20 dark:via-primary/5 dark:to-transparent dark:text-white dark:shadow-[0_0_15px_var(--ring)]'
                    : 'border border-transparent text-muted-foreground hover:bg-accent/40 hover:text-foreground',
                )}
              >
                {isActive && (
                  <span className="absolute left-0 top-2 bottom-2 w-1 rounded-r-full bg-primary shadow-[0_0_8px_var(--primary)]" />
                )}
                <s.icon className="size-5 shrink-0 transition-transform duration-150 group-hover:scale-105" />
                <span className="truncate">{s.label}</span>
              </button>
            )
          })}
        </nav>

        <div className="flex min-w-0 max-w-3xl flex-1 flex-col gap-5">
          {section === 'appearance' && <AppearanceSettings />}

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
                  {/* Stacked: the badge plus two buttons are wider than the side-by-side
                      control column, and a control that wide squeezed the title into a
                      one-character-wide column. */}
                  <SettingRow
                    stacked
                    title="Administrator helper"
                    hint="Windows only lets administrators change how domain names resolve. The helper is a small background service that does it for the app, so Windows asks you once, when it's installed, instead of on every change."
                  >
                    {helperField}
                  </SettingRow>
                </CardContent>
              </Card>
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-2 text-sm">
                    <SettingsDiagnosticsIcon className="size-4" /> Diagnostics
                  </CardTitle>
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
            <>
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
              <IgnoredFoldersCard />
            </>
          )}

          {section === 'deleted' && (
            <>
              <IgnoredFoldersCard />
              <ExcludedSitesCard />
            </>
          )}

          {section === 'startup' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Startup and tray</CardTitle>
                <CardDescription>Servers keep running from the system tray after you close the window.</CardDescription>
              </CardHeader>
              <CardContent>
                <SwitchRow checked={currentStartup.with_windows} onChange={(v) => set({ with_windows: v })} title="Start with Windows" hint="Adds OLS to your account's startup list (no administrator rights needed)." />
                <SwitchRow checked={currentStartup.start_minimized} onChange={(v) => set({ start_minimized: v })} title="Start minimized to the tray" hint="Every launch, whether you open it yourself or Windows starts it with your account." />
                <SwitchRow checked={currentStartup.close_to_tray} onChange={(v) => set({ close_to_tray: v })} title="Closing the window keeps running in the tray" hint="Use Quit in the tray menu to stop everything." />
                <SwitchRow checked={currentStartup.notifications} onChange={(v) => set({ notifications: v })} title="Desktop notifications" hint="When a process crashes, an install finishes, or a Quick App completes." />
                <SettingRow stacked title="Start these services automatically" hint="A web server listed here has its config applied before it starts, so your sites answer as soon as the app opens.">
                  <div className="flex flex-col gap-2.5">
                    {services.filter((s) => s.installed).map((s) => (
                      <Toggle key={s.id} checked={currentStartup.autostart_services.includes(s.id)} onChange={(v) => toggleService(s.id, v)} label={s.name} />
                    ))}
                    {services.every((s) => !s.installed) && <span className="text-sm text-muted-foreground">No services installed yet.</span>}
                  </div>
                </SettingRow>
                <div className="mt-4 flex items-center justify-between border-t border-border pt-4">
                  <div className="space-y-0.5">
                    <div className="text-sm font-medium">Exit OpenLocalServer</div>
                    <div className="text-xs text-muted-foreground">Stop all running services, web servers, and exit the application.</div>
                  </div>
                  <Button variant="destructive" size="sm" onClick={() => void requestExit()}>
                    Quit OpenLocalServer
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

          {section === 'manual' && (
            <Card>
              <CardHeader className="pb-2">
                <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
                  <div>
                    <CardTitle className="text-sm">User Manual</CardTitle>
                    <CardDescription>
                      Open Local Server user manual and release notes. Read how sites, databases, services,
                      tunnels, runtimes, and profiles work before changing settings.
                    </CardDescription>
                  </div>
                  <Badge variant="outline" className="text-xs px-2.5 py-0.5 w-fit border-border/80">
                    v{version || '2.5.0'}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-3">
                  <div className="flex gap-2">
                    <button
                      className={cn(
                        'flex flex-shrink-0 items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors cursor-pointer',
                        manualTab === 'manual' ? 'bg-accent text-accent-foreground shadow-xs' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
                      )}
                      onClick={() => setManualTab('manual')}
                    >
                      <Book className="size-4 shrink-0" /> User Manual
                    </button>
                    <button
                      className={cn(
                        'flex flex-shrink-0 items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors cursor-pointer',
                        manualTab === 'notes' ? 'bg-accent text-accent-foreground shadow-xs' : 'text-muted-foreground hover:bg-accent/50 hover:text-foreground',
                      )}
                      onClick={() => setManualTab('notes')}
                    >
                      <Sparkles className="size-4 shrink-0 text-primary" /> {`Release Notes (v${version || '2.6.0'})`}
                    </button>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void copyManualContent()}
                      className="h-8 text-xs shrink-0 cursor-pointer"
                      title="Copy current document text"
                    >
                      {manualCopied ? (
                        <>
                          <Check className="size-3.5 text-emerald-500 mr-1" /> Copied
                        </>
                      ) : (
                        <>
                          <Copy className="size-3.5 mr-1" /> Copy Markdown
                        </>
                      )}
                    </Button>
                  </div>
                </div>

                {manualTab === 'notes' ? (
                  <ReleaseNotesView
                    markdown={activeManualMarkdown}
                    currentVersion={version || '2.6.0'}
                    onCopyMarkdown={() => void copyManualContent()}
                    isCopied={manualCopied}
                  />
                ) : (
                  <div className="flex-1 rounded-xl border border-border/70 bg-card/40 p-5 text-sm leading-relaxed overflow-y-auto min-h-[350px] max-h-[calc(100vh-280px)]">
                    {manual ? (
                      <Markdown content={manual} className="prose prose-sm max-w-none release-notes-prose" />
                    ) : (
                      <p className="text-muted-foreground">Loading manual…</p>
                    )}
                  </div>
                )}
              </CardContent>
            </Card>
          )}
          {section === 'resources' && <ResourcesCard />}
          {section === 'backups' && (
            <>
              <AutoBackupCard />
              <SettingsBackupsCard />
            </>
          )}
          {section === 'about' && (
            <>
              <AboutCard />
              <UpdatesCard />
            </>
          )}

          {section === 'mcp' && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">MCP authentication</CardTitle>
                <CardDescription>
                  AI tools (Claude Desktop, Cursor, etc.) connect to OLS through the ols-mcp
                  server. Enable a bearer token so only authorised hosts can send commands.
                  The token is stored in the OS credential manager, never in settings or logs.
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <SwitchRow
                  checked={mcpEnabled}
                  onChange={setMcpEnabled}
                  title="Require MCP authentication"
                  hint="When on, the ols-mcp server must be launched with --token or OLS_MCP_TOKEN matching this token. Off keeps existing installs working unchanged."
                />
                {!mcpEnabled && (
                  <p className="text-xs text-muted-foreground">
                    Turn authentication on to generate a token, then pass it as
                    &ldquo;--token &lt;token&gt;&rdquo; or OLS_MCP_TOKEN in your MCP host config.
                  </p>
                )}
                {mcpEnabled && (
                  <>
                    <SettingRow
                      stacked
                      title="Bearer token"
                      hint={mcpConfigured ? 'Stored in the OS credential manager. Copy it into your MCP host config as --token or OLS_MCP_TOKEN.' : 'No token configured. Generate one below.'}
                    >
                      <div className="flex items-center gap-2">
                        <Input
                          type={mcpTokenVisible ? 'text' : 'password'}
                          value={mcpToken}
                          onChange={(e) => setMcpToken(e.target.value)}
                          placeholder="Leave blank to keep current token"
                          className="flex-1 font-mono text-xs"
                        />
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => setMcpTokenVisible(!mcpTokenVisible)}
                          aria-label={mcpTokenVisible ? 'Hide token' : 'Show token'}
                        >
                          {mcpTokenVisible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={async () => {
                            await navigator.clipboard.writeText(mcpToken)
                          }}
                          disabled={!mcpToken}
                          aria-label="Copy token"
                        >
                          <Copy className="size-4" />
                        </Button>
                        <Button size="sm" variant="outline" onClick={generateMcpToken}>
                          <RefreshCw className="size-4" />
                        </Button>
                      </div>
                    </SettingRow>
                    <SettingRow
                      stacked
                      title="Token expiry"
                      hint="Never expires = valid until rotated. Hours = TTL from creation. At = absolute date/time."
                    >
                      <div className="flex items-start gap-3">
                        <Select
                          value={mcpExpiryMode}
                          onChange={(e) => setMcpExpiryMode(e.target.value as 'none' | 'hours' | 'at')}
                          className="w-48"
                        >
                          <option value="none">Never expires</option>
                          <option value="hours">Expires after N hours</option>
                          <option value="at">Expires at date/time</option>
                        </Select>
                        {mcpExpiryMode === 'hours' && (
                          <Input
                            type="number"
                            value={mcpTtlHours}
                            onChange={(e) => setMcpTtlHours(parseInt(e.target.value, 10) || 0)}
                            placeholder="hours"
                            className="w-24 text-sm"
                          />
                        )}
                        {mcpExpiryMode === 'at' && (
                          <Input
                            type="datetime-local"
                            value={mcpExpiresAt}
                            onChange={(e) => setMcpExpiresAt(e.target.value)}
                            className="w-64 text-sm"
                          />
                        )}
                      </div>
                    </SettingRow>
                    <p className="text-xs text-muted-foreground">
                      Token rotation: changing the token above invalidates every connected
                      MCP host on next request. Update each host&rsquo;s --token / OLS_MCP_TOKEN
                      with the new value. No reinstall needed.
                    </p>
                  </>
                )}
              </CardContent>
            </Card>
          )}

          {showSave && (
            <div className="flex items-center gap-3">
              <SaveButton action={action} name="save" busyLabel="Saving…" onClick={() => void run('save', section === 'mcp' ? saveMcp : saveAll)}>Save settings</SaveButton>
              {saved && <span className="text-sm text-success">{saved}</span>}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
