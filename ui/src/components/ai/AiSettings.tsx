import { CircleCheck, CircleX, Pencil, Plus, Radar, Sparkles, Trash2 } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type AiDetected, type AiKind, type AiModel, type AiProvider, type AiState, type CoreResponse, runCommand } from '@/core'
import { setAiState, useAiState } from '@/lib/ai'
import { confirmThen } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

const PRESETS: Record<AiKind, { label: string; name: string; url: string; key: boolean; hint: string }> = {
  lmstudio: { label: 'LM Studio', name: 'LM Studio', url: 'http://localhost:1234/v1', key: false, hint: 'Runs on this computer. Start its local server (Developer tab) and load a model.' },
  ollama: { label: 'Ollama', name: 'Ollama', url: 'http://localhost:11434/v1', key: false, hint: 'Runs on this computer. Pull a model with `ollama pull`.' },
  huggingface: {
    label: 'Hugging Face',
    name: 'Hugging Face',
    url: 'https://router.huggingface.co/v1',
    key: true,
    hint: 'Uses an access token from huggingface.co/settings/tokens. For your own Inference Endpoint or Text Generation Inference server, replace the address with its /v1 URL.',
  },
  openrouter: { label: 'OpenRouter', name: 'OpenRouter', url: 'https://openrouter.ai/api/v1', key: true, hint: 'Uses an API key from openrouter.ai/keys. Prices are shown with each answer.' },
  custom: { label: 'Another server', name: '', url: '', key: false, hint: 'Any server that speaks the OpenAI chat API: llama.cpp, vLLM, LocalAI, a company gateway.' },
}

const EMPTY: AiProvider = { id: '', name: '', kind: 'custom', base_url: '', model: '', tools: true, local: false, has_key: false }

function priceText(m?: AiModel): string {
  if (!m || m.prompt_per_m == null || m.completion_per_m == null) return ''
  return `$${m.prompt_per_m.toFixed(2)} in / $${m.completion_per_m.toFixed(2)} out per million tokens`
}

