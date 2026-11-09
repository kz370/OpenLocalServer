/**
 * The site whose "Open on my phone" section should scroll into view the next time it mounts.
 * Set by the Sites page ("Share this site on my network", the row's phone button, the
 * "On your network" link), which opens the Domains tab; read by `PhoneSection`.
 *
 * Reading does not clear it: React runs effects twice in development, and a request taken by
 * the first run would be lost. `PhoneSection` clears it once it has scrolled.
 */
let focusNext: string | null = null

export function focusPhoneSection(hostname: string) {
  focusNext = hostname
}

/** True while a scroll to `hostname`'s phone section is pending. */
export function wantsPhoneFocus(hostname: string): boolean {
  return focusNext === hostname
}

export function clearPhoneFocus(hostname: string) {
  if (focusNext === hostname) focusNext = null
}
