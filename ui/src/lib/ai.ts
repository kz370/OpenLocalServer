/**
 * The AI assistant's front door (Stage 19). `askAi` can be called from any button; the one `<AiHost />` mounted in App
 * renders the request as a dialog (the same idea as `confirmAction`). Buttons only appear while the assistant is turned on
 * and a provider exists, so it stays out of the way until the user opts in.
 */
import { useEffect, useState } from 'react'

import { type AiRequest, type AiState, runCommand } from '@/core'

export interface AskAi {
  request: AiRequest
  title: string
  description?: string
  /** A question box: `required` when the request is nothing without it (logs, palette), `optional` to refine it. */
  question?: 'none' | 'optional' | 'required'
  placeholder?: string
  /** Offered on the answer when the model drafted one that passed the core's checks. */
  onManifest?: (yaml: string) => void
  onFile?: (content: string) => void
  onScript?: (js: string) => void
  onCommit?: (message: string) => void
}

let show: ((ask: AskAi) => void) | null = null

/** Called by `AiHost` when it mounts. */
export function registerAiHost(fn: ((ask: AskAi) => void) | null) {
  show = fn
}

export function askAi(ask: AskAi) {
  show?.(ask)
}

let state: AiState | null = null
const listeners = new Set<() => void>()

/** Reads the settings again (after they change) so every button follows. */
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
