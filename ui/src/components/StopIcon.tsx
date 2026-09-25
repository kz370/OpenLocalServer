import { Square, type LucideProps } from 'lucide-react'

/** A solid square — the outline `Square` reads as an empty checkbox next to Play. */
export function StopIcon(props: LucideProps) {
  return <Square fill="currentColor" strokeWidth={0} {...props} />
}
