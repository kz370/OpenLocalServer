import { Loader2 } from 'lucide-react'

import { cn } from '@/lib/utils'

/** Shown in place of a Start/Stop icon until the change has actually happened. */
export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn('animate-spin', className)} />
}
