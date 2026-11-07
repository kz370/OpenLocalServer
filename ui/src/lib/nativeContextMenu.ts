/**
 * The webview's own right-click menu (Back, Reload, Inspect, ...) is a browser menu, not
 * part of the app. It is turned off everywhere except where it is the useful one: inside
 * text fields and editors, where it holds Cut / Copy / Paste. The app's own menus (the
 * Sites and Projects rows) open from their own `onContextMenu` and are not affected.
 */

/** Something that can answer `closest(selector)`, as a DOM element can. */
interface ClosestCapable {
  closest?: (selector: string) => unknown
}

const EDITABLE =
  'input:not([type="checkbox"]):not([type="radio"]):not([type="button"]):not([type="submit"]):not([type="range"]):not([type="color"]), textarea, [contenteditable=""], [contenteditable="true"], .cm-editor'

/** Whether a right-click on `target` keeps the webview's own menu (text editing). */
export function keepsNativeMenu(target: unknown): boolean {
  const el = target as ClosestCapable | null
  return typeof el?.closest === 'function' && el.closest(EDITABLE) != null
}

/** Installs the listener once, in every build (DevTools stay on F12 / Ctrl+Shift+I in dev). */
export function disableNativeContextMenu(): void {
  window.addEventListener('contextmenu', (e) => {
    if (!keepsNativeMenu(e.target)) e.preventDefault()
  })
}
