import { AlertTriangle } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type ConfirmRequest, registerConfirmHost } from '@/lib/confirm'

/** Renders `confirmAction(...)` requests as an in-app dialog. Mount once, in App. */
export function ConfirmHost() {
  const [req, setReq] = useState<ConfirmRequest | null>(null)
  const yes = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    registerConfirmHost((next) =>
      setReq((cur) => {
        // One question at a time: a second request while one is open is declined.
        if (cur) {
          next.resolve(false)
          return cur
        }
        return next
      }),
    )
    return () => registerConfirmHost(null)
  }, [])

  useEffect(() => {
    if (req) yes.current?.focus()
  }, [req])

  const answer = (ok: boolean) => {
    req?.resolve(ok)
    setReq(null)
  }

  // The dialog keeps rendering for its exit animation after `req` is cleared; show the last
  // request meanwhile, or the closing dialog flashes an empty title and a "Confirm" button.
  const [shown, setShown] = useState<ConfirmRequest | null>(null)
  if (req && req !== shown) setShown(req)

  const [first, ...rest] = (shown?.message ?? '').split('\n')
  return (
    <Dialog
      open={req !== null}
      onClose={() => answer(false)}
      title={shown?.title ?? ''}
      layer="confirm"
      footer={
        <>
          <Button variant="ghost" onClick={() => answer(false)}>
            Cancel
          </Button>
          <Button ref={yes} variant={shown?.confirmTone === 'amber' ? 'outline' : 'destructive'} className={shown?.confirmTone === 'amber' ? 'border-amber-500/50 text-amber-600 hover:bg-amber-500/10 dark:text-amber-400' : undefined} onClick={() => answer(true)}>
            {shown?.confirmLabel ?? 'Confirm'}
          </Button>
        </>
      }
    >
      <div className="flex gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-destructive/10 text-destructive">
          <AlertTriangle className="size-4" />
        </span>
        <div className="flex flex-col gap-1 pt-1.5 text-sm">
          <p className="font-medium">{first}</p>
          {rest.filter(Boolean).map((line, i) => (
            <p key={i} className="text-muted-foreground">
              {line}
            </p>
          ))}
        </div>
      </div>
    </Dialog>
  )
}
