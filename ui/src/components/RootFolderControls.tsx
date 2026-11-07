import { FolderOpen, FolderTree } from 'lucide-react'

import { MiddleTruncate } from '@/components/ui/MiddleTruncate'
import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/select'
import { TableRow, TableSpanCell } from '@/components/ui/table'
import { PROXY_ROOT, type ListSort, type ListView } from '@/lib/rootFolder'

/** "Group by root folder" toggle and a sort choice, shared by the Projects and Sites tables. */
export function RootFolderControls({ view, onChange }: { view: ListView; onChange: (next: ListView) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        type="button"
        size="sm"
        variant={view.group ? 'secondary' : 'outline'}
        className="h-8 cursor-pointer gap-1.5"
        aria-pressed={view.group}
        onClick={() => onChange({ ...view, group: !view.group })}
        title="Put rows that live in the same parent folder under one heading, even when you added them by hand"
      >
        <FolderTree className="size-4" /> Group by root folder
      </Button>
      <Select
        aria-label="Sort the list"
        className="h-8 w-44"
        value={view.group ? 'root' : view.sort}
        disabled={view.group}
        onChange={(e) => onChange({ ...view, sort: e.target.value as ListSort })}
      >
        <option value="name">Sort: Name</option>
        <option value="root">Sort: Root folder</option>
      </Select>
    </div>
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
