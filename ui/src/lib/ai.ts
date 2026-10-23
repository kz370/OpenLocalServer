/**
 * The AI assistant's front door. `useAiState` reads the settings so the chat page
 * and buttons follow when they change. Off by default; nothing is sent until asked.
 */
import { useEffect, useState } from 'react'

import { type AiState, runCommand } from '@/core'

let state: AiState | null = null
const listeners = new Set<() => void>()

/** Reads the settings again (after they change) so every consumer follows. */
export async function refreshAi(): Promise<AiState | null> {
  try {
    const r = await runCommand({ type: 'ai_get_state' })
    if (r.type === 'ai_state') setAiState(r.state)
  } catch {
    /* the assistant is optional; a failure here must never break a page */
  }
  return state
}

export function setAiState(next: AiState) {
  state = next
  listeners.forEach((l) => l())
}

export function useAiState(): AiState | null {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    if (!state) void refreshAi()
    return () => {
      listeners.delete(l)
    }
  }, [])
  return state
}

/** On, with at least one provider. */
export function useAiReady(): boolean {
  const s = useAiState()
  return !!s && s.settings.enabled && s.settings.providers.length > 0
}
