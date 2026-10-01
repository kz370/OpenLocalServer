import { Check } from 'lucide-react'
import type { ComponentProps } from 'react'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/** What `<SaveButton>` needs from `useAction`: which key is running, which last succeeded. */
export type SaveAction = { busy: string | null; saved: string | null }

export interface SaveButtonProps extends Omit<ComponentProps<typeof Button>, 'children'> {
  /** The `useAction()` handle the click runs through. */
  action: SaveAction
  /** The key this button's click uses in `run(...)`. */
  name: string
  /** What the button says. Hidden while running and while the saved mark is up. */
  children: React.ReactNode
  /** Word shown while the command is in flight, e.g. "Saving…". Defaults to "Working…". */
  busyLabel?: string
  /** Word shown after it succeeded, e.g. "Saved". Defaults to "Saved". */
  savedLabel?: string
}

/**
 * A button that commits something, and says so.
 *
 * The problem it solves is that most of these commands answer in well under a second: the
 * button is pressed, the label does not change, the list underneath does not visibly move, and
 * a user who was not looking concludes nothing happened and presses again. So this one holds
 * three states — running, saved, idle — and the saved state is timed (`SAVED_FLASH_MS`) rather
 * than permanent, because a permanent "Saved" on a button that has not been pressed since is a
 * lie.
 *
 * `disabled` still wins over the saved mark, so a button that is disabled for another reason
 * does not claim credit for a save.
 */
export function SaveButton({
  action,
  name,
  children,
  busyLabel = 'Working…',
  savedLabel = 'Saved',
  className,
  disabled,
  ...props
}: SaveButtonProps) {
  const running = action.busy === name
  const justSaved = !running && !disabled && action.saved === name
  return (
    <Button
      {...props}
      disabled={disabled || action.busy !== null}
      data-state={running ? 'running' : justSaved ? 'saved' : 'idle'}
      className={cn('min-w-[8.5rem] transition-colors', justSaved && 'bg-success text-success-foreground hover:bg-success/90', className)}
    >
      {running ? (
        <>
          <Spinner /> {busyLabel}
        </>
      ) : justSaved ? (
        <>
          <Check className="animate-[ols-saved-pop_240ms_ease-out]" /> {savedLabel}
        </>
      ) : (
        children
      )}
    </Button>
  )
}