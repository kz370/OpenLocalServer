/**
 * The site whose "Open on my phone" section should scroll into view the next time it mounts.
 * Set by the Sites right-click "Share this site on my network", which opens the Domains tab;
 * read once by `PhoneSection`.
 */
let focusNext: string | null = null

export function focusPhoneSection(hostname: string) {
  focusNext = hostname
}

/** True once for `hostname` after `focusPhoneSection(hostname)`, then false again. */
export function takePhoneFocus(hostname: string): boolean {
  if (focusNext !== hostname) return false
  focusNext = null
  return true
}
