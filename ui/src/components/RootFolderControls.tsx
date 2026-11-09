import { Check, FolderOpen, FolderTree, ListFilter } from 'lucide-react'
import { useState } from 'react'

import { MiddleTruncate } from '@/components/ui/MiddleTruncate'
import { Button } from '@/components/ui/button'
import { FloatingMenu, type MenuAnchor, type MenuItem } from '@/components/ui/menu'
import { TableRow, TableSpanCell } from '@/components/ui/table'
import { PROXY_ROOT, type ListView } from '@/lib/rootFolder'

/**
 * One "View" button for the Projects and Sites tables: group rows by root folder, and the
 * sort order. A menu instead of a toggle plus a drop-down keeps the toolbar on one line.
 */
export function RootFolderControls({ view, onChange }: { view: ListView; onChange: (next: ListView) => void }) {
  const [anchor, setAnchor] = useState<MenuAnchor | null>(null)
  const items: MenuItem[] = [
    {
      label: 'Group by root folder',
      icon: view.group ? <Check /> : <FolderTree />,
      hint: 'Put rows that live in the same parent folder under one heading',
      // Ungrouping goes back to sorting by name; grouping sorts by root folder by itself.
      onSelect: () => onChange({ ...view, group: !view.group, sort: view.group ? 'name' : view.sort }),
    },
    'separator',
    { label: 'Sort by name', icon: !view.group && view.sort === 'name' ? <Check /> : undefined, disabled: view.group, hint: view.group ? 'Grouping sorts by root folder' : undefined, onSelect: () => onChange({ ...view, sort: 'name' }) },
    { label: 'Sort by root folder', icon: view.group || view.sort === 'root' ? <Check /> : undefined, disabled: view.group, onSelect: () => onChange({ ...view, sort: 'root' }) },
  ]
  return (
    <>
      <Button
        type="button"
        size="sm"
        variant="outline"
        className={`h-8 cursor-pointer gap-1.5 ${anchor ? 'bg-accent' : ''}`}
        aria-haspopup="menu"
        title="Group and sort the list"
        onClick={(e) => {
          const el = e.currentTarget
          setAnchor(anchor ? null : { type: 'button', rect: el.getBoundingClientRect(), element: el })
        }}
      >
        <ListFilter className="size-4" /> View
        {view.group && <span className="rounded-full bg-primary/15 px-1.5 text-[11px] text-primary">grouped</span>}
      </Button>
      {anchor && <FloatingMenu anchor={anchor} placement="bottom-end" items={items} onClose={() => setAnchor(null)} />}
    </>
  )
}

/** The heading row above a run of rows that share a root folder. */
export function RootGroupRow({ root, count, colSpan }: { root: string; count: number; colSpan: number }) {
  return (
    <TableRow className="bg-muted/40 hover:bg-muted/40" data-root-group>
      <TableSpanCell colSpan={colSpan} className="py-1.5 pl-4">
        <div className="flex min-w-0 items-center gap-2 text-xs font-medium text-muted-foreground">
          <FolderOpen className="size-3.5 shrink-0" />
          {root === PROXY_ROOT ? (
            <span>Proxy</span>
          ) : root ? (
            <MiddleTruncate className="max-w-2xl flex-initial font-mono" text={root} />
          ) : (
            <span>No folder</span>
          )}
          <span className="shrink-0 rounded bg-background/60 px-1.5 py-0.5 text-[10px] tabular-nums">{count}</span>
        </div>
      </TableSpanCell>
    </TableRow>
  )
}
