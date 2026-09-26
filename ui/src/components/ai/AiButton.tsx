import { Sparkles } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { type AskAi, askAi, useAiReady } from '@/lib/ai'

/** Opens the assistant for one thing. Hidden until the assistant is turned on in Settings. */
export function AiButton({
  ask,
  label = 'Explain',
  variant = 'ghost',
  title = 'Ask the AI assistant. You see exactly what is sent first.',
  disabled,
}: {
  ask: AskAi | (() => AskAi)
  label?: string
  variant?: 'ghost' | 'secondary' | 'outline' | 'default'
  title?: string
  disabled?: boolean
}) {
  const ready = useAiReady()
  if (!ready) return null
  return (
    <Button size="sm" variant={variant} title={title} disabled={disabled} onClick={() => askAi(typeof ask === 'function' ? ask() : ask)}>
      <Sparkles /> {label}
    </Button>
  )
}
