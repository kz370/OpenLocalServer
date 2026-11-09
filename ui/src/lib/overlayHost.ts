import { useEffect, useState } from 'react'

/**
 * Portal targets mounted once in the app root (see `OverlayHosts`).
 *
 * `position: fixed` is laid out against the nearest ancestor with a backdrop-filter or
 * transform, and every Card has backdrop-blur, so a dialog or notification rendered inside a
 * card would be placed relative to that card instead of the window. Overlays portal here.
 * The hosts sit inside the app root (not <body>) so they keep its rounded window corners.
 */
export const OVERLAY_ROOT_ID = 'ols-overlay-root'
/** Bottom-right stack slot for overlay ErrorCards, shown above the toasts. */
export const CORNER_ERRORS_ID = 'ols-corner-errors'

/** The host element with `id`, or `document.body` when it is not mounted (tests, early boot). */
export function useOverlayHost(id: string): HTMLElement | null {
  const [host, setHost] = useState<HTMLElement | null>(() =>
    typeof document === 'undefined' ? null : document.getElementById(id),
  )
  useEffect(() => {
    if (!host || !host.isConnected) setHost(document.getElementById(id) ?? document.body)
  }, [host, id])
  return host
}
