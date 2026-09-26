import { Check, Eye, EyeOff, Send, Sparkles, Square, TriangleAlert, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { Spinner } from '@/components/Spinner'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Textarea, Toggle } from '@/components/ui/form'
import { type AiAnswer, type AiJobView, type AiPrompt, type AiRequest, runCommand } from '@/core'
import { type AskAi, registerAiHost, useAiState } from '@/lib/ai'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

function money(usd: number): string {
  return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`
}

/** Renders `askAi(...)` requests. Mount once, in App. */
export function AiHost({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [ask, setAsk] = useState<AskAi | null>(null)
  const [question, setQuestion] = useState('')
  const [remoteOk, setRemoteOk] = useState(false)
  const [prompt, setPrompt] = useState<AiPrompt | null>(null)
  const [job, setJob] = useState<AiJobView | null>(null)
  const [picked, setPicked] = useState<number[]>([])
  const [results, setResults] = useState<{ label: string; ok: boolean; detail: string }[] | null>(null)
  const state = useAiState()
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    registerAiHost((next) => {
      setAsk(next)
      setQuestion(next.request.question ?? '')
      setRemoteOk(false)
      setPrompt(null)
      setJob(null)
      setResults(null)
      setPicked([])
      setError(null)
    })
    return () => registerAiHost(null)
  }, [setError])

  // Follow a running request.
  useEffect(() => {
    if (!job || job.state !== 'running') return
    const id = job.id
    const t = setInterval(() => {
      runCommand({ type: 'ai_job', job_id: id })
        .then((r) => {
          if (r.type === 'ai_job') {
            setJob(r.job)
            if (r.job.answer) setPicked(r.job.answer.actions.filter((a) => !a.destructive).map((_, i) => i))
          }
        })
        .catch(() => undefined)
    }, 400)
    return () => clearInterval(t)
  }, [job?.id, job?.state]) // eslint-disable-line react-hooks/exhaustive-deps

  const provider = ask && state ? state.settings.providers.find((p) => p.id === state.settings.features[ask.request.feature]) ?? state.settings.providers[0] : undefined
  const ready = !!state?.settings.enabled && !!provider
  const withQuestion = (): AiRequest => ({ ...ask!.request, question: question.trim() || ask!.request.question || null })
  const needsQuestion = ask?.question === 'required'
  const running = job?.state === 'running'
  const answer = job?.answer ?? null

  const close = useCallback(() => {
    if (job?.state === 'running') void runCommand({ type: 'ai_cancel', job_id: job.id }).catch(() => undefined)
    setAsk(null)
  }, [job])

  function preview() {
    if (prompt) return setPrompt(null)
    void run('preview', async () => {
      const r = await runCommand({ type: 'ai_preview', request: withQuestion() })
      if (r.type === 'ai_prompt') setPrompt(r.prompt)
    })
  }

  function start() {
    setPrompt(null)
    setResults(null)
    void run('ask', async () => {
      const r = await runCommand({ type: 'ai_start', request: withQuestion(), confirm_remote: remoteOk })
      if (r.type === 'ai_job') setJob(r.job)
    })
  }

  async function apply(a: AiAnswer) {
    const chosen = picked.map((i) => a.actions[i]).filter(Boolean)
    const risky = chosen.filter((c) => c.destructive)
    let confirmDestructive = false
    if (risky.length > 0) {
      confirmDestructive = await confirmAction(`These steps replace or remove something:\n${risky.map((c) => c.label).join('\n')}\nRun them too?`, 'Replaces or removes something')
    }
    const r = await runCommand({ type: 'ai_apply', actions: chosen.map((c) => c.command), confirm_destructive: confirmDestructive })
    if (r.type === 'ai_applied') setResults(r.steps)
  }

  return (
    <Dialog
      open={ask !== null}
      onClose={close}
      wide
      title={ask?.title ?? ''}
      description={ask?.description}
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            Close
          </Button>
          {ready && (
            <>
              <Button variant="secondary" disabled={busy !== null || running || (needsQuestion && !question.trim())} onClick={preview}>
                {busy === 'preview' ? <Spinner /> : prompt ? <EyeOff /> : <Eye />} {prompt ? 'Hide what is sent' : 'Show what will be sent'}
              </Button>
              {running ? (
                <Button variant="destructive" onClick={() => void runCommand({ type: 'ai_cancel', job_id: job!.id })}>
                  <Square /> Stop
                </Button>
              ) : (
                <Button disabled={busy !== null || (needsQuestion && !question.trim()) || (!provider!.local && !remoteOk)} onClick={start}>
                  {busy === 'ask' ? <Spinner /> : <Send />} {job ? 'Ask again' : 'Ask'}
                </Button>
              )}
            </>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />

        {!ready && (
          <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
            <p className="font-medium">The AI assistant is {state?.settings.providers.length ? 'turned off' : 'not set up'}.</p>
            <p className="text-muted-foreground">It is opt-in and bring-your-own-model: point it at LM Studio on this computer, or at Hugging Face, OpenRouter or another server. Nothing is sent until you ask.</p>
            <Button
              className="self-start"
              onClick={() => {
                setAsk(null)
                onNavigate('settings')
              }}
            >
              <Sparkles /> Open AI settings
            </Button>
          </div>
        )}

        {ready && provider && (
          <div className="flex flex-wrap items-center gap-2">
            {provider.local ? <Badge variant="success">Runs on this computer</Badge> : <Badge variant="warning">Sends data to {new URL(provider.base_url).host}</Badge>}
            <span className="text-muted-foreground">
              {provider.name} · {provider.model || 'no model chosen'}
            </span>
          </div>
        )}

        {ready && provider && !provider.local && (
          <Toggle checked={remoteOk} onChange={setRemoteOk} label={`Send this to ${new URL(provider.base_url).host}`} hint="It leaves this computer. Secrets are hidden first; use “Show what will be sent” to check." />
        )}

        {ready && ask?.question && ask.question !== 'none' && (
          <Textarea
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            rows={2}
            className="font-sans"
            placeholder={ask.placeholder ?? (needsQuestion ? 'Your question' : 'Anything to add? (optional)')}
          />
        )}

        {prompt && (
          <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
            <p className="text-xs text-muted-foreground">
              To {prompt.provider_name} ({prompt.host}), model {prompt.model}. {prompt.local ? 'On this computer.' : 'Outside this computer.'} Secrets are already hidden.
            </p>
            {prompt.attachments.length > 0 && <p className="text-xs">Attached: {prompt.attachments.join(' · ')}</p>}
            {prompt.tools.length > 0 && <p className="text-xs text-muted-foreground">The model may also read (only read): {prompt.tools.join(', ')}. Each result is hidden the same way.</p>}
            {prompt.messages.map((m, i) => (
              <div key={i}>
                <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{m.role}</div>
                <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-md bg-muted/50 p-2 font-mono text-xs">{m.content}</pre>
              </div>
            ))}
          </div>
        )}

        {job && (
          <div className="flex flex-col gap-2">
            {job.activity.length > 0 && (
              <ul className="text-xs text-muted-foreground">
                {job.activity.map((a, i) => (
                  <li key={i}>{a}</li>
                ))}
              </ul>
            )}
            {running && !job.partial && (
              <p className="flex items-center gap-2 text-muted-foreground">
                <Spinner /> Waiting for {job.provider}…
              </p>
            )}
            {(job.partial || answer) && <pre className="whitespace-pre-wrap font-sans leading-relaxed">{answer ? answer.text : job.partial}</pre>}
            {job.state === 'failed' && <ErrorCard error={{ problem: 'The assistant did not answer.', cause: job.error ?? '', fix: null }} />}
            {job.state === 'cancelled' && <p className="text-muted-foreground">Stopped.</p>}
          </div>
        )}

        {answer && (
          <div className="flex flex-col gap-3">
            <p className="text-xs text-muted-foreground">
              {answer.provider} · {answer.model}
              {answer.tokens_in != null && ` · ${answer.tokens_in} in, ${answer.tokens_out} out`}
              {answer.cost_usd != null && ` · ${money(answer.cost_usd)}`}
              {answer.local ? ' · stayed on this computer' : ' · sent off this computer'}
            </p>
            {answer.used.length > 0 && (
              <details className="text-xs text-muted-foreground">
                <summary className="cursor-pointer">What it read ({answer.used.length})</summary>
                <ul className="mt-1 list-disc pl-5">
                  {answer.used.map((u, i) => (
                    <li key={i}>{u}</li>
                  ))}
                </ul>
              </details>
            )}
            {answer.rejected.length > 0 && (
              <div className="rounded-lg border border-warning/40 bg-warning/5 p-3 text-xs">
                <p className="mb-1 flex items-center gap-1.5 font-medium text-warning">
                  <TriangleAlert className="size-3.5" /> Left out
                </p>
                <ul className="list-disc pl-5 text-muted-foreground">
                  {answer.rejected.map((r, i) => (
                    <li key={i}>{r}</li>
                  ))}
                </ul>
              </div>
            )}

            {answer.manifest && (
              <Draft title="Drafted environment.yaml" text={answer.manifest} action={ask?.onManifest ? { label: 'Use this draft', run: () => { ask.onManifest!(answer.manifest!); setAsk(null) } } : undefined} />
            )}
            {answer.script && (
              <Draft title="Drafted k6 script (passed the safety scan)" text={answer.script} action={ask?.onScript ? { label: 'Save as a script', run: () => { ask.onScript!(answer.script!); setAsk(null) } } : undefined} />
            )}
            {answer.commit_message && ask?.onCommit && (
              <Button className="self-start" onClick={() => { ask.onCommit!(answer.commit_message!); setAsk(null) }}>
                <Check /> Use this commit message
              </Button>
            )}

            {answer.actions.length > 0 && (
              <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
                <p className="font-medium">Proposed steps — nothing has run</p>
                {answer.actions.map((a, i) => (
                  <label key={i} className="flex cursor-pointer items-start gap-2">
                    <Checkbox
                      className="mt-0.5"
                      label={a.label}
                      checked={picked.includes(i)}
                      onChange={(on) => setPicked(on ? [...picked, i] : picked.filter((n) => n !== i))}
                    />
                    <span className="min-w-0">
                      {a.label} {a.destructive && <Badge variant="warning">replaces or removes something</Badge>}
                      <span className="block break-all font-mono text-xs text-muted-foreground">{JSON.stringify(a.command)}</span>
                    </span>
                  </label>
                ))}
                <Button className="self-start" disabled={busy !== null || picked.length === 0 || results !== null} onClick={() => run('apply', () => apply(answer))}>
                  {busy === 'apply' ? <Spinner /> : <Check />} Run {picked.length} step{picked.length === 1 ? '' : 's'}
                </Button>
                {results && (
                  <ul className="text-xs">
                    {results.map((r, i) => (
                      <li key={i} className={r.ok ? 'text-success' : 'text-destructive'}>
                        {r.ok ? <Check className="mr-1 inline size-3.5" /> : <X className="mr-1 inline size-3.5" />}
                        {r.label}: {r.detail}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </Dialog>
  )
}

function Draft({ title, text, action }: { title: string; text: string; action?: { label: string; run: () => void } }) {
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
      <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{title}</p>
      <pre className="max-h-72 overflow-auto whitespace-pre-wrap rounded-md bg-muted/50 p-2 font-mono text-xs">{text}</pre>
      {action && (
        <Button className="self-start" onClick={action.run}>
          <Check /> {action.label}
        </Button>
      )}
    </div>
  )
}
