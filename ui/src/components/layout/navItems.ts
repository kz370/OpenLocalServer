import {
  CommandsIcon,
  DashboardIcon,
  DatabasesIcon,
  DockerIcon,
  LogsIcon,
  PluginsIcon,
  ProcessesIcon,
  ProfilesIcon,
  ProjectsIcon,
  QuickAppsIcon,
  RuntimesIcon,
  ServicesIcon,
  SettingsIcon,
  SitesIcon,
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

export type NavItem = {
  id: Page
  label: string
  icon: React.ComponentType<{ className?: string; monochrome?: boolean }>
}

export const DASHBOARD: NavItem = { id: 'dashboard', label: 'Dashboard', icon: DashboardIcon }
export const SETTINGS: NavItem = { id: 'settings', label: 'Settings', icon: SettingsIcon }

export const NAV_GROUPS: { id: string; label: string; items: NavItem[] }[] = [
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
    items: [{ id: 'webserver', label: 'Web server', icon: WebServerIcon }],
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

/**
 * Sidebar items that can never be hidden: the core of running a local server, plus Settings,
 * which is where hidden items are brought back (hiding it would leave no way back).
 * Any of these found in the stored hidden list is ignored.
 */
export const LOCKED_NAV_ITEMS: ReadonlySet<Page> = new Set<Page>([
  'dashboard',
  'projects',
  'sites',
  'webserver',
  'databases',
  'services',
  'runtimes',
  'settings',
])
