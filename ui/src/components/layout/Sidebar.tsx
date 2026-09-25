import {
  Box,
  Database,
  FileCode2,
  FolderKanban,
  Globe,
  HardDrive,
  LayoutDashboard,
  Moon,
  Rocket,
  ScrollText,
  Server,
  Settings,
  Sun,
  SunMoon,
  Terminal,
  Zap,
} from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { useTheme } from '@/lib/theme'

export type Page =
  | 'dashboard'
  | 'projects'
  | 'quickapps'
  | 'commands'
  | 'domains'
  | 'config'
  | 'databases'
  | 'services'
  | 'runtimes'
  | 'logs'
  | 'processes'
  | 'settings'

const NAV_ITEMS: { id: Page; label: string; icon: typeof LayoutDashboard }[] = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
  { id: 'projects', label: 'Projects', icon: FolderKanban },
  { id: 'quickapps', label: 'Quick Apps', icon: Rocket },
  { id: 'commands', label: 'Commands', icon: Zap },
  { id: 'domains', label: 'Domains & HTTPS', icon: Globe },
  { id: 'config', label: 'Web config', icon: FileCode2 },
  { id: 'databases', label: 'Databases', icon: HardDrive },
  { id: 'services', label: 'Services', icon: Database },
  { id: 'runtimes', label: 'Runtimes', icon: Box },
  { id: 'logs', label: 'Logs', icon: ScrollText },
  { id: 'processes', label: 'Processes', icon: Terminal },
  { id: 'settings', label: 'Settings', icon: Settings },
]

const SOON_ITEMS = ['Tunnels', 'Profiles', 'Plugins']

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  const { theme, resolvedTheme, setTheme } = useTheme()

  function cycleTheme() {
    setTheme(theme === 'system' ? 'light' : theme === 'light' ? 'dark' : 'system')
  }

  const ThemeIcon = theme === 'system' ? SunMoon : resolvedTheme === 'dark' ? Moon : Sun

  return (
    <aside className="flex h-full w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
      <div className="flex items-center gap-2 px-4 py-4">
        <div className="flex size-8 items-center justify-center rounded-lg bg-gradient-to-br from-teal-500 to-emerald-600 text-white shadow-sm shadow-teal-500/30">
          <Server className="size-4" />
        </div>
        <span className="text-sm font-semibold tracking-tight">OpenLocalServer</span>
      </div>

      <nav className="flex flex-1 flex-col gap-0.5 px-2">
        {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => onNavigate(id)}
            className={cn(
              'flex items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium transition-colors',
              page === id
                ? 'bg-sidebar-accent text-sidebar-accent-foreground'
                : 'text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground',
            )}
          >
            <Icon className="size-4" />
            {label}
          </button>
        ))}

        <div className="mt-4 px-3 text-[11px] font-medium uppercase tracking-wider text-sidebar-foreground/40">
          Roadmap
        </div>
        {SOON_ITEMS.map((label) => (
          <div
            key={label}
            className="flex items-center justify-between rounded-md px-3 py-2 text-sm text-sidebar-foreground/35"
          >
            {label}
            <Badge variant="secondary" className="text-[10px] opacity-60">
              soon
            </Badge>
          </div>
        ))}
      </nav>

      <button
        onClick={cycleTheme}
        className="mx-2 mb-3 flex items-center gap-2.5 rounded-md px-3 py-2 text-sm text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
        title={`Theme: ${theme}`}
      >
        <ThemeIcon className="size-4" />
        <span className="capitalize">{theme} theme</span>
      </button>
    </aside>
  )
}
