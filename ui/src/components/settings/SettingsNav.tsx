import { ListTree, Search, Wrench, X } from 'lucide-react'
import { useEffect, useMemo, useState, type ComponentType, type RefObject } from 'react'

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

type SearchEntry = {
  section: Section
  /** What the result list shows. */
  title: string
  keywords?: string
  /** Text to scroll to on the page when it differs from `title`; `null` opens the page without scrolling. */
  match?: string | null
}

/**
 * Every setting on every Settings page, by the title shown on the page. Pages render only while
 * open, so this cannot be read from the DOM; keep it in step when a setting is added or renamed.
 */
const SETTING_ENTRIES: SearchEntry[] = [
  // General
  { section: 'general', title: 'Administrator helper', keywords: 'admin hosts dns service windows uac elevation' },
  { section: 'general', title: 'Confirm before exit', keywords: 'quit close ask' },
  { section: 'general', title: 'Confirm before Stop all', keywords: 'tray ask' },
  { section: 'general', title: 'Diagnostics', keywords: 'doctor problems' },
  { section: 'general', title: 'Automatically fix safe problems', keywords: 'diagnostics repair auto fix' },
  { section: 'general', title: 'Local API', keywords: 'rest http token api key' },
  { section: 'general', title: 'API port', match: 'Port', keywords: 'local api' },
  { section: 'general', title: 'What the API may do', match: 'What it may do', keywords: 'read only operate permissions local api' },
  { section: 'general', title: 'Windows integration, support and privacy', keywords: 'system telemetry analytics' },
  { section: 'general', title: 'Explorer right-click menu', keywords: 'context menu shell windows explorer' },
  { section: 'general', title: 'Support bundle', keywords: 'bug report logs zip doctor' },
  // Startup & tray
  { section: 'startup', title: 'Start with Windows', keywords: 'boot login autostart startup' },
  { section: 'startup', title: 'Start minimized to the tray', keywords: 'hidden background' },
  { section: 'startup', title: 'Closing the window keeps running in the tray', keywords: 'close to tray background' },
  { section: 'startup', title: 'Desktop notifications', keywords: 'toast alerts crash' },
  { section: 'startup', title: 'Start these services automatically', keywords: 'autostart mysql mariadb nginx apache redis' },
  { section: 'startup', title: 'Exit OpenLocalServer', keywords: 'quit stop all' },
  // Editors
  { section: 'editors', title: 'Editors', keywords: 'vscode vs code cursor phpstorm ide open with add editor refresh' },
  // Appearance
  { section: 'appearance', title: 'Mode', keywords: 'dark light system theme' },
  { section: 'appearance', title: 'Presets', keywords: 'theme' },
  { section: 'appearance', title: 'Accent Color', keywords: 'colour primary' },
  { section: 'appearance', title: 'Sidebar Icon Style', keywords: 'icons colorful monochrome' },
  { section: 'appearance', title: 'App Background', keywords: 'wallpaper image gradient angle' },
  { section: 'appearance', title: 'Live Preview Strip', keywords: 'preview' },
  { section: 'sidebar', title: 'Sidebar items', keywords: 'hide show pages menu shorter sidebar' },
  { section: 'sidebar', title: 'Dockerize in right-click menus', keywords: 'docker context menu' },
  { section: 'sidebar', title: 'Security section on the Dashboard', keywords: 'hide show dashboard vulnerabilities' },
  // Sites
  { section: 'sites', title: 'Sites and domains', keywords: 'domains' },
  { section: 'sites', title: 'Default sites folder', keywords: 'projects directory www htdocs' },
  { section: 'sites', title: 'Where new projects are created', keywords: 'quick apps git clone import location' },
  { section: 'sites', title: 'Create domains automatically', keywords: 'auto domains https laragon' },
  { section: 'sites', title: 'Default top-level domain', keywords: 'tld .local .test .localhost custom' },
  { section: 'sites', title: 'Watch sites folder', keywords: 'detect new projects watcher' },
  { section: 'roots', title: 'Root folders', keywords: 'watched projects add folder' },
  { section: 'roots', title: 'Ignored folders', keywords: 'skip exclude hide folder' },
  { section: 'deleted', title: 'Hidden sites', keywords: 'excluded deleted restore domain' },
  // Developer tools
  { section: 'developer', title: 'Databases', keywords: 'migration guard' },
  { section: 'developer', title: 'Save the database before migrations', keywords: 'migration guard backup rewind laravel symfony django prisma' },
  { section: 'developer', title: 'Migration backup size limit', match: 'Skip the copy when the database is bigger than (MB)', keywords: 'megabytes mb skip large database' },
  { section: 'developer', title: 'PHP dump viewer', keywords: 'dump dd debug laravel symfony' },
  // MCP / AI tools
  { section: 'mcp', title: 'MCP authentication', keywords: 'ai claude cursor assistant' },
  { section: 'mcp', title: 'Require MCP authentication', keywords: 'ai security' },
  { section: 'mcp', title: 'Bearer token', keywords: 'mcp token secret key' },
  { section: 'mcp', title: 'Token expiry', keywords: 'mcp hours expires' },
  // Resources
  { section: 'resources', title: 'Resources', keywords: 'limits' },
  { section: 'resources', title: 'CPU', keywords: 'cpulimit processor cap limit' },
  { section: 'resources', title: 'Database resources', keywords: 'memory ram mysql mariadb postgres' },
  { section: 'resources', title: 'Application resources', keywords: 'memory ram' },
  { section: 'resources', title: 'Faster installs on Windows', keywords: 'performance defender antivirus exclusion dev drive speed' },
  // Backups
  { section: 'backups', title: 'Back up automatically', keywords: 'auto backup schedule' },
  { section: 'backups', title: 'What to back up', keywords: 'sites snapshots databases' },
  { section: 'backups', title: 'Snapshot contents', keywords: 'project files env' },
  { section: 'backups', title: 'Which sites', keywords: 'backup scope' },
  { section: 'backups', title: 'How often', keywords: 'schedule cron period backup' },
  { section: 'backups', title: 'How many to keep', keywords: 'retention backup' },
  // Help & About
  { section: 'manual', title: 'User Manual', keywords: 'docs help guide' },
  { section: 'manual', title: 'Release notes', match: null, keywords: 'changelog what is new' },
  { section: 'about', title: 'Updates', keywords: 'check update download install version' },
  { section: 'about', title: 'About OLS', match: null, keywords: 'version license gpl' },
]

