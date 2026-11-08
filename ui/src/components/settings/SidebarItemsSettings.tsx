import { Lock } from 'lucide-react'
import { useState } from 'react'

import { DASHBOARD, LOCKED_NAV_ITEMS, NAV_GROUPS, type NavItem, type Page, SETTINGS } from '@/components/layout/navItems'
import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import type { Diagnostic } from '@/core'
import { setDockerizeMenu, useDockerizeMenuSetting } from '@/lib/dockerizeMenu'
import { setHiddenSidebarItems, useHiddenSidebarItems } from '@/lib/sidebarVisibility'

/**
 * Settings → Sidebar: choose which pages the sidebar lists. Locked items
 * (`LOCKED_NAV_ITEMS`) always show. Changes apply at once; there is no Save button.
 */
export function SidebarItemsSettings() {
  const hidden = useHiddenSidebarItems()
  const dockerizeMenu = useDockerizeMenuSetting()
  const dockerHidden = hidden.has('docker')
  const [error, setError] = useState<Diagnostic | null>(null)

  const sections: { id: string; label: string; items: NavItem[] }[] = [
    { id: 'top', label: 'Top', items: [DASHBOARD] },
    ...NAV_GROUPS,
    { id: 'bottom', label: 'Bottom', items: [SETTINGS] },
  ]

  const save = (next: Set<Page>) => {
    setError(null)
    setHiddenSidebarItems(next).catch((err) => setError(asDiagnostic(err)))
  }

  const toggle = (id: Page, show: boolean) => {
    const next = new Set(hidden)
    if (show) next.delete(id)
    else next.add(id)
    save(next)
  }

  return (
    // A container query, not a viewport one: Settings has its own menu beside this card,
    // so the window can be wide while the card is narrow.
    <Card className="@container">
      <CardHeader className="flex flex-row items-start justify-between gap-3 pb-3">
        <div>
          <CardTitle className="text-base font-semibold">Sidebar items</CardTitle>
          <CardDescription>
            Hide pages you don&apos;t use to keep the sidebar short. Hidden pages still open from search (Ctrl K) and from
            links. Items with a lock are always shown.
          </CardDescription>
        </div>
        <Button
          size="sm"
          variant="outline"
          className="shrink-0 cursor-pointer"
          disabled={hidden.size === 0}
          onClick={() => save(new Set())}
        >
          Show all
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-x-6 gap-y-4 @2xl:grid-cols-2">
          {sections.map((section) => (
            <div key={section.id} className="flex flex-col gap-1">
              <span className="px-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                {section.label}
              </span>
              {section.items.map(({ id, label, icon: Icon }) => {
                const locked = LOCKED_NAV_ITEMS.has(id)
                const shown = locked || !hidden.has(id)
                return (
                  <div
                    key={id}
                    className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/60 px-3 py-2"
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2.5 text-sm">
                      <Icon className="size-4" />
                      <span className="truncate">{label}</span>
                    </span>
                    {locked ? (
                      <span
                        className="flex shrink-0 items-center gap-1 whitespace-nowrap text-xs text-muted-foreground"
                        title={id === 'settings' ? 'Always shown: hidden items are brought back from here' : 'Always shown: a core page'}
                      >
                        <Lock className="size-3.5" /> Always shown
                      </span>
                    ) : (
                      <Switch
                        size="sm"
                        checked={shown}
                        label={`${shown ? 'Hide' : 'Show'} ${label} in the sidebar`}
                        onChange={(v) => toggle(id, v)}
                      />
                    )}
                  </div>
                )
              })}
            </div>
          ))}
        </div>
        <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/60 px-3 py-2">
          <span className="min-w-0 flex-1 text-sm">
            <span className="block">Dockerize in right-click menus</span>
            <span className="block text-xs text-muted-foreground">
              Adds Dockerize… to the Projects and Sites menus.
              {dockerHidden ? ' Hidden now because the Docker page is hidden from the sidebar.' : ''}
            </span>
          </span>
          <Switch
            size="sm"
            checked={dockerizeMenu && !dockerHidden}
            disabled={dockerHidden}
            label={`${dockerizeMenu ? 'Hide' : 'Show'} Dockerize in right-click menus`}
            onChange={(v) => setDockerizeMenu(v).catch((err) => setError(asDiagnostic(err)))}
          />
        </div>
      </CardContent>
    </Card>
  )
}
