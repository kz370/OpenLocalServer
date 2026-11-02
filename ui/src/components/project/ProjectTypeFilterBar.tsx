import {
  autoUpdate,
  flip,
  FloatingPortal,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
} from '@floating-ui/react'
import { ChevronDown, FolderX, Pin, Search, SlidersHorizontal, X } from 'lucide-react'
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { ProjectTypeTab } from '@/lib/projectTypes'
import { cn } from '@/lib/utils'

export interface ProjectTypeFilterBarProps {
  tabs: ProjectTypeTab[]
  selectedTab: string
  onSelectTab: (id: string) => void
  missingCount: number
  missingOnly: boolean
  onToggleMissingOnly: () => void
  onOpenManageTypes: () => void
  pinnedIds?: string[]
}

const GAP = 6 // 1.5 in tailwind = 6px
const DEFAULT_MORE_BTN_WIDTH = 84

export function ProjectTypeFilterBar({
  tabs,
  selectedTab,
  onSelectTab,
  missingCount,
  missingOnly,
  onToggleMissingOnly,
  onOpenManageTypes,
  pinnedIds = [],
}: ProjectTypeFilterBarProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const chipsContainerRef = useRef<HTMLDivElement>(null)
  const rightControlsRef = useRef<HTMLDivElement>(null)
  const measureContainerRef = useRef<HTMLDivElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)

  const [availableWidth, setAvailableWidth] = useState<number>(0)
  const [measuredWidths, setMeasuredWidths] = useState<Record<string, number>>({})
  const [moreButtonWidth, setMoreButtonWidth] = useState<number>(DEFAULT_MORE_BTN_WIDTH)

  const [popoverOpen, setPopoverOpen] = useState(false)
  const [popoverSearch, setPopoverSearch] = useState('')

  const pinnedSet = useMemo(() => new Set(pinnedIds.map((id) => id.toLowerCase())), [pinnedIds])

  // Measure tab widths offscreen
  useLayoutEffect(() => {
    if (!measureContainerRef.current) return

    const widths: Record<string, number> = {}
    const elements = measureContainerRef.current.querySelectorAll<HTMLElement>('[data-measure-id]')
    elements.forEach((el) => {
      const id = el.getAttribute('data-measure-id')
      if (id) {
        const w = el.getBoundingClientRect().width
        if (id === '__more__') {
          if (w > 0) setMoreButtonWidth(w)
        } else {
          widths[id] = w
        }
      }
    })
    setMeasuredWidths(widths)
  }, [tabs])

  // ResizeObserver on the chips container
  useEffect(() => {
    const el = chipsContainerRef.current
    if (!el || typeof ResizeObserver === 'undefined') return

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const width = entry.contentRect.width
        if (width > 0) {
          setAvailableWidth(width)
        }
      }
    })

    observer.observe(el)
    setAvailableWidth(el.clientWidth)

    return () => observer.disconnect()
  }, [])

  // Calculate visible vs overflow chips
  const { visibleTabs, overflowTabs } = useMemo(() => {
    if (tabs.length === 0) {
      return { visibleTabs: [], overflowTabs: [] }
    }

    const allTab = tabs.find((t) => t.id === 'all') ?? tabs[0]
    const otherTabs = tabs.filter((t) => t.id !== allTab.id)

    // If availableWidth is not yet determined or large enough, check if all tabs fit
    const totalRequiredWidth = tabs.reduce((acc, tab) => {
      const w = measuredWidths[tab.id] ?? 80
      return acc + w + GAP
    }, -GAP)

    if (availableWidth > 0 && totalRequiredWidth <= availableWidth) {
      return { visibleTabs: tabs, overflowTabs: [] }
    }

    // Space reserved for the +N more button
    const maxChipsWidth = Math.max(availableWidth - moreButtonWidth - GAP, 80)

    const visible: ProjectTypeTab[] = [allTab]
    const overflow: ProjectTypeTab[] = []

    const allWidth = measuredWidths[allTab.id] ?? 50
    let currentWidth = allWidth

    for (const tab of otherTabs) {
      const w = measuredWidths[tab.id] ?? 80
      if (currentWidth + GAP + w <= maxChipsWidth) {
        visible.push(tab)
        currentWidth += GAP + w
      } else {
        overflow.push(tab)
      }
    }

    // Swap rule: "swap a type picked from the popover into the visible row"
    if (selectedTab !== 'all') {
      const overflowIdx = overflow.findIndex((t) => t.id === selectedTab)
      if (overflowIdx !== -1) {
        const selectedItem = overflow[overflowIdx]
        if (visible.length > 1) {
          // Replace the last unpinned item in visible, or the last item if none are unpinned
          let swapIdx = visible.length - 1
          for (let i = visible.length - 1; i >= 1; i--) {
            if (!pinnedSet.has(visible[i].id.toLowerCase())) {
              swapIdx = i
              break
            }
          }
          const displaced = visible[swapIdx]
          visible[swapIdx] = selectedItem
          overflow[overflowIdx] = displaced
        } else {
          visible.push(selectedItem)
          overflow.splice(overflowIdx, 1)
        }
      }
    }

    return { visibleTabs: visible, overflowTabs: overflow }
  }, [tabs, measuredWidths, availableWidth, moreButtonWidth, selectedTab, pinnedSet])

  // Floating UI popover
  const { refs, floatingStyles, context } = useFloating({
    open: popoverOpen,
    onOpenChange: (open) => {
      setPopoverOpen(open)
      if (!open) setPopoverSearch('')
    },
    placement: 'bottom-start',
    middleware: [offset(4), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
  })

  const dismiss = useDismiss(context)
  const { getReferenceProps, getFloatingProps } = useInteractions([dismiss])

  useEffect(() => {
    if (popoverOpen) {
      // Focus search on open
      requestAnimationFrame(() => {
        searchInputRef.current?.focus()
      })
    }
  }, [popoverOpen])

  const filteredOverflow = useMemo(() => {
    const term = popoverSearch.trim().toLowerCase()
    if (!term) return overflowTabs
    return overflowTabs.filter((t) => t.label.toLowerCase().includes(term))
  }, [overflowTabs, popoverSearch])

  return (
    <>
      {/* Hidden container for accurate width measurement */}
      <div
        ref={measureContainerRef}
        aria-hidden="true"
        className="absolute left-[-9999px] top-[-9999px] pointer-events-none opacity-0 flex items-center gap-1.5 whitespace-nowrap"
      >
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            data-measure-id={tab.id}
            className="rounded-md px-2.5 py-1.5 text-xs font-medium flex items-center gap-1.5 shrink-0"
          >
            <TechIcon id={tab.icon} className="size-3.5 shrink-0" />
            <span>{tab.label}</span>
            <span className="ml-0.5 text-[10px] opacity-75">{tab.count}</span>
          </button>
        ))}
        <button
          type="button"
          data-measure-id="__more__"
          className="rounded-md px-2.5 py-1.5 text-xs font-medium flex items-center gap-1 shrink-0"
        >
          <span>+{Math.max(overflowTabs.length, 99)} more</span>
          <ChevronDown className="size-3.5" />
        </button>
      </div>

      {/* Main filter row */}
      <div ref={containerRef} className="flex items-center justify-between w-full min-w-0 gap-2">
        {/* Chips row */}
        <div ref={chipsContainerRef} className="flex items-center gap-1.5 min-w-0 flex-1 overflow-hidden">
          {visibleTabs.map((tab) => {
            const isSelected = selectedTab === tab.id
            return (
              <button
                key={tab.id}
                type="button"
                onClick={() => onSelectTab(tab.id)}
                className={cn(
                  'cursor-pointer rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors flex items-center gap-1.5 shrink-0 select-none',
                  isSelected
                    ? 'bg-primary text-[var(--on-accent,var(--primary-foreground))] shadow-xs font-semibold'
                    : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground',
                )}
                title={`${tab.label} (${tab.count} project${tab.count === 1 ? '' : 's'})`}
              >
                <TechIcon id={tab.icon} className="size-3.5 shrink-0" />
                <span>{tab.label}</span>
                <span
                  className={cn(
                    'ml-0.5 text-[10px] opacity-80',
                    isSelected ? 'text-[var(--on-accent,var(--primary-foreground))]' : 'text-muted-foreground',
                  )}
                >
                  {tab.count}
                </span>
              </button>
            )
          })}

          {/* +N more popover button */}
          {overflowTabs.length > 0 && (
            <div className="shrink-0 relative">
              <button
                ref={refs.setReference}
                {...getReferenceProps()}
                type="button"
                onClick={() => setPopoverOpen((prev) => !prev)}
                className={cn(
                  'cursor-pointer rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors flex items-center gap-1 shrink-0 border border-border/80 bg-background hover:bg-muted/70 text-foreground',
                  popoverOpen && 'bg-muted border-primary/50 text-foreground',
                )}
                title={`${overflowTabs.length} more project types`}
              >
                <span>+{overflowTabs.length} more</span>
                <ChevronDown className={cn('size-3.5 transition-transform duration-150', popoverOpen && 'rotate-180')} />
              </button>

              {popoverOpen && (
                <FloatingPortal>
                  <div
                    ref={refs.setFloating}
                    style={floatingStyles}
                    {...getFloatingProps()}
                    className="z-50 w-64 rounded-lg border border-border bg-popover p-1.5 shadow-lg text-popover-foreground outline-none animate-in fade-in-0 zoom-in-95"
                  >
                    {/* Search box inside popover */}
                    <div className="relative mb-1.5 px-0.5 pt-0.5">
                      <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
                      <Input
                        ref={searchInputRef}
                        value={popoverSearch}
                        onChange={(e) => setPopoverSearch(e.target.value)}
                        placeholder="Search types..."
                        className="h-8 pl-8 pr-7 text-xs"
                      />
                      {popoverSearch && (
                        <button
                          type="button"
                          onClick={() => setPopoverSearch('')}
                          className="absolute right-2 top-1/2 -translate-y-1/2 size-4 text-muted-foreground hover:text-foreground flex items-center justify-center cursor-pointer"
                        >
                          <X className="size-3" />
                        </button>
                      )}
                    </div>

                    {/* Overflow items list */}
                    <div className="flex flex-col gap-0.5 max-h-56 overflow-y-auto pr-0.5">
                      {filteredOverflow.length === 0 ? (
                        <div className="py-4 text-center text-xs text-muted-foreground">
                          No matching types
                        </div>
                      ) : (
                        filteredOverflow.map((tab) => {
                          const isPinned = pinnedSet.has(tab.id.toLowerCase())
                          const isSelected = selectedTab === tab.id
                          return (
                            <button
                              key={tab.id}
                              type="button"
                              onClick={() => {
                                onSelectTab(tab.id)
                                setPopoverOpen(false)
                              }}
                              className={cn(
                                'flex items-center justify-between w-full rounded-md px-2 py-1.5 text-xs font-medium transition-colors cursor-pointer text-left',
                                isSelected
                                  ? 'bg-primary/10 text-primary font-semibold'
                                  : 'text-foreground hover:bg-muted/70',
                              )}
                            >
                              <div className="flex items-center gap-2 min-w-0 flex-1 mr-2">
                                <TechIcon id={tab.icon} className="size-3.5 shrink-0" />
                                <span className="truncate">{tab.label}</span>
                                {isPinned && (
                                  <Pin className="size-2.5 text-primary/70 fill-primary/70 shrink-0" />
                                )}
                              </div>
                              <Badge
                                variant="secondary"
                                className="text-[10px] px-1.5 py-0 font-normal shrink-0"
                              >
                                {tab.count}
                              </Badge>
                            </button>
                          )
                        })
                      )}
                    </div>
                  </div>
                </FloatingPortal>
              )}
            </div>
          )}
        </div>

        {/* Right pinned controls: Missing folders filter & Manage types */}
        <div ref={rightControlsRef} className="flex items-center gap-1.5 shrink-0 ml-auto">
          {missingCount > 0 && (
            <button
              type="button"
              onClick={onToggleMissingOnly}
              className={cn(
                'cursor-pointer rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors flex items-center gap-1.5 shrink-0',
                missingOnly
                  ? 'bg-destructive/20 text-destructive border border-destructive/40 shadow-xs'
                  : 'text-destructive/80 hover:bg-destructive/10',
              )}
            >
              <FolderX className="size-3.5" />
              <span>Folder not found</span>
              <Badge variant="destructive" className="ml-1 px-1 py-0 text-[10px]">
                {missingCount}
              </Badge>
            </button>
          )}

          <Button
            variant="outline"
            size="sm"
            onClick={onOpenManageTypes}
            className="h-8 px-2.5 text-xs cursor-pointer gap-1.5 shrink-0"
            title="Manage custom project types"
          >
            <SlidersHorizontal className="size-3.5" />
            <span>Manage types</span>
          </Button>
        </div>
      </div>
    </>
  )
}
