import type { CoreCommand } from '../commands'

export type AiKind = 'lmstudio' | 'ollama' | 'huggingface' | 'openrouter' | 'custom'

export interface AiProvider {
  id: string
  name: string
  kind: AiKind
  base_url: string
  model: string
  tools: boolean
  /** Runs on this computer. Computed by the core. */
  local: boolean
  /** A key is stored for it. Computed by the core. */
  has_key: boolean
}

export interface AiState {
  settings: { enabled: boolean; providers: AiProvider[]; features: Record<string, string> }
  features: { id: string; label: string }[]
}

export interface AiModel {
  id: string
  name: string
  context: number | null
  prompt_per_m: number | null
  completion_per_m: number | null
}

export interface AiTestResult {
  ok: boolean
  message: string
  ms: number
  models: AiModel[]
}

export interface AiDetected {
  kind: AiKind
  name: string
  base_url: string
  models: string[]
}

export interface AiRequest {
  feature: 'assistant' | 'explain' | 'config' | 'logs' | 'traffic' | 'commit' | 'palette' | 'app_guide'
  kind?: string
  project_id?: string | null
  question?: string | null
  title?: string | null
  text?: string | null
  tunnel_id?: string | null
  request_ids?: number[]
  log_sources?: string[]
  messages?: { role: string; content: string }[]
}

export interface AiPrompt {
  provider_id: string
  provider_name: string
  model: string
  local: boolean
  host: string
  messages: { role: string; content: string }[]
  attachments: string[]
  tools: string[]
}

export interface AiAction {
  label: string
  command: CoreCommand
  destructive: boolean
}

export interface AiAnswer {
  text: string
  actions: AiAction[]
  rejected: string[]
  manifest: string | null
  file: string | null
  script: string | null
  commit_message: string | null
  provider: string
  model: string
  local: boolean
  tokens_in: number | null
  tokens_out: number | null
  cost_usd: number | null
  used: string[]
}

export interface AiJobView {
  id: string
  feature: string
  state: 'running' | 'done' | 'failed' | 'cancelled'
  partial: string
  activity: string[]
  answer: AiAnswer | null
  error: string | null
  provider: string
  local: boolean
}

// @@ts-types
