import type { ReactNode } from 'react'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * One icon-only control in a service row. Shared by the Dashboard services widget and the
 * Services page so both offer the same actions, the same titles and the same disabled
 * treatment — the two used to drift, and a page that looks able to restart a service but
 * cannot is worse than one that never offered it.
 */
export function ServiceIconAction({
  title,
  disabled,
  spinning,
  onClick,
  children,
  iconColor,
}: {
  title: string
  disabled: boolean
  spinning?: boolean
  onClick: () => void
  children: ReactNode
  iconColor?: string
}) {
  return (
    <Button
      size="sm"
      variant="ghost"
      className={cn(
        'size-7 shrink-0 cursor-pointer rounded-md p-0 transition-colors',
        // Enabled: the action's own colour, dimmed slightly until hover. Disabled:
        // flat grey with no hover response, so "you cannot do this right now" reads
        // at a glance instead of looking like a live button.
        disabled
          ? 'cursor-not-allowed bg-muted/30 text-muted-foreground/45 disabled:opacity-100 hover:bg-muted/30 hover:text-muted-foreground/45'
          : iconColor
            ? `opacity-70 hover:bg-muted/60 hover:opacity-100 ${iconColor} hover:${iconColor}`
            : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
      )}
      title={title}
      aria-label={title}
      disabled={disabled}
      onClick={onClick}
    >
      {spinning ? <Spinner className="size-3.5" /> : children}
    </Button>
  )
}
