/**
 * A phone with signal waves beside it ("shared with a phone on your network"). lucide's
 * Smartphone is a plain rounded rectangle that reads as a broken square at 12-14 px; the
 * waves and the screen make this one recognisable. Uses currentColor like a lucide icon.
 */
export function PhoneShareIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={className}>
      <rect x="3" y="5" width="9" height="16" rx="2" />
      <path d="M6.5 18h2" />
      <path d="M15.5 9.5a4 4 0 0 1 0 5" />
      <path d="M18.5 7a8 8 0 0 1 0 10" />
    </svg>
  )
}
