import { ListTree, Search, Wrench, X } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState, type ComponentType, type RefObject } from 'react'

import {
  SettingsAppearanceIcon,
  SettingsBackupsIcon,
  SettingsEditorsIcon,
  SettingsGeneralIcon,
  SettingsManualIcon,
  SettingsMcpIcon,
  SettingsResourcesIcon,
  SettingsSitesIcon,
  SettingsStartupIcon,
} from '@/components/layout/SidebarIcons'
import { cn } from '@/lib/utils'

export type Section =
  | 'general' | 'editors' | 'appearance' | 'sidebar' | 'sites' | 'roots' | 'deleted'
  | 'startup' | 'manual' | 'resources' | 'backups' | 'about' | 'mcp' | 'developer'

type IconType = ComponentType<{ className?: string }>

/** One sidebar entry. Entries with several `sections` show a tab bar above the page. */
export interface NavItem {
  label: string
  icon: IconType
  sections: { id: Section; tab: string }[]
}

export const NAV_GROUPS: { label: string; items: NavItem[] }[] = [
  {
    label: 'App',
    items: [
      { label: 'General', icon: SettingsGeneralIcon, sections: [{ id: 'general', tab: 'General' }] },
      { label: 'Startup & tray', icon: SettingsStartupIcon, sections: [{ id: 'startup', tab: 'Startup & tray' }] },
      { label: 'Editors', icon: SettingsEditorsIcon, sections: [{ id: 'editors', tab: 'Editors' }] },
    ],
  },
  {
    label: 'Look & feel',
    items: [
      {
        label: 'Appearance',
        icon: SettingsAppearanceIcon,
        sections: [{ id: 'appearance', tab: 'Theme' }, { id: 'sidebar', tab: 'Sidebar & Dashboard' }],
      },
    ],
  },
  {
    label: 'Sites',
    items: [
      {
        label: 'Sites',
        icon: SettingsSitesIcon,
        sections: [{ id: 'sites', tab: 'Sites & domains' }, { id: 'roots', tab: 'Root folders' }, { id: 'deleted', tab: 'Hidden sites' }],
      },
    ],
  },
  {
    label: 'Developer',
    items: [
      { label: 'Developer tools', icon: Wrench, sections: [{ id: 'developer', tab: 'Developer tools' }] },
      { label: 'MCP / AI tools', icon: SettingsMcpIcon, sections: [{ id: 'mcp', tab: 'MCP / AI tools' }] },
    ],
  },
  {
    label: 'System',
    items: [
      { label: 'Resources', icon: SettingsResourcesIcon, sections: [{ id: 'resources', tab: 'Resources' }] },
      { label: 'Backups', icon: SettingsBackupsIcon, sections: [{ id: 'backups', tab: 'Backups' }] },
    ],
  },
  {
    label: 'Help',
    items: [
      {
        label: 'Help & About',
        icon: SettingsManualIcon,
        sections: [{ id: 'manual', tab: 'User Manual' }, { id: 'about', tab: 'About & updates' }],
      },
    ],
  },
]

/** Every section id, for validating stored or requested sections. */
export const SECTION_IDS: Section[] = NAV_GROUPS.flatMap((g) => g.items.flatMap((i) => i.sections.map((s) => s.id)))

export function navItemFor(section: Section): NavItem | undefined {
  return NAV_GROUPS.flatMap((g) => g.items).find((i) => i.sections.some((s) => s.id === section))
}