/** Every page and tab is searchable by its own name, too. */
const SEARCH_INDEX: SearchEntry[] = [
  ...NAV_GROUPS.flatMap((g) => g.items.flatMap((item) =>
    item.sections.map((s) => ({ section: s.id, title: item.sections.length > 1 ? s.tab : item.label, keywords: `${item.label} ${g.label} page`, match: null })),
  )),
  ...SETTING_ENTRIES,
]

/** Where a result lives, e.g. "Sites › Root folders". */
function locationOf(section: Section): string {
  const item = navItemFor(section)
  if (!item) return ''
  const tab = item.sections.find((s) => s.id === section)
  return item.sections.length > 1 && tab ? `${item.label} › ${tab.tab}` : item.label
}

/** The text an element shows itself, ignoring icons, badges and other nested elements. */
function ownText(el: Element): string {
  return Array.from(el.childNodes)
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent ?? '')
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
    .toLowerCase()
}

/** Scrolls to and briefly flashes the element on the page that shows `title`. */
export function jumpToSetting(root: HTMLElement | null, title: string): boolean {
  if (!root) return false
  const want = title.trim().toLowerCase()
  const all = Array.from(root.querySelectorAll<HTMLElement>('*'))
  const el =
    all.find((n) => ownText(n) === want) ??
    all.find((n) => n.children.length <= 2 && (n.textContent ?? '').trim().toLowerCase() === want)
  if (!el) return false
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

/** Search box for the settings sidebar. Wide layout only: the narrow icon rail has no room. */
export function SettingsSearch({ onPick }: { onPick: (section: Section, match: string | null) => void }) {
  const [query, setQuery] = useState('')
  const results = useMemo(() => {
    const q = query.trim().toLowerCase()
    const tokens = q.split(/\s+/).filter(Boolean)
    if (tokens.length === 0) return []
    const scored: { e: SearchEntry; score: number }[] = []
    for (const e of SEARCH_INDEX) {
      const title = e.title.toLowerCase()
      const hay = `${title} ${e.keywords ?? ''} ${locationOf(e.section)}`.toLowerCase()
      if (!tokens.every((t) => hay.includes(t))) continue
      const score = title.startsWith(q) ? 0 : title.includes(q) ? 1 : tokens.every((t) => title.includes(t)) ? 2 : 3
      scored.push({ e, score })
    }
    return scored.sort((a, b) => a.score - b.score).map((s) => s.e)
  }, [query])

  const pick = (e: SearchEntry) => {
    onPick(e.section, e.match === undefined ? e.title : e.match)
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
        <div className="flex max-h-[min(24rem,60vh)] flex-col gap-0.5 overflow-y-auto overscroll-contain rounded-lg border border-border/80 bg-card p-1 ols-scroll-thin">
          {results.length === 0 && <p className="px-2 py-1.5 text-xs text-muted-foreground">No matching setting.</p>}
          {results.length > 0 && (
            <p className="px-2 pb-0.5 pt-1 text-[10px] text-muted-foreground">
              {results.length} result{results.length === 1 ? '' : 's'}
            </p>
          )}
          {results.map((r) => (
            <button
              key={`${r.section}:${r.title}`}
              type="button"
              onClick={() => pick(r)}
              className="flex shrink-0 flex-col items-start rounded-md px-2 py-1.5 text-left hover:bg-accent/50"
            >
              <span className="text-xs font-medium text-foreground">{r.title}</span>
              <span className="text-[10px] text-muted-foreground">{locationOf(r.section)}</span>
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
  if (el.dataset.tocLabel) return el.dataset.tocLabel
  const own = Array.from(el.childNodes)
    .filter((n) => n.nodeType === Node.TEXT_NODE)
    .map((n) => n.textContent ?? '')
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
  return own || (el.textContent ?? '').trim()
}

function headingElements(root: RefObject<HTMLElement | null>): HTMLElement[] {
  return Array.from(root.current?.querySelectorAll<HTMLElement>('[data-card-title]') ?? [])
}

/** Card titles on the current settings page, kept current as cards mount and change. */
export function usePageHeadings(root: RefObject<HTMLElement | null>, resetKey: string): string[] {
  const [labels, setLabels] = useState<string[]>([])

  useEffect(() => {
    const el = root.current
    if (!el) return
    let timer: ReturnType<typeof setTimeout> | undefined
    const scan = () => {
      const next = headingElements(root).map(titleText).filter(Boolean)
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
  }, [root, resetKey])

  return labels
}

/**
 * "On this page" bar: jump between the cards of a long settings page; the card in view is
 * highlighted. Rendered outside the scrolling region, so content never passes underneath it.
 */
export function OnThisPage({ root, labels }: { root: RefObject<HTMLElement | null>; labels: string[] }) {
  const [active, setActive] = useState(0)

  // Scroll-spy: the first card title in the top 40% of the view is "current".
  useEffect(() => {
    const titles = headingElements(root)
    if (titles.length === 0) return
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
  }, [root, labels])

  useEffect(() => setActive(0), [labels])

  return (
    <nav
      aria-label="On this page"
      className="flex flex-wrap items-center gap-x-1 gap-y-1 rounded-xl border border-border/80 bg-card px-2.5 py-1.5"
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
            headingElements(root)[i]?.scrollIntoView({ block: 'start', behavior: 'smooth' })
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
