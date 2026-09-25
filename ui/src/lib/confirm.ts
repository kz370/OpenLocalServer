/**
 * In-app "are you sure?" prompts. `confirmAction` can be called from anywhere; the one
 * `<ConfirmHost />` mounted in App renders the request as the app's own dialog.
 */

export interface ConfirmRequest {
  message: string
  title: string
  resolve: (ok: boolean) => void
}

let show: ((req: ConfirmRequest) => void) | null = null

/** Called by `ConfirmHost` when it mounts. */
export function registerConfirmHost(fn: ((req: ConfirmRequest) => void) | null) {
  show = fn
}

export function confirmAction(message: string, title = 'Are you sure?'): Promise<boolean> {
  return new Promise((resolve) => {
    // No host mounted means no way to ask, which must never count as "yes".
    if (!show) return resolve(false)
    show({ message, title, resolve })
  })
}

/** For inline handlers: runs `fn` only after the user says yes. */
export function confirmThen(message: string, fn: () => unknown): void {
  void confirmAction(message).then((ok) => {
    if (ok) void fn()
  })
}