/** What the search box can find: setting titles (exactly as shown on the page) and extra keywords. */
const SEARCH_INDEX: { section: Section; title: string; keywords?: string }[] = [
  { section: 'general', title: 'Administrator helper', keywords: 'admin hosts dns service windows' },
  { section: 'general', title: 'Confirm before exit', keywords: 'quit close' },
  { section: 'general', title: 'Confirm before Stop all', keywords: 'tray' },
  { section: 'general', title: 'Diagnostics', keywords: 'auto fix problems' },
  { section: 'general', title: 'Automatically fix safe problems', keywords: 'diagnostics repair' },
  { section: 'editors', title: 'Editors', keywords: 'vscode open with ide add editor' },
  { section: 'startup', title: 'Start with Windows', keywords: 'boot login autostart' },
  { section: 'startup', title: 'Start minimized to the tray' },
  { section: 'startup', title: 'Closing the window keeps running in the tray' },
  { section: 'startup', title: 'Desktop notifications', keywords: 'toast alerts' },
  { section: 'startup', title: 'Start these services automatically', keywords: 'autostart mysql nginx apache' },
  { section: 'startup', title: 'Exit OpenLocalServer', keywords: 'quit' },
  { section: 'appearance', title: 'Mode', keywords: 'dark light theme' },
  { section: 'appearance', title: 'Presets', keywords: 'theme' },
  { section: 'appearance', title: 'Accent Color', keywords: 'colour primary' },
  { section: 'appearance', title: 'Sidebar Icon Style', keywords: 'icons colorful monochrome' },
  { section: 'appearance', title: 'App Background', keywords: 'wallpaper image' },
  { section: 'appearance', title: 'Live Preview Strip' },
  { section: 'sidebar', title: 'Sidebar', keywords: 'hide show reorder items pages menu' },
  { section: 'sidebar', title: 'Dashboard', keywords: 'security section hide show' },
  { section: 'sites', title: 'Default sites folder', keywords: 'projects directory' },
  { section: 'sites', title: 'Where new projects are created', keywords: 'quick apps clone import' },
  { section: 'sites', title: 'Create domains automatically', keywords: 'https' },
  { section: 'sites', title: 'Default top-level domain', keywords: 'tld local test localhost' },
  { section: 'sites', title: 'Watch sites folder', keywords: 'detect new projects' },
  { section: 'roots', title: 'Root folders', keywords: 'watched projects' },
  { section: 'deleted', title: 'Hidden sites', keywords: 'excluded deleted ignored folders restore' },
  { section: 'developer', title: 'Databases', keywords: 'migration guard backup' },
  { section: 'developer', title: 'PHP dump viewer', keywords: 'dump dd debug' },
  { section: 'mcp', title: 'MCP authentication', keywords: 'ai claude cursor token bearer expiry' },
  { section: 'resources', title: 'Resources', keywords: 'cpu memory ram database application' },
  { section: 'resources', title: 'Faster installs on Windows', keywords: 'performance defender exclusion speed' },
  { section: 'backups', title: 'Backups', keywords: 'auto backup schedule restore snapshot' },
  { section: 'backups', title: 'How often', keywords: 'schedule backup' },
  { section: 'backups', title: 'How many to keep', keywords: 'retention backup' },
  { section: 'manual', title: 'User Manual', keywords: 'docs help release notes' },
  { section: 'about', title: 'About', keywords: 'version updates check license' },
  { section: 'general', title: 'API', keywords: 'local api' },
  { section: 'general', title: 'System', keywords: 'info paths' },
]

/** Scrolls to and briefly flashes the element on the page whose text is exactly `title`. */
export function jumpToSetting(root: HTMLElement | null, title: string): boolean {
  if (!root) return false
  const want = title.trim().toLowerCase()
  const candidates = root.querySelectorAll<HTMLElement>('[data-card-title], h1, h2, h3, label, span, p, div')
  for (const el of candidates) {
    if (el.children.length > 2) continue
    if ((el.textContent ?? '').trim().toLowerCase() !== want) continue
    el.scrollIntoView({ block: 'center', behavior: 'smooth' })
    try {
      el.animate(
        [{ backgroundColor: 'color-mix(in srgb, var(--primary) 30%, transparent)' }, { backgroundColor: 'transparent' }],
        { duration: 1800, easing: 'ease-out' },
      )
    } catch {
      // animation is cosmetic
    }
    return true
  }
  return false
}

