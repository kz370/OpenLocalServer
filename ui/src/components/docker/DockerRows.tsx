import { Copy } from 'lucide-react'
import type { ReactNode } from 'react'

import { cn } from '@/lib/utils'
import { notify } from '@/lib/notify'

/**
 * §166: the container / service list used on both Docker tabs. A CSS grid that adapts to
 * the list's own width (container queries), not the window's:
 * - wide: name · ports · status · actions on one line, with a header row;
 * - narrow: name and actions on the first line, ports and status under it.
 * Long names and image references wrap instead of pushing columns off-screen.
 */
const WIDE_COLS = '@3xl:grid-cols-[minmax(0,2.4fr)_minmax(0,1fr)_minmax(0,1.3fr)_auto]'

export function RowList({ first = 'Name', children }: { first?: string; children: ReactNode }) {
  return (
    <div className="@container">
      <div
        className={cn(
          'sticky top-0 z-10 hidden gap-x-4 border-b bg-[var(--dialog-surface)] px-4 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground @3xl:grid',
          WIDE_COLS,
        )}
      >
        <span>{first}</span>
        <span>Ports</span>
        <span>Status</span>
        <span className="text-right">Action</span>
      </div>
      <div className="divide-y">{children}</div>
    </div>
  )
}

export function GroupHeading({ children }: { children: ReactNode }) {
  return <div className="bg-muted/30 px-4 py-1.5 text-xs font-medium break-words text-muted-foreground">{children}</div>
}

export function Row({
  icon,
  title,
  subtitle,
  badge,
  ports,
  status,
  actions,
}: {
  icon?: ReactNode
  title: ReactNode
  subtitle?: ReactNode
  badge?: ReactNode
  ports: ReactNode
  status: ReactNode
  actions: ReactNode
}) {
  return (
    <div
      className={cn(
        'grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-4 gap-y-2 px-4 py-3 hover:bg-muted/20 @3xl:items-center',
        WIDE_COLS,
      )}
    >
      <div className="flex min-w-0 items-start gap-2 @3xl:col-start-1 @3xl:row-start-1">
        {icon && <span className="mt-0.5 shrink-0">{icon}</span>}
        <div className="min-w-0">
          <div className="font-medium break-words">{title}</div>
          {subtitle && <div className="text-xs break-all text-muted-foreground">{subtitle}</div>}
          {badge && <div className="mt-1 flex flex-wrap gap-1">{badge}</div>}
        </div>
      </div>
      <div className="col-start-2 row-start-1 flex justify-end @3xl:col-start-4">{actions}</div>
      {/* Narrow: ports and status share one wrapped line. Wide: they become grid columns. */}
      <div className="col-span-2 flex flex-wrap items-center gap-2 @3xl:contents">
        <div className="flex flex-wrap gap-1 @3xl:col-start-2 @3xl:row-start-1">{ports}</div>
        <div className="flex flex-col items-start gap-0.5 @3xl:col-start-3 @3xl:row-start-1">{status}</div>
      </div>
    </div>
  )
}

/** Short id with a copy button for the full id. */
export function CopyId({ id }: { id: string }) {
  return (
    <button
      type="button"
      title={`Copy the full container ID (${id})`}
      className="inline-flex cursor-pointer items-center gap-1 rounded px-1 font-mono text-[11px] text-muted-foreground hover:bg-muted hover:text-foreground"
      onClick={() =>
        void navigator.clipboard
          .writeText(id)
          .then(() => notify.success(id.slice(0, 12), { title: 'Container ID copied' }))
      }
    >
      {id.slice(0, 12)}
      <Copy className="size-3" />
    </button>
  )
}
