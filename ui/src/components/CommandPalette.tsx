import { CornerDownLeft, Search } from 'lucide-react'
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type { Page } from '@/components/layout/Sidebar'
import { Spinner } from '@/components/Spinner'
import { type CoreCommand, type SearchHit, runCommand } from '@/core'
import { asDiagnostic } from '@/components/ErrorCard'
import { openProject } from '@/lib/nav'
import { cn } from '@/lib/utils'

interface Action {
  id: string
  label: string
  group: string
  hint?: string
  run: () => unknown
}

const PAGES: [Page, string][] = [
  ['dashboard', 'Dashboard'],
  ['projects', 'Projects'],
  ['quickapps', 'Quick Apps'],
  ['commands', 'Commands'],
  ['domains', 'Sites'],
  ['config', 'Web config'],
  ['tunnels', 'Tunnels'],
  ['databases', 'Databases'],
  ['services', 'Services'],
  ['runtimes', 'Runtimes'],
  ['profiles', 'Profiles'],
  ['logs', 'Logs'],
  ['processes', 'Processes'],
  ['settings', 'Settings'],
]

const KIND_LABEL: Record<SearchHit['kind'], string> = {
  project: 'Project',
  service: 'Service',
  site: 'Site',
  quick_app: 'Quick App',
  quick_command: 'Quick Command',
  runtime: 'Runtime',
  tunnel: 'Tunnel',
  config: 'Web config',
  log: 'Log',
}

const KIND_PAGE: Record<SearchHit['kind'], Page> = {
  project: 'projects',
  service: 'services',
  site: 'domains',
  quick_app: 'quickapps',
  quick_command: 'commands',
  runtime: 'runtimes',
  tunnel: 'tunnels',
  config: 'config',
  log: 'logs',
}

function matches(query: string, text: string) {
  const t = text.toLowerCase()
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => t.includes(w))
}

/**
 * §122 command palette (Ctrl+Shift+P) and §123 global search (Ctrl+K): one box. Typing
 * filters the actions and also searches projects, services, sites, Quick Apps and
 * Commands, runtimes, web configs and logs.
 */