/** Search box for the settings sidebar. Wide layout only: the narrow icon rail has no room. */
export function SettingsSearch({ onPick }: { onPick: (section: Section, title: string) => void }) {
  const [query, setQuery] = useState('')
  const results = useMemo(() => {
    const tokens = query.toLowerCase().split(/\s+/).filter(Boolean)
    if (tokens.length === 0) return []
    return SEARCH_INDEX.filter((e) => {
      const hay = `${e.title} ${e.keywords ?? ''} ${navItemFor(e.section)?.label ?? ''}`.toLowerCase()
      return tokens.every((t) => hay.includes(t))
    }).slice(0, 8)
  }, [query])

  const pick = (e: { section: Section; title: string }) => {
    onPick(e.section, e.title)
    setQuery('')
  }

  return (
    <div className="hidden flex-col gap-1 @4xl:flex">
      <div className="relative">
        <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setQuery('')
            if (e.key === 'Enter' && results[0]) pick(results[0])
          }}
          placeholder="Search settings…"
          aria-label="Search settings"
          className="h-8 w-full rounded-lg border border-border/80 bg-input/60 pl-8 pr-7 text-xs placeholder:text-muted-foreground focus-visible:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
        />
        {query && (
          <button type="button" aria-label="Clear search" className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-muted-foreground hover:text-foreground" onClick={() => setQuery('')}>
            <X className="size-3.5" />
          </button>
        )}
      </div>
      {query && (
        <div className="flex flex-col gap-0.5 rounded-lg border border-border/80 bg-card p-1">
          {results.length === 0 && <p className="px-2 py-1.5 text-xs text-muted-foreground">No matching setting.</p>}
          {results.map((r) => (
            <button
              key={`${r.section}:${r.title}`}
              type="button"
              onClick={() => pick(r)}
              className="flex flex-col items-start rounded-md px-2 py-1.5 text-left hover:bg-accent/50"
            >
              <span className="text-xs font-medium text-foreground">{r.title}</span>
              <span className="text-[10px] text-muted-foreground">{navItemFor(r.section)?.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

/** Tab bar for sidebar entries that hold several pages. */
export function SectionTabs({ item, section, onChange }: { item: NavItem; section: Section; onChange: (s: Section) => void }) {
  if (item.sections.length < 2) return null
  return (
    <div role="tablist" aria-label={item.label} className="flex flex-wrap gap-1 rounded-xl border border-border/60 bg-muted/30 p-1">
      {item.sections.map((s) => (
        <button
          key={s.id}
          role="tab"
          aria-selected={section === s.id}
          type="button"
          onClick={() => onChange(s.id)}
          className={cn(
            'rounded-lg px-3 py-1.5 text-xs font-medium transition-colors',
            section === s.id ? 'bg-background text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {s.tab}
        </button>
      ))}
    </div>
  )
}

/** A card title's own text, without badges or other elements nested inside it. */
function titleText(el: HTMLElement): string {
  const own = Array.from(el.childNodes)
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent ?? '')
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
  return own || (el.textContent ?? '').trim()
}

/** "On this page" bar: jump between the cards of a long settings page; the card in view is highlighted. */
export function OnThisPage({ root, resetKey }: { root: RefObject<HTMLElement | null>; resetKey: string }) {
  const [labels, setLabels] = useState<string[]>([])
  const [active, setActive] = useState(0)

  const query = useCallback(() => Array.from(root.current?.querySelectorAll<HTMLElement>('[data-card-title]') ?? []), [root])

  useEffect(() => {
    const el = root.current
    if (!el) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const scan = () => {
      const next = query().map(titleText).filter(Boolean)
      setLabels((prev) => (prev.length === next.length && prev.every((p, i) => p === next[i]) ? prev : next))
    }
    scan()
    const observer = new MutationObserver(() => {
      clearTimeout(timer)
      timer = setTimeout(scan, 150)
    })
    observer.observe(el, { childList: true, subtree: true })
    return () => {
      clearTimeout(timer)
      observer.disconnect()
    }
  }, [root, query, resetKey])

  // Scroll-spy: the last card whose title has scrolled past the top third of the view is "current".
  useEffect(() => {
    const titles = query()
    if (titles.length < 3) return
    const visible = new Set<number>()
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const idx = titles.indexOf(e.target as HTMLElement)
          if (e.isIntersecting) visible.add(idx)
          else visible.delete(idx)
        }
        if (visible.size > 0) setActive(Math.min(...visible))
      },
      { rootMargin: '0px 0px -60% 0px' },
    )
    titles.forEach((t) => io.observe(t))
    return () => io.disconnect()
  }, [query, labels])

  if (labels.length < 3) return null
  return (
    <nav
      aria-label="On this page"
      className="sticky top-0 z-10 flex flex-wrap items-center gap-x-1 gap-y-1 rounded-xl border border-border/70 bg-background px-2.5 py-1.5 shadow-sm"
    >
      <span className="flex shrink-0 items-center gap-1.5 pr-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        <ListTree className="size-3.5" /> On this page
      </span>
      <span className="mr-1 h-4 w-px shrink-0 bg-border" />
      {labels.map((label, i) => (
        <button
          key={`${i}:${label}`}
          type="button"
          aria-current={active === i ? 'true' : undefined}
          onClick={() => {
            setActive(i)
            query()[i]?.scrollIntoView({ block: 'start', behavior: 'smooth' })
          }}
          className={cn(
            'max-w-[14rem] truncate rounded-lg px-2.5 py-1 text-xs font-medium transition-colors',
            active === i
              ? 'bg-primary/15 text-foreground ring-1 ring-primary/50'
              : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
          )}
        >
          {label}
        </button>
      ))}
    </nav>
  )
}
