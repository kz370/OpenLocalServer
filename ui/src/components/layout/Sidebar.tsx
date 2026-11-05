import { useState } from 'react'

import { OfflineNotice, useOnline } from '@/components/ReleaseCards'
import { Badge } from '@/components/ui/badge'
import { runCommand } from '@/core'
import { useAppMarkPath, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { backgroundSuitsMode, useTheme } from '@/lib/theme'
import {
  CommandsIcon,
  DashboardIcon,
  DatabasesIcon,
  DockerIcon,
  LogsIcon,
  MoonThemeIcon,
  PluginsIcon,
  ProcessesIcon,
  ProfilesIcon,
  ProjectsIcon,
  QuickAppsIcon,
  RuntimesIcon,
  SearchNavIcon,
  ServicesIcon,
  SettingsIcon,
  SitesIcon,
  SunThemeIcon,
  SystemThemeIcon,
  TasksIcon,
  TunnelsIcon,
  WebServerIcon,
} from './SidebarIcons'

export type Page =
  | 'dashboard'
  | 'projects'
  | 'sites'
  | 'docker'
  | 'quickapps'
  | 'commands'
  | 'webserver'
  | 'config'
  | 'tunnels'
  | 'profiles'
  | 'plugins'
  | 'databases'
  | 'services'
  | 'runtimes'
  | 'logs'
  | 'processes'
  | 'tasks'
  | 'settings'

type NavItem = {
  id: Page
  label: string
  icon: React.ComponentType<{ className?: string }>
}

const DASHBOARD: NavItem = { id: 'dashboard', label: 'Dashboard', icon: DashboardIcon }
const SETTINGS: NavItem = { id: 'settings', label: 'Settings', icon: SettingsIcon }

const NAV_GROUPS: { id: string; label: string; items: NavItem[] }[] = [
  {
    id: 'sites',
    label: 'Sites',
    items: [
      { id: 'projects', label: 'Projects', icon: ProjectsIcon },
      { id: 'sites', label: 'Sites', icon: SitesIcon },
      { id: 'docker', label: 'Docker', icon: DockerIcon },
      { id: 'quickapps', label: 'Quick Apps', icon: QuickAppsIcon },
      { id: 'commands', label: 'Commands', icon: CommandsIcon },
      { id: 'tunnels', label: 'Tunnels', icon: TunnelsIcon },
    ],
  },
  {
    id: 'web',
    label: 'Web',
    items: [
      { id: 'webserver', label: 'Web server', icon: WebServerIcon },
    ],
  },
  {
    id: 'data',
    label: 'Data & services',
    items: [
      { id: 'databases', label: 'Databases', icon: DatabasesIcon },
      { id: 'services', label: 'Services', icon: ServicesIcon },
      { id: 'runtimes', label: 'Runtimes', icon: RuntimesIcon },
    ],
  },
  {
    id: 'environment',
    label: 'Environment',
    items: [
      { id: 'profiles', label: 'Profiles', icon: ProfilesIcon },
      { id: 'plugins', label: 'Plugins', icon: PluginsIcon },
    ],
  },
  {
    id: 'monitor',
    label: 'Monitor',
    items: [
      { id: 'logs', label: 'Logs', icon: LogsIcon },
      { id: 'processes', label: 'Processes', icon: ProcessesIcon },
      { id: 'tasks', label: 'Background tasks', icon: TasksIcon },
    ],
  },
]

const SOON_ITEMS: string[] = []

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  const { theme, resolvedTheme, setTheme, options } = useTheme()
  // Translucent acrylic only over a gradient that suits the mode; a mismatched one keeps the solid surface.
  const isGradient =
    options?.background?.type === 'gradient' && backgroundSuitsMode(options.background, resolvedTheme)
  // §59: always visible when something is public.
  const [publicCount, setPublicCount] = useState(0)
  const network = useOnline()

  const navButton = ({ id, label, icon: Icon }: NavItem) => {
    const isActive = page === id
    return (
      <button
        key={id}
        onClick={() => onNavigate(id)}
        className={cn(
          'group relative flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-all duration-150',
          isActive
            ? 'border border-primary/70 bg-gradient-to-r from-primary/15 via-primary/5 to-transparent text-foreground font-semibold shadow-xs dark:border-primary/80 dark:from-primary/20 dark:via-primary/5 dark:to-transparent dark:text-white dark:shadow-[0_0_15px_var(--ring)]'
            : 'border border-transparent text-sidebar-foreground/75 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground',
        )}
      >
        {isActive && (
          <span className="absolute left-0 top-1.5 bottom-1.5 w-1 rounded-r-full bg-primary shadow-[0_0_8px_var(--primary)]" />
        )}
        <Icon className="size-5 shrink-0 transition-transform duration-150 group-hover:scale-105" />
        <span className="truncate">{label}</span>
        {id === 'tunnels' && publicCount > 0 && (
          <Badge variant="success" className="ml-auto text-[10px]" title="A site is public through a tunnel">
            public
          </Badge>
        )}
      </button>
    )
  }

  usePoll(async () => {
    const r = await runCommand({ type: 'list_tunnels' }).catch(() => null)
    if (r?.type === 'tunnels') setPublicCount(r.tunnels.filter((t) => t.state === 'connected' || t.state === 'starting').length)
  }, 5000)

  function cycleTheme() {
    setTheme(theme === 'system' ? 'light' : theme === 'light' ? 'dark' : 'system')
  }

  const currentThemeEffective = theme === 'system' ? 'system' : resolvedTheme === 'dark' ? 'dark' : 'light'
  const ThemeIcon =
    currentThemeEffective === 'system'
      ? SystemThemeIcon
      : currentThemeEffective === 'dark'
        ? MoonThemeIcon
        : SunThemeIcon

  const themeLabel =
    currentThemeEffective === 'dark'
      ? 'Dark Theme'
      : currentThemeEffective === 'light'
        ? 'Light Theme'
        : 'System Theme'

  // Same four colourways (light/dark × running/stopped) the tray and taskbar use.
  const mark = useAppMarkPath()

  return (
    <aside
      className={cn(
        'flex h-full w-60 shrink-0 flex-col border-r border-sidebar-border backdrop-blur-xl text-sidebar-foreground transition-colors duration-300',
        isGradient ? 'bg-sidebar/40' : 'bg-sidebar/85',
      )}
    >
      <div className="flex items-center gap-2.5 px-4 py-4">
        <img
          src={mark}
          alt=""
          className="size-8 rounded-full shadow-sm shadow-teal-500/30"
        />
        <span className="text-sm font-semibold tracking-tight">OLS</span>
      </div>

      <button
        onClick={() => window.dispatchEvent(new Event('ols:palette'))}
        className="mx-2 mb-2 flex items-center gap-2.5 rounded-lg border border-sidebar-border/70 bg-sidebar-accent/20 px-3 py-1.5 text-sm text-sidebar-foreground/65 transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
        title="Commands and search (Ctrl+Shift+P or Ctrl+K)"
      >
        <SearchNavIcon className="size-4" />
        <span className="flex-1 text-left text-xs">Search…</span>
        <kbd className="rounded bg-sidebar-accent/80 px-1.5 py-0.5 text-[10px] text-sidebar-foreground/75">Ctrl K</kbd>
      </button>

      <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2">
        {navButton(DASHBOARD)}
        {NAV_GROUPS.map((g) => (
          <div key={g.id} className="mt-2.5 flex flex-col gap-0.5">
            <div className="flex items-center gap-2 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-sidebar-foreground/45">
              <span>{g.label}</span>
              <div className="h-px flex-1 bg-sidebar-border/60" />
            </div>
            {g.items.map(navButton)}
          </div>
        ))}

        {SOON_ITEMS.length > 0 && (
          <div className="mt-4 flex items-center gap-2 px-3 text-[11px] font-semibold uppercase tracking-wider text-sidebar-foreground/40">
            <span>Roadmap</span>
            <div className="h-px flex-1 bg-sidebar-border/60" />
          </div>
        )}
        {SOON_ITEMS.map((label) => (
          <div
            key={label}
            className="flex items-center justify-between rounded-lg px-3 py-2 text-sm text-sidebar-foreground/35"
          >
            {label}
            <Badge variant="secondary" className="text-[10px] opacity-60">
              soon
            </Badge>
          </div>
        ))}
      </nav>

      <div className="mx-2 my-1 h-px bg-sidebar-border/60" />

      <div className="flex flex-col gap-0.5 px-2">
        {navButton(SETTINGS)}
      </div>
      <OfflineNotice status={network} />
      <button
        onClick={cycleTheme}
        className="mx-2 mb-3 mt-1 flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-sidebar-foreground/75 transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
        title={`Theme: ${theme} (click to switch)`}
      >
        <ThemeIcon className="size-5 shrink-0" />
        <span>{themeLabel}</span>
      </button>
    </aside>
  )
}