export function CommandPalette({ onNavigate, onDoctor }: { onNavigate: (p: Page) => void; onDoctor: () => void }) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [actions, setActions] = useState<Action[]>([])
  const [hits, setHits] = useState<SearchHit[]>([])
  const [searching, setSearching] = useState(false)
  const [active, setActive] = useState(0)
  const [status, setStatus] = useState<{ text: string; ok: boolean } | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase()
      if ((e.ctrlKey && e.shiftKey && k === 'p') || (e.ctrlKey && !e.shiftKey && k === 'k')) {
        e.preventDefault()
        setOpen(true)
      }
    }
    const onAsk = () => setOpen(true)
    window.addEventListener('keydown', onKey)
    window.addEventListener('ols:palette', onAsk)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('ols:palette', onAsk)
    }
  }, [])

  const exec = useCallback(async (label: string, cmd: CoreCommand) => {
    setStatus({ text: `${label}…`, ok: true })
    try {
      await runCommand(cmd)
      setStatus({ text: `${label}: done`, ok: true })
    } catch (e) {
      const d = asDiagnostic(e)
      setStatus({ text: `${label}: ${d.cause || d.problem}`, ok: false })
    }
  }, [])

  // The actions depend on what exists right now, so they are built each time it opens.
  useEffect(() => {
    if (!open) return
    setQuery('')
    setHits([])
    setActive(0)
    setStatus(null)
    setTimeout(() => input.current?.focus(), 0)
    const go = (page: Page) => () => {
      onNavigate(page)
      setOpen(false)
    }
    void (async () => {
      const [projects, services, catalog, tunnels, apps] = await Promise.all([
        runCommand({ type: 'list_projects' }),
        runCommand({ type: 'list_services' }),
        runCommand({ type: 'list_runtime_catalog' }),
        runCommand({ type: 'list_tunnels' }),
        runCommand({ type: 'list_quick_apps' }),
      ])
      const a: Action[] = [
        { id: 'doctor', label: 'Run the doctor', group: 'Environment', hint: 'Check everything', run: () => { setOpen(false); onDoctor() } },
        { id: 'repair', label: 'Repair environment', group: 'Environment', hint: 'Fix what is safe', run: () => { setOpen(false); onDoctor() } },
        { id: 'apply-web', label: 'Apply web config and start the web server', group: 'Environment', run: () => exec('Apply web config', { type: 'apply_web', overwrite: [] }) },
        { id: 'stop-web', label: 'Stop the web server', group: 'Environment', run: () => exec('Stop web server', { type: 'stop_web' }) },
        { id: 'open-mailpit', label: 'Open Mailpit', group: 'Environment', run: () => exec('Open Mailpit', { type: 'open_url', url: 'http://127.0.0.1:8025' }) },
        { id: 'open-nginx', label: 'Open web server config', group: 'Environment', run: go('config') },
        { id: 'new-quick-app', label: 'Create a project from a Quick App', group: 'Environment', run: go('quickapps') },
      ]
      if (projects.type === 'projects') {
        for (const p of projects.projects) {
          a.push({ id: `open:${p.id}`, label: `Open ${p.name}`, group: 'Projects', hint: p.path, run: () => { openProject({ id: p.id }); onNavigate('projects'); setOpen(false) } })
          a.push({ id: `setup:${p.id}`, label: `Start ${p.name}`, group: 'Projects', hint: 'Plan and apply its environment', run: () => { openProject({ id: p.id, tab: 'environment' }); onNavigate('projects'); setOpen(false) } })
          a.push({ id: `diag:${p.id}`, label: `Diagnose ${p.name}`, group: 'Projects', run: () => { openProject({ id: p.id, tab: 'repair' }); onNavigate('projects'); setOpen(false) } })
          a.push({ id: `git:${p.id}`, label: `Git: ${p.name}`, group: 'Projects', run: () => { openProject({ id: p.id, tab: 'git' }); onNavigate('projects'); setOpen(false) } })
          a.push({ id: `workers:${p.id}`, label: `Start ${p.name} workers`, group: 'Projects', run: () => exec(`Start ${p.name} workers`, { type: 'start_project_workers', project_id: p.id }) })
          a.push({ id: `terminal:${p.id}`, label: `Open terminal in ${p.name}`, group: 'Projects', run: () => { openProject({ id: p.id, tab: 'terminal' }); onNavigate('projects'); setOpen(false) } })
        }
      }
      if (services.type === 'services') {
        for (const s of services.services.filter((x) => x.installed)) {
          a.push(
            s.running
              ? { id: `stop:${s.id}`, label: `Stop ${s.name}`, group: 'Services', run: () => exec(`Stop ${s.name}`, { type: 'stop_service', id: s.id }) }
              : { id: `start:${s.id}`, label: `Start ${s.name}`, group: 'Services', run: () => exec(`Start ${s.name}`, { type: 'start_service', id: s.id }) },
          )
        }
      }
      if (catalog.type === 'runtime_catalog') {
        for (const r of catalog.entries.filter((x) => !x.installed)) {
          a.push({ id: `install:${r.id}:${r.version}`, label: `Install ${r.name} ${r.version}`, group: 'Runtimes', run: () => exec(`Install ${r.name} ${r.version}`, { type: 'install_runtime', id: r.id, version: r.version }) })
        }
      }
      if (tunnels.type === 'tunnels') {
        for (const t of tunnels.tunnels) {
          a.push(
            t.state === 'stopped' || t.state === 'needs_confirmation'
              ? { id: `tunnel:${t.config.id}`, label: `Start tunnel ${t.config.name}`, group: 'Tunnels', hint: 'Opens the Tunnels page to confirm', run: go('tunnels') }
              : { id: `tunnel:${t.config.id}`, label: `Stop tunnel ${t.config.name}`, group: 'Tunnels', run: () => exec(`Stop tunnel ${t.config.name}`, { type: 'stop_tunnel', id: t.config.id }) },
          )
        }
      }
      if (apps.type === 'quick_apps') {
        for (const q of apps.apps) a.push({ id: `qa:${q.id}`, label: `Create ${q.name}`, group: 'Quick Apps', run: go('quickapps') })
      }
      for (const [page, label] of PAGES) a.push({ id: `page:${page}`, label: `Go to ${label}`, group: 'Pages', run: go(page) })
      setActions(a)
    })()
  }, [open, onNavigate, onDoctor, exec])

  // Global search, debounced.
  useEffect(() => {
    if (!open || query.trim().length < 2) {
      setHits([])
      return
    }
    setSearching(true)
    const id = setTimeout(async () => {
      const r = await runCommand({ type: 'global_search', query }).catch(() => null)
      if (r?.type === 'search_results') setHits(r.hits)
      setSearching(false)
    }, 200)
    return () => clearTimeout(id)
  }, [query, open])

  const shownActions = useMemo(() => (query.trim() ? actions.filter((a) => matches(query, `${a.label} ${a.group}`)).slice(0, 12) : actions.slice(0, 10)), [actions, query])
  const items: { key: string; node: ReactNode; run: () => unknown; group: string }[] = [
    ...shownActions.map((a) => ({
      key: a.id,
      group: a.group,
      run: a.run,
      node: (
        <>
          <span className="min-w-0 flex-1 truncate">{a.label}</span>
          {a.hint && <span className="shrink-0 truncate text-xs text-muted-foreground">{a.hint}</span>}
        </>
      ),
    })),
    ...hits.map((h, i) => ({
      key: `hit:${h.kind}:${h.target}:${i}`,
      group: `Search · ${KIND_LABEL[h.kind]}`,
      run: () => {
        if (h.kind === 'project') openProject({ id: h.target })
        onNavigate(KIND_PAGE[h.kind])
        setOpen(false)
      },
      node: (
        <span className="min-w-0 flex-1">
          <span className="block truncate">{h.title}</span>
          <span className="block truncate text-xs text-muted-foreground">{h.excerpt ?? h.subtitle}</span>
        </span>
      ),
    })),
  ]

  useEffect(() => setActive(0), [query])
  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [active])

  if (!open) return null
  let lastGroup = ''
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center p-4 pt-[12vh]">
      <div className="absolute inset-0 bg-black/40" onClick={() => setOpen(false)} />
      <div role="dialog" aria-modal="true" aria-label="Command palette" className="relative flex max-h-[70vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl">
        <div className="flex items-center gap-2 border-b border-border px-4">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            ref={input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setOpen(false)
              else if (e.key === 'ArrowDown') {
                e.preventDefault()
                setActive((i) => Math.min(items.length - 1, i + 1))
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setActive((i) => Math.max(0, i - 1))
              } else if (e.key === 'Enter') {
                e.preventDefault()
                void items[active]?.run()
              }
            }}
            placeholder="Type a command, or search projects, sites, services, logs…"
            className="h-12 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            aria-label="Command or search"
          />
          {searching && <Spinner className="size-4 text-muted-foreground" />}
        </div>
        <div ref={list} className="flex-1 overflow-y-auto p-1.5">
          {items.length === 0 && <p className="px-3 py-6 text-center text-sm text-muted-foreground">{query.trim().length < 2 ? 'Type to search.' : 'Nothing found.'}</p>}
          {items.map((it, i) => {
            const header = it.group !== lastGroup ? it.group : null
            lastGroup = it.group
            return (
              <div key={it.key}>
                {header && <p className="px-3 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">{header}</p>}
                <button
                  data-index={i}
                  onMouseMove={() => setActive(i)}
                  onClick={() => void it.run()}
                  className={cn('flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm', i === active ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50')}
                >
                  {it.node}
                  {i === active && <CornerDownLeft className="size-3.5 shrink-0 text-muted-foreground" />}
                </button>
              </div>
            )
          })}
        </div>
        <div className="flex items-center justify-between gap-3 border-t border-border px-4 py-2 text-xs text-muted-foreground">
          <span className={cn('truncate', status && !status.ok && 'text-destructive')}>{status?.text ?? '↑↓ to move · Enter to run · Esc to close'}</span>
          <span className="shrink-0">Ctrl+Shift+P · Ctrl+K</span>
        </div>
      </div>
    </div>
  )
}
