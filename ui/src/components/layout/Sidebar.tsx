import {
  Box,
  Database,
  Globe,
  Globe2,
  HardDrive,
  Layers,
  LayoutDashboard,
  Moon,
  Plug,
  Rocket,
  ScrollText,
  Search,
  ServerCog,
  Settings,
  Sun,
  SunMoon,
  Terminal,
  Zap,
} from 'lucide-react'

import { useState } from 'react'

import { OfflineNotice, useOnline } from '@/components/ReleaseCards'
import { Badge } from '@/components/ui/badge'
import { runCommand } from '@/core'
import { usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { useTheme } from '@/lib/theme'

export type Page =
  | 'dashboard'
  | 'sites'
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
  | 'settings'

type NavItem = { id: Page; label: string; icon: typeof LayoutDashboard }

const DASHBOARD: NavItem = { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard }
const SETTINGS: NavItem = { id: 'settings', label: 'Settings', icon: Settings }

const NAV_GROUPS: { id: string; label: string; items: NavItem[] }[] = [
  {
    id: 'sites',
    label: 'Sites',
    items: [
      { id: 'sites', label: 'Sites', icon: Globe },
      { id: 'quickapps', label: 'Quick Apps', icon: Rocket },
      { id: 'commands', label: 'Commands', icon: Zap },
      { id: 'tunnels', label: 'Tunnels', icon: Globe2 },
    ],
  },
  {
    id: 'web',
    label: 'Web',
    items: [
      { id: 'webserver', label: 'Web server', icon: ServerCog },
    ],
  },
  {
    id: 'data',
    label: 'Data & services',
    items: [
      { id: 'databases', label: 'Databases', icon: HardDrive },
      { id: 'services', label: 'Services', icon: Database },
      { id: 'runtimes', label: 'Runtimes', icon: Box },
    ],
  },
  {
    id: 'environment',
    label: 'Environment',
    items: [
      { id: 'profiles', label: 'Profiles', icon: Layers },
      { id: 'plugins', label: 'Plugins', icon: Plug },
    ],
  },
  {
    id: 'monitor',
    label: 'Monitor',
    items: [
      { id: 'logs', label: 'Logs', icon: ScrollText },
      { id: 'processes', label: 'Processes', icon: Terminal },
    ],
  },
]

const SOON_ITEMS: string[] = []

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  const { theme, resolvedTheme, setTheme } = useTheme()
  // §59: always visible when something is public.
  const [publicCount, setPublicCount] = useState(0)
  const network = useOnline()

  const navButton = ({ id, label, icon: Icon }: NavItem) => (
    <button
      key={id}
      onClick={() => onNavigate(id)}
      className={cn(
        'flex items-center gap-2.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors',
        page === id ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground',
      )}
    >
      <Icon className="size-4" />
      {label}
      {id === 'tunnels' && publicCount > 0 && (
        <Badge variant="destructive" className="ml-auto text-[10px]" title="A site is public through a tunnel">
          public
        </Badge>
      )}
    </button>
  )
  usePoll(async () => {
    const r = await runCommand({ type: 'list_tunnels' }).catch(() => null)
    if (r?.type === 'tunnels') setPublicCount(r.tunnels.filter((t) => t.state === 'connected' || t.state === 'starting').length)
  }, 5000)

  function cycleTheme() {
    setTheme(theme === 'system' ? 'light' : theme === 'light' ? 'dark' : 'system')
  }

  const ThemeIcon = theme === 'system' ? SunMoon : resolvedTheme === 'dark' ? Moon : Sun

  return (
    <aside className="flex h-full w-60 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
      <div className="flex items-center gap-2 px-4 py-4">
        <img src="/favicon.svg" alt="" className="size-8 rounded-lg shadow-sm shadow-teal-500/30" />
        <span className="text-sm font-semibold tracking-tight">OpenLocalServer</span>
      </div>

      <button
        onClick={() => window.dispatchEvent(new Event('ols:palette'))}
        className="mx-2 mb-2 flex items-center gap-2 rounded-md border border-sidebar-border px-3 py-1.5 text-sm text-sidebar-foreground/60 transition-colors hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
        title="Commands and search (Ctrl+Shift+P or Ctrl+K)"
      >
        <Search className="size-4" />
        <span className="flex-1 text-left">Search…</span>
        <kbd className="rounded bg-sidebar-accent px-1.5 text-[10px]">Ctrl K</kbd>
      </button>

      <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2">
        {navButton(DASHBOARD)}
        {NAV_GROUPS.map((g) => (
          <div key={g.id} className="mt-2.5 flex flex-col gap-0.5">
            <div className="px-3 py-1 text-[11px] font-medium uppercase tracking-wider text-sidebar-foreground/45">
              {g.label}
            </div>
            {g.items.map(navButton)}
          </div>
        ))}

        {SOON_ITEMS.length > 0 && (
          <div className="mt-4 px-3 text-[11px] font-medium uppercase tracking-wider text-sidebar-foreground/40">
            Roadmap
          </div>
        )}
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

      <div className="flex flex-col gap-0.5 border-t border-sidebar-border px-2 pt-2">{navButton(SETTINGS)}</div>
      <OfflineNotice status={network} />
      <button
        onClick={cycleTheme}
        className="mx-2 mb-3 mt-0.5 flex items-center gap-2.5 rounded-md px-3 py-1.5 text-sm text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
        title={`Theme: ${theme}`}
      >
        <ThemeIcon className="size-4" />
        <span className="capitalize">{theme} theme</span>
      </button>
    </aside>
  )
}