/** Settings → AI assistant: on/off, providers, per-feature choice. Off by default; nothing is sent until asked. */
export function AiCard() {
  const state = useAiState()
  const [editing, setEditing] = useState<AiProvider | null>(null)
  const [detected, setDetected] = useState<AiDetected[] | null>(null)
  const [tests, setTests] = useState<Record<string, { ok: boolean; message: string }>>({})
  const { busy, error, setError, run } = useAction()

  if (!state) return null
  const { settings, features } = state

  const save = async (enabled: boolean, routes: Record<string, string>) => {
    const r = await runCommand({ type: 'ai_save_settings', enabled, features: routes })
    if (r.type === 'ai_state') setAiState(r.state)
  }
  const apply = (r: CoreResponse) => {
    if (r.type === 'ai_state') setAiState(r.state)
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <Sparkles className="size-4" /> AI assistant
        </CardTitle>
        <CardDescription>
          Explains problems, drafts configs and answers questions about your logs. It ships no model and needs no account: you point it at LM Studio on this computer, or at Hugging Face, OpenRouter or another
          server. Nothing is sent unless you press a button, secrets are hidden first, and every change it proposes waits for your approval.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Toggle
          checked={settings.enabled}
          onChange={(v) => run('enable', () => save(v, settings.features))}
          label="Turn the AI assistant on"
          hint="Off by default. While it is off no AI buttons appear and nothing is sent anywhere."
        />

        <div className="flex flex-col gap-2">
          {settings.providers.length === 0 && <p className="text-sm text-muted-foreground">No provider yet. Add one below.</p>}
          {settings.providers.map((p) => {
            const t = tests[p.id]
            return (
              <div key={p.id} className="flex flex-wrap items-center gap-2 rounded-lg border border-border p-3 text-sm">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2 font-medium">
                    {p.name}
                    {p.local ? <Badge variant="success">On this computer</Badge> : <Badge variant="warning">Sends data to {new URL(p.base_url).host}</Badge>}
                    {!p.local && !p.has_key && <Badge variant="outline">no key</Badge>}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">
                    {p.base_url} · {p.model || 'no model chosen'}
                  </div>
                  {t && (
                    <div className={`mt-1 flex items-start gap-1 text-xs ${t.ok ? 'text-success' : 'text-destructive'}`}>
                      {t.ok ? <CircleCheck className="mt-0.5 size-3.5 shrink-0" /> : <CircleX className="mt-0.5 size-3.5 shrink-0" />} {t.message}
                    </div>
                  )}
                </div>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busy !== null}
                  onClick={() =>
                    run(`test:${p.id}`, async () => {
                      const r = await runCommand({ type: 'ai_test', provider_id: p.id })
                      if (r.type === 'ai_test') setTests((cur) => ({ ...cur, [p.id]: { ok: r.result.ok, message: r.result.message } }))
                    })
                  }
                >
                  {busy === `test:${p.id}` ? <Spinner /> : null} Test connection
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setEditing(p)}>
                  <Pencil /> Edit
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  title="Remove this provider and its key"
                  onClick={() => confirmThen(`Remove ${p.name}?\nIts stored key is deleted too.`, () => run('remove', async () => apply(await runCommand({ type: 'ai_remove_provider', id: p.id }))))}
                >
                  <Trash2 />
                </Button>
              </div>
            )
          })}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs font-medium text-muted-foreground">Add:</span>
          {(Object.keys(PRESETS) as AiKind[]).map((k) => (
            <Button key={k} size="sm" variant="outline" onClick={() => setEditing({ ...EMPTY, kind: k, name: PRESETS[k].name, base_url: PRESETS[k].url, local: k === 'lmstudio' || k === 'ollama' })}>
              <Plus /> {PRESETS[k].label}
            </Button>
          ))}
          <Button
            size="sm"
            variant="ghost"
            disabled={busy !== null}
            onClick={() =>
              run('detect', async () => {
                const r = await runCommand({ type: 'ai_detect_local' })
                if (r.type === 'ai_detected') setDetected(r.servers)
              })
            }
          >
            {busy === 'detect' ? <Spinner /> : <Radar />} Look on this computer
          </Button>
        </div>
        {detected && (
          <div className="rounded-lg border border-border p-3 text-sm">
            {detected.length === 0 && <p className="text-muted-foreground">No LM Studio or Ollama server answered on this computer. Start its local server and look again.</p>}
            {detected.map((d) => (
              <div key={d.base_url} className="flex flex-wrap items-center gap-2">
                <span className="font-medium">{d.name}</span>
                <span className="text-xs text-muted-foreground">
                  is running · {d.models.length} model{d.models.length === 1 ? '' : 's'}
                </span>
                <Button size="sm" variant="secondary" disabled={settings.providers.some((p) => p.base_url === d.base_url)} onClick={() => setEditing({ ...EMPTY, kind: d.kind, name: d.name, base_url: d.base_url, model: d.models[0] ?? '', local: true })}>
                  {settings.providers.some((p) => p.base_url === d.base_url) ? 'Already added' : 'Add'}
                </Button>
              </div>
            ))}
          </div>
        )}

        {settings.providers.length > 0 && (
          <div className="flex flex-col gap-2">
            <div className="text-xs font-medium text-muted-foreground">Which provider each feature uses</div>
            <p className="text-xs text-muted-foreground">For example a local model for logs and a larger one for config help. The first provider is used unless you choose another.</p>
            <div className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-2 sm:max-w-xl">
              {features.map((f) => (
                <div key={f.id} className="contents">
                  <span className="text-sm">{f.label}</span>
                  <Select
                    className="w-56"
                    value={settings.features[f.id] ?? ''}
                    disabled={busy !== null}
                    onChange={(e) => run('route', () => save(settings.enabled, { ...settings.features, [f.id]: e.target.value }))}
                  >
                    <option value="">{settings.providers[0].name} (default)</option>
                    {settings.providers.slice(1).map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </Select>
                </div>
              ))}
            </div>
          </div>
        )}
      </CardContent>

      {editing && (
        <ProviderDialog
          key={editing.id || editing.name + editing.base_url}
          initial={editing}
          onClose={() => setEditing(null)}
          onSaved={(s) => {
            setAiState(s)
          }}
        />
      )}
    </Card>
  )
}

