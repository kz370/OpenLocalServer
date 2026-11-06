import { useState } from 'react'

import { OfflineNotice } from '@/components/ReleaseCards'
import { useOnline } from '@/lib/useOnline'
import { Badge } from '@/components/ui/badge'
import { runCommand } from '@/core'
import { useAppMarkPath, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { backgroundSuitsMode, useTheme } from '@/lib/theme'
import { DASHBOARD, LOCKED_NAV_ITEMS, NAV_GROUPS, type NavItem, type Page, SETTINGS } from './navItems'
import { MoonThemeIcon, SearchNavIcon, SunThemeIcon, SystemThemeIcon } from './SidebarIcons'
import { useHiddenSidebarItems } from '@/lib/sidebarVisibility'

export type { Page } from './navItems'

const SOON_ITEMS: string[] = []

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  const { theme, resolvedTheme, setTheme, options } = useTheme()
  // Translucent acrylic only over a gradient that suits the mode; a mismatched one keeps the solid surface.
  const isGradient =
    options?.background?.type === 'gradient' && backgroundSuitsMode(options.background, resolvedTheme)
  // §59: always visible when something is public.
  const [publicCount, setPublicCount] = useState(0)
  const network = useOnline()
  const hidden = useHiddenSidebarItems()
  // Hidden items stay reachable from search (Ctrl K) and links; the page you are on stays
  // listed so the sidebar always shows where you are.
  // §59: Tunnels comes back while a site is public, even if the user hid it.
  const isShown = (id: Page) =>
    LOCKED_NAV_ITEMS.has(id) || !hidden.has(id) || page === id || (id === 'tunnels' && publicCount > 0)

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
        {NAV_GROUPS.map((g) => {
          const items = g.items.filter((item) => isShown(item.id))
          // A group whose items are all hidden loses its heading too.
          if (items.length === 0) return null
          return (
            <div key={g.id} className="mt-2.5 flex flex-col gap-0.5">
              <div className="flex items-center gap-2 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-sidebar-foreground/45">
                <span>{g.label}</span>
                <div className="h-px flex-1 bg-sidebar-border/60" />
              </div>
              {items.map(navButton)}
            </div>
          )
        })}

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