function ProviderDialog({ initial, onClose, onSaved }: { initial: AiProvider; onClose: () => void; onSaved: (s: AiState) => void }) {
  const [p, setP] = useState<AiProvider>(initial)
  const [apiKey, setApiKey] = useState('')
  const [removeKey, setRemoveKey] = useState(false)
  const [models, setModels] = useState<AiModel[]>([])
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null)
  const { busy, error, setError, run } = useAction()
  const preset = PRESETS[p.kind]
  const isNew = !initial.id

  /** Saves, and returns the provider as stored (a new one gets its id from the core). */
  async function save(): Promise<AiProvider> {
    const r = await runCommand({ type: 'ai_save_provider', provider: p, api_key: removeKey ? '' : apiKey ? apiKey : null })
    if (r.type !== 'ai_state') throw new Error('unexpected reply')
    onSaved(r.state)
    const saved = p.id ? r.state.settings.providers.find((x) => x.id === p.id) : [...r.state.settings.providers].reverse().find((x) => x.name === p.name.trim())
    if (!saved) throw new Error('the provider was not saved')
    setP(saved)
    setApiKey('')
    setRemoveKey(false)
    return saved
  }

  async function saveAndTest() {
    const saved = await save()
    const r = await runCommand({ type: 'ai_test', provider_id: saved.id })
    if (r.type === 'ai_test') {
      setModels(r.result.models)
      setMessage({ ok: r.result.ok, text: r.result.message })
      if (r.result.ok && !saved.model && r.result.models.length > 0) setP({ ...saved, model: r.result.models[0].id })
    }
  }

  const chosen = models.find((m) => m.id === p.model)
  return (
    <Dialog
      open
      onClose={() => busy === null && onClose()}
      title={isNew ? `Add ${preset.label}` : `Edit ${initial.name}`}
      description={preset.hint}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="secondary" disabled={busy !== null || !p.name.trim() || !p.base_url.trim()} onClick={() => run('test', saveAndTest)}>
            {busy === 'test' ? <Spinner /> : null} Save and test
          </Button>
          <Button
            disabled={busy !== null || !p.name.trim() || !p.base_url.trim()}
            onClick={() =>
              run('save', async () => {
                await save()
                onClose()
              })
            }
          >
            {busy === 'save' ? <Spinner /> : null} Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="Name">
          <Input value={p.name} onChange={(e) => setP({ ...p, name: e.target.value })} />
        </Field>
        <Field label="Address" hint="The server's OpenAI-style base URL, ending in /v1.">
          <Input value={p.base_url} onChange={(e) => setP({ ...p, base_url: e.target.value })} placeholder="http://localhost:1234/v1" />
        </Field>
        <Field
          label={preset.key ? 'API key' : 'API key (only if the server wants one)'}
          hint={p.has_key && !removeKey ? 'A key is stored in the Windows credential store. Leave this empty to keep it.' : 'Stored in the Windows credential store, never in a file.'}
        >
          <div className="flex gap-2">
            <Input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder={p.has_key && !removeKey ? '••••••••' : ''} autoComplete="off" />
            {p.has_key && (
              <Button variant="ghost" onClick={() => setRemoveKey(!removeKey)}>
                {removeKey ? 'Keep key' : 'Remove key'}
              </Button>
            )}
          </div>
        </Field>
        <Field label="Model" hint={chosen ? priceText(chosen) : models.length ? `${models.length} models available. Pick one or type its name.` : '“Save and test” lists the models this server has.'}>
          <Input value={p.model} onChange={(e) => setP({ ...p, model: e.target.value })} list="ai-models" placeholder="model name" />
          <datalist id="ai-models">
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
          </datalist>
        </Field>
        <Toggle checked={p.tools} onChange={(v) => setP({ ...p, tools: v })} label="Let the model read through tools" hint="It can look up logs, configs and findings itself (read-only). Turn off for small local models that can't use tools." />
        {message && (
          <div className={`flex items-start gap-1.5 text-sm ${message.ok ? 'text-success' : 'text-destructive'}`}>
            {message.ok ? <CircleCheck className="mt-0.5 size-4 shrink-0" /> : <CircleX className="mt-0.5 size-4 shrink-0" />} {message.text}
          </div>
        )}
      </div>
    </Dialog>
  )
}
