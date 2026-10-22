import {
  BookOpen,
  Bot,
  Check,
  Compass,
  Copy,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  HelpCircle,
  Info,
  Layers,
  MessageSquare,
  Printer,
  RotateCcw,
  Search,
  Send,
  Sparkles,
  Square,
  Wrench,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Textarea, Toggle } from '@/components/ui/form'
import { type AiAnswer, type AiJobView, type AiPrompt, type CoreCommand, runCommand } from '@/core'
import { useAiState } from '@/lib/ai'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

const QUICK_QUESTIONS = [
  'How do I create a WordPress or Laravel site?',
  'How do custom domains and SSL certificates work?',
  'How do I make my site public with Tunnels?',
  'How do I switch PHP versions or toggle extensions?',
  'How do I backup and export my sites with database dumps?',
  'What is new in OpenLocalServer v2.3.0?',
]

interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  answer?: AiAnswer | null
  timestamp: number
}

export function AskAiPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [activeTab, setActiveTab] = useState<'chat' | 'manual' | 'version'>('chat')
  const [question, setQuestion] = useState('')
  const [remoteOk, setRemoteOk] = useState(false)
  const [promptPreview, setPromptPreview] = useState<AiPrompt | null>(null)
  const [currentJob, setCurrentJob] = useState<AiJobView | null>(null)
  const [pickedActions, setPickedActions] = useState<number[]>([])
  const [actionResults, setActionResults] = useState<{ label: string; ok: boolean; detail: string }[] | null>(null)
  const [manualMarkdown, setManualMarkdown] = useState<string>('')
  const [manualSearch, setManualSearch] = useState('')
  const [copiedManual, setCopiedManual] = useState(false)
  const [messages, setMessages] = useState<ChatMessage[]>(() => {
    try {
      const stored = sessionStorage.getItem('ols:askai-history')
      if (stored) return JSON.parse(stored)
    } catch {
      // ignore
    }
    return [
      {
        id: 'welcome',
        role: 'assistant',
        content:
          "👋 Hello! I am your OpenLocalServer Assistant.\n\nI have complete knowledge of OpenLocalServer v2.3.0, including site configuration, web servers (Nginx/Apache/Caddy), PHP/Node/Python runtimes, databases, public tunnels, encrypted site bundles, and the new Dynamic DNS & broadcast controls.\n\nAsk me anything about how to use the app, or pick one of the quick starter questions below!",
        timestamp: Date.now(),
      },
    ]
  })

  const aiState = useAiState()
  const { busy, error, setError, run } = useAction()
  const chatBottomRef = useRef<HTMLDivElement>(null)

  // Persist conversation history in session storage
  useEffect(() => {
    try {
      sessionStorage.setItem('ols:askai-history', JSON.stringify(messages))
    } catch {
      // ignore
    }
  }, [messages])

  // Scroll to bottom when new messages arrive
  useEffect(() => {
    if (activeTab === 'chat') {
      chatBottomRef.current?.scrollIntoView({ behavior: 'smooth' })
    }
  }, [messages, currentJob?.partial, activeTab])

  // Fetch full user manual from backend
  useEffect(() => {
    runCommand({ type: 'get_app_guide', version: '2.3.0' })
      .then((r) => {
        if (r.type === 'app_guide') {
          setManualMarkdown(r.markdown)
        }
      })
      .catch(() => undefined)
  }, [])

  // Follow running AI job
  useEffect(() => {
    if (!currentJob || currentJob.state !== 'running') return
    const id = currentJob.id
    const timer = setInterval(() => {
      runCommand({ type: 'ai_job', job_id: id })
        .then((r) => {
          if (r.type === 'ai_job') {
            setCurrentJob(r.job)
            if (r.job.state === 'done' && r.job.answer) {
              setMessages((prev) => [
                ...prev,
                {
                  id: r.job.id,
                  role: 'assistant',
                  content: r.job.answer?.text || r.job.partial,
                  answer: r.job.answer,
                  timestamp: Date.now(),
                },
              ])
              if (r.job.answer.actions.length > 0) {
                setPickedActions(r.job.answer.actions.filter((a) => !a.destructive).map((_, i) => i))
              }
              setCurrentJob(null)
            } else if (r.job.state === 'failed') {
              setError({ problem: 'The assistant could not complete the answer.', cause: r.job.error ?? 'Unknown error', fix: 'Check your AI provider in Settings → AI assistant.' })
              setCurrentJob(null)
            }
          }
        })
        .catch(() => undefined)
    }, 400)
    return () => clearInterval(timer)
  }, [currentJob?.id, currentJob?.state, setError]) // eslint-disable-line react-hooks/exhaustive-deps

  const provider = useMemo(() => {
    if (!aiState?.settings) return undefined
    const featureId = aiState.settings.features['app_guide']
    return aiState.settings.providers.find((p) => p.id === featureId) ?? aiState.settings.providers[0]
  }, [aiState])

  const isAiReady = !!aiState?.settings.enabled && !!provider && !!provider.model

  const askQuestion = (qText: string) => {
    const trimmed = qText.trim()
    if (!trimmed) return
    setError(null)
    setPromptPreview(null)
    setActionResults(null)

    // Add user message to chat immediately
    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: trimmed,
      timestamp: Date.now(),
    }
    setMessages((prev) => [...prev, userMsg])
    setQuestion('')

    void run('ask', async () => {
      const r = await runCommand({
        type: 'ai_start',
        request: {
          feature: 'app_guide',
          question: trimmed,
        },
        confirm_remote: remoteOk,
      })
      if (r.type === 'ai_job') {
        setCurrentJob(r.job)
      }
    })
  }

  const handlePreviewPrompt = () => {
    if (promptPreview) {
      setPromptPreview(null)
      return
    }
    const trimmed = question.trim() || 'How do I use OpenLocalServer?'
    void run('preview', async () => {
      const r = await runCommand({
        type: 'ai_preview',
        request: {
          feature: 'app_guide',
          question: trimmed,
        },
      })
      if (r.type === 'ai_prompt') {
        setPromptPreview(r.prompt)
      }
    })
  }

  const handleCancelJob = () => {
    if (currentJob?.id) {
      void runCommand({ type: 'ai_cancel', job_id: currentJob.id }).catch(() => undefined)
      setCurrentJob(null)
    }
  }

  const handleClearHistory = () => {
    setMessages([
      {
        id: 'welcome-reset',
        role: 'assistant',
        content:
          "Conversation history cleared.\n\nAsk me anything about how to use OpenLocalServer, configure sites, databases, tunnels, or runtimes!",
        timestamp: Date.now(),
      },
    ])
    sessionStorage.removeItem('ols:askai-history')
  }

  const handleApplyActions = async (answer: AiAnswer) => {
    const chosen = pickedActions.map((i) => answer.actions[i]).filter(Boolean)
    const risky = chosen.filter((c) => c.destructive)
    let confirmDestructive = false
    if (risky.length > 0) {
      confirmDestructive = await confirmAction(
        `These steps modify existing files or services:\n${risky.map((c) => c.label).join('\n')}\nRun them now?`,
        'Approval needed',
      )
    }
    const r = await runCommand({
      type: 'ai_apply',
      actions: chosen.map((c) => c.command as CoreCommand),
      confirm_destructive: confirmDestructive,
    })
    if (r.type === 'ai_applied') {
      setActionResults(r.steps)
    }
  }

  const handleCopyManual = () => {
    if (!manualMarkdown) return
    void navigator.clipboard.writeText(manualMarkdown)
    setCopiedManual(true)
    setTimeout(() => setCopiedManual(false), 2000)
  }

  const handleDownloadManual = () => {
    if (!manualMarkdown) return
    const blob = new Blob([manualMarkdown], { type: 'text/markdown;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'OpenLocalServer_User_Manual_v2.3.0.md'
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  const handlePrintManual = () => {
    window.print()
  }

  // Filtered manual sections
  const filteredManualSections = useMemo(() => {
    if (!manualMarkdown) return []
    const lines = manualMarkdown.split('\n')
    const sections: { title: string; content: string }[] = []
    let currentTitle = 'Overview'
    let currentBody: string[] = []

    for (const line of lines) {
      if (line.startsWith('## ')) {
        if (currentBody.length > 0) {
          sections.push({ title: currentTitle, content: currentBody.join('\n') })
        }
        currentTitle = line.replace('## ', '').trim()
        currentBody = []
      } else {
        currentBody.push(line)
      }
    }
    if (currentBody.length > 0) {
      sections.push({ title: currentTitle, content: currentBody.join('\n') })
    }

    if (!manualSearch.trim()) return sections
    const query = manualSearch.toLowerCase()
    return sections.filter(
      (s) => s.title.toLowerCase().includes(query) || s.content.toLowerCase().includes(query),
    )
  }, [manualMarkdown, manualSearch])

  return (
    <div className="flex flex-col gap-6 max-w-6xl mx-auto pb-12">
      {/* Top Header */}
      <div className="flex flex-wrap items-center justify-between gap-4 border-b border-border pb-4">
        <div>
          <div className="flex items-center gap-2.5">
            <div className="flex size-9 items-center justify-center rounded-lg bg-teal-500/10 text-teal-600 dark:text-teal-400">
              <Sparkles className="size-5" />
            </div>
            <div>
              <h1 className="text-xl font-bold tracking-tight">Ask AI & Operations Guide</h1>
              <p className="text-xs text-muted-foreground">
                Intelligent assistant and complete operations manual for OpenLocalServer v2.3.0
              </p>
            </div>
          </div>
        </div>

        {/* AI Status / Provider Info */}
        <div className="flex items-center gap-2">
          {isAiReady && provider ? (
            <div className="flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-1.5 text-xs shadow-xs">
              {provider.local ? (
                <Badge variant="success" className="text-[10px]">
                  Local AI ({provider.name})
                </Badge>
              ) : (
                <Badge variant="warning" className="text-[10px]">
                  Cloud AI ({provider.name})
                </Badge>
              )}
              <span className="text-muted-foreground font-mono truncate max-w-44">
                {provider.model || 'model active'}
              </span>
              <Badge variant="secondary" className="text-[10px]">
                v2.3.0 RAG
              </Badge>
            </div>
          ) : (
            <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-400">
              <Info className="size-3.5 shrink-0" />
              <span>AI offline · Manual guide active</span>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-xs text-amber-700 hover:text-amber-800 dark:text-amber-400 dark:hover:text-amber-300"
                onClick={() => onNavigate('settings')}
              >
                Configure
              </Button>
            </div>
          )}
        </div>
      </div>

      {/* Tabs Switcher */}
      <div className="flex items-center gap-2 border-b border-border pb-2">
        <button
          onClick={() => setActiveTab('chat')}
          className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
            activeTab === 'chat'
              ? 'bg-primary text-primary-foreground shadow-xs'
              : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
          }`}
        >
          <MessageSquare className="size-4" />
          Ask Assistant
        </button>
        <button
          onClick={() => setActiveTab('manual')}
          className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
            activeTab === 'manual'
              ? 'bg-primary text-primary-foreground shadow-xs'
              : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
          }`}
        >
          <BookOpen className="size-4" />
          App User Manual (MD/PDF)
        </button>
        <button
          onClick={() => setActiveTab('version')}
          className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
            activeTab === 'version'
              ? 'bg-primary text-primary-foreground shadow-xs'
              : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
          }`}
        >
          <Layers className="size-4" />
          Version 2.3.0 Notes (RAG)
        </button>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {/* TAB 1: ASK AI (CHAT) */}
      {activeTab === 'chat' && (
        <div className="flex flex-col gap-5">
          {!isAiReady && (
            <Card className="border-border bg-muted/20">
              <CardHeader className="pb-3">
                <CardTitle className="text-sm flex items-center gap-2">
                  <Bot className="size-4 text-teal-600 dark:text-teal-400" />
                  Bring-Your-Own-Model AI Assistant is not set up yet
                </CardTitle>
                <CardDescription className="text-xs">
                  OLS is strictly privacy-first: it ships no proprietary models and collects zero telemetry. Point it at{' '}
                  <strong>LM Studio</strong> or <strong>Ollama</strong> on this computer for 100% free, offline AI, or connect{' '}
                  <strong>OpenRouter</strong> / <strong>Hugging Face</strong> with your key.
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-wrap items-center gap-3">
                <Button size="sm" onClick={() => onNavigate('settings')}>
                  <Sparkles className="size-3.5" />
                  Open AI Settings
                </Button>
                <Button variant="secondary" size="sm" onClick={() => setActiveTab('manual')}>
                  <BookOpen className="size-3.5" />
                  Browse Built-in Manual Instead
                </Button>
              </CardContent>
            </Card>
          )}

          {/* Quick Starter Pills */}
          <div className="flex flex-col gap-2">
            <span className="text-xs font-medium text-muted-foreground">Common questions you can ask:</span>
            <div className="flex flex-wrap gap-2">
              {QUICK_QUESTIONS.map((q) => (
                <button
                  key={q}
                  disabled={busy !== null || !isAiReady}
                  onClick={() => askQuestion(q)}
                  className="rounded-full border border-border bg-card px-3 py-1 text-xs text-foreground/80 transition-colors hover:border-teal-500 hover:bg-teal-500/5 hover:text-teal-600 dark:hover:text-teal-400 disabled:opacity-50"
                >
                  {q}
                </button>
              ))}
            </div>
          </div>

          {/* Chat Messages Feed */}
          <div className="flex flex-col gap-4 rounded-xl border border-border bg-card/60 p-4 min-h-[340px] max-h-[520px] overflow-y-auto">
            {messages.map((m) => (
              <div
                key={m.id}
                className={`flex gap-3 text-sm ${
                  m.role === 'user' ? 'justify-end' : 'justify-start'
                }`}
              >
                {m.role === 'assistant' && (
                  <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-teal-500/15 text-teal-600 dark:text-teal-400">
                    <Bot className="size-4" />
                  </div>
                )}
                <div
                  className={`flex flex-col gap-1.5 max-w-[85%] rounded-lg px-4 py-3 leading-relaxed ${
                    m.role === 'user'
                      ? 'bg-primary text-primary-foreground rounded-br-none'
                      : 'border border-border bg-muted/40 text-foreground rounded-bl-none'
                  }`}
                >
                  <div className="whitespace-pre-wrap font-sans text-xs sm:text-sm">
                    {m.content}
                  </div>

                  {/* Actions attached to answer */}
                  {m.answer && m.answer.actions.length > 0 && (
                    <div className="mt-3 flex flex-col gap-2 rounded-md border border-border bg-card/80 p-3 text-xs">
                      <p className="font-semibold text-foreground">Suggested executable steps (reviewed by safety gate):</p>
                      {m.answer.actions.map((act, idx) => (
                        <label key={idx} className="flex cursor-pointer items-start gap-2 text-muted-foreground">
                          <Checkbox
                            className="mt-0.5"
                            checked={pickedActions.includes(idx)}
                            onChange={(checked) =>
                              setPickedActions((prev) =>
                                checked ? [...prev, idx] : prev.filter((i) => i !== idx),
                              )
                            }
                          />
                          <span>
                            <strong className="text-foreground">{act.label}</strong>
                            {act.destructive && (
                              <Badge variant="warning" className="ml-1.5 text-[10px]">
                                replaces something
                              </Badge>
                            )}
                          </span>
                        </label>
                      ))}
                      <Button
                        size="sm"
                        className="self-start mt-1"
                        disabled={busy !== null || pickedActions.length === 0 || actionResults !== null}
                        onClick={() => void handleApplyActions(m.answer!)}
                      >
                        {busy === 'apply' ? <Spinner /> : <Check />} Run {pickedActions.length} step{pickedActions.length === 1 ? '' : 's'}
                      </Button>
                      {actionResults && (
                        <ul className="mt-2 flex flex-col gap-1 text-[11px]">
                          {actionResults.map((r, ri) => (
                            <li key={ri} className={r.ok ? 'text-success' : 'text-destructive'}>
                              {r.ok ? '✓' : '✗'} {r.label}: {r.detail}
                            </li>
                          ))}
                        </ul>
                      )}
                    </div>
                  )}

                  <span className="text-[10px] opacity-60 self-end">
                    {new Date(m.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                  </span>
                </div>
                {m.role === 'user' && (
                  <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground text-xs font-semibold">
                    You
                  </div>
                )}
              </div>
            ))}

            {/* Live Streaming Indicator */}
            {currentJob && currentJob.state === 'running' && (
              <div className="flex gap-3 text-sm justify-start">
                <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-teal-500/15 text-teal-600 dark:text-teal-400">
                  <Bot className="size-4" />
                </div>
                <div className="flex flex-col gap-2 rounded-lg border border-border bg-muted/40 p-4 text-foreground max-w-[85%]">
                  <p className="flex items-center gap-2 text-xs text-muted-foreground">
                    <Spinner /> Reading OpenLocalServer RAG knowledge base & drafting answer…
                  </p>
                  {currentJob.partial && (
                    <div className="whitespace-pre-wrap font-sans text-xs sm:text-sm">
                      {currentJob.partial}
                    </div>
                  )}
                  <Button variant="ghost" size="sm" className="self-start h-7 text-xs text-destructive" onClick={handleCancelJob}>
                    <Square className="size-3" /> Stop generating
                  </Button>
                </div>
              </div>
            )}
            <div ref={chatBottomRef} />
          </div>

          {/* Prompt Preview Card */}
          {promptPreview && (
            <div className="flex flex-col gap-2 rounded-lg border border-border bg-muted/20 p-3 text-xs">
              <div className="flex items-center justify-between">
                <span className="font-semibold text-foreground">
                  Exact Prompt Sent to {promptPreview.provider_name} ({promptPreview.host})
                </span>
                <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={() => setPromptPreview(null)}>
                  Hide
                </Button>
              </div>
              <p className="text-muted-foreground">Model: {promptPreview.model} · Secrets are automatically redacted before sending.</p>
              {promptPreview.attachments.length > 0 && (
                <p className="text-teal-600 dark:text-teal-400">Attached Knowledge: {promptPreview.attachments.join(' · ')}</p>
              )}
              {promptPreview.messages.map((pm, pmi) => (
                <div key={pmi} className="flex flex-col gap-1">
                  <span className="font-semibold uppercase text-[10px] text-muted-foreground">{pm.role}</span>
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted/70 p-2 font-mono text-[11px]">
                    {pm.content}
                  </pre>
                </div>
              ))}
            </div>
          )}

          {/* Question Input Bar */}
          <div className="flex flex-col gap-2.5 rounded-xl border border-border bg-card p-3 shadow-xs">
            {isAiReady && provider && !provider.local && (
              <div className="flex items-center justify-between border-b border-border/60 pb-2">
                <Toggle
                  checked={remoteOk}
                  onChange={setRemoteOk}
                  label={`Allow sending prompt to ${provider.name} (${new URL(provider.base_url).host})`}
                  hint="Prompt leaves this PC. Secrets are scrubbed first."
                />
                <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={handlePreviewPrompt}>
                  {promptPreview ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                  {promptPreview ? 'Hide preview' : 'Inspect prompt'}
                </Button>
              </div>
            )}

            <div className="flex items-end gap-2">
              <Textarea
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey || !e.shiftKey)) {
                    e.preventDefault()
                    if (isAiReady && (!provider || provider.local || remoteOk)) {
                      askQuestion(question)
                    }
                  }
                }}
                disabled={busy !== null || !isAiReady}
                placeholder={
                  isAiReady
                    ? 'Ask anything about OpenLocalServer, how to use features, troubleshoot, or configure web sites… (Press Enter to send)'
                    : 'Configure an AI provider in Settings → AI assistant to ask questions.'
                }
                rows={2}
                className="resize-none font-sans text-sm"
              />

              <div className="flex flex-col gap-1.5 shrink-0">
                <Button
                  onClick={() => askQuestion(question)}
                  disabled={
                    busy !== null ||
                    !isAiReady ||
                    !question.trim() ||
                    (provider && !provider.local && !remoteOk)
                  }
                  className="h-10 px-4"
                >
                  {busy === 'ask' ? <Spinner /> : <Send className="size-4" />}
                  Ask
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  title="Clear conversation history"
                  onClick={handleClearHistory}
                  className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground"
                >
                  <RotateCcw className="size-3" />
                  Clear
                </Button>
              </div>
            </div>

            {/* Quick Navigation Help Links */}
            <div className="flex flex-wrap items-center gap-1.5 pt-1 text-[11px] text-muted-foreground">
              <span>Quick Jump:</span>
              <button onClick={() => onNavigate('sites')} className="hover:text-foreground underline">Sites</button>
              <span>·</span>
              <button onClick={() => onNavigate('webserver')} className="hover:text-foreground underline">Web server</button>
              <span>·</span>
              <button onClick={() => onNavigate('databases')} className="hover:text-foreground underline">Databases</button>
              <span>·</span>
              <button onClick={() => onNavigate('runtimes')} className="hover:text-foreground underline">Runtimes</button>
              <span>·</span>
              <button onClick={() => onNavigate('tunnels')} className="hover:text-foreground underline">Tunnels</button>
              <span>·</span>
              <button onClick={() => onNavigate('logs')} className="hover:text-foreground underline">Logs</button>
              <span>·</span>
              <button onClick={() => onNavigate('settings')} className="hover:text-foreground underline">Settings</button>
            </div>
          </div>
        </div>
      )}

      {/* TAB 2: APP USER MANUAL & REFERENCE */}
      {activeTab === 'manual' && (
        <div className="flex flex-col gap-4">
          {/* Manual Toolbar */}
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-card p-3 shadow-xs">
            <div className="relative flex-1 min-w-[240px]">
              <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
              <input
                type="text"
                value={manualSearch}
                onChange={(e) => setManualSearch(e.target.value)}
                placeholder="Search manual by topic, feature, or directive (e.g. DDNS, PHP, SSL, .htaccess)…"
                className="w-full rounded-md border border-border bg-background pl-8 pr-3 py-1.5 text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-teal-500"
              />
            </div>

            <div className="flex items-center gap-2">
              <Button variant="secondary" size="sm" onClick={handleCopyManual}>
                {copiedManual ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />}
                {copiedManual ? 'Copied to Clipboard' : 'Copy Full Markdown'}
              </Button>
              <Button variant="secondary" size="sm" onClick={handleDownloadManual}>
                <Download className="size-3.5" />
                Download .md
              </Button>
              <Button variant="secondary" size="sm" onClick={handlePrintManual}>
                <Printer className="size-3.5" />
                Print / Save PDF
              </Button>
            </div>
          </div>

          {/* Manual Content Display */}
          <div className="flex flex-col gap-6 rounded-xl border border-border bg-card p-6 shadow-xs leading-relaxed text-sm">
            {filteredManualSections.length === 0 ? (
              <div className="py-12 text-center text-muted-foreground">
                No sections matched &ldquo;{manualSearch}&rdquo;. Try another search term.
              </div>
            ) : (
              filteredManualSections.map((sec, idx) => (
                <div key={idx} className="flex flex-col gap-3 border-b border-border/60 pb-6 last:border-b-0">
                  <h2 className="text-base font-semibold text-foreground flex items-center gap-2">
                    <span className="flex size-6 items-center justify-center rounded-md bg-teal-500/10 text-xs font-mono text-teal-600 dark:text-teal-400">
                      {idx + 1}
                    </span>
                    {sec.title}
                  </h2>
                  <div className="whitespace-pre-wrap font-sans text-muted-foreground text-xs sm:text-sm pl-8">
                    {sec.content}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {/* TAB 3: VERSION 2.3.0 RAG HIGHLIGHTS */}
      {activeTab === 'version' && (
        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <CardTitle className="text-base flex items-center gap-2">
                  <Sparkles className="size-4 text-teal-600 dark:text-teal-400" />
                  What&rsquo;s New in OpenLocalServer v2.3.0 (RAG Specification)
                </CardTitle>
                <Badge variant="success">Current Release</Badge>
              </div>
              <CardDescription>
                Detailed overview of the features, network capabilities, and enhancements introduced in version 2.3.0.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-6 text-sm">
              <div className="grid gap-4 sm:grid-cols-2">
                <div className="rounded-lg border border-border p-4 bg-muted/20 flex flex-col gap-2">
                  <h3 className="font-semibold text-foreground flex items-center gap-2">
                    <Compass className="size-4 text-teal-600" />
                    Direct Site Broadcast Controls
                  </h3>
                  <p className="text-xs text-muted-foreground leading-relaxed">
                    Live broadcast status, instant public tunnel controls, and one-click public URL copying are directly integrated into the Site Domains panel.
                  </p>
                  <Button variant="ghost" size="sm" className="self-start mt-auto text-xs px-2 h-7" onClick={() => onNavigate('sites')}>
                    Open Sites Page <ExternalLink className="size-3 ml-1" />
                  </Button>
                </div>

                <div className="rounded-lg border border-border p-4 bg-muted/20 flex flex-col gap-2">
                  <h3 className="font-semibold text-foreground flex items-center gap-2">
                    <Wrench className="size-4 text-teal-600" />
                    Dynamic DNS (Cloudflare Sync)
                  </h3>
                  <p className="text-xs text-muted-foreground leading-relaxed">
                    Multi-provider DNS synchronization for custom domains with verified Cloudflare API integration, conflict resolution, and automated router troubleshooting.
                  </p>
                  <Button variant="ghost" size="sm" className="self-start mt-auto text-xs px-2 h-7" onClick={() => onNavigate('sites')}>
                    Manage Domains <ExternalLink className="size-3 ml-1" />
                  </Button>
                </div>

                <div className="rounded-lg border border-border p-4 bg-muted/20 flex flex-col gap-2">
                  <h3 className="font-semibold text-foreground flex items-center gap-2">
                    <Layers className="size-4 text-teal-600" />
                    Multi-Interface Web Binding (0.0.0.0)
                  </h3>
                  <p className="text-xs text-muted-foreground leading-relaxed">
                    Nginx, Apache, and Caddy now bind to `0.0.0.0`, allowing phones, tablets, and LAN devices on your local Wi-Fi to access your local development sites directly via LAN IP.
                  </p>
                  <Button variant="ghost" size="sm" className="self-start mt-auto text-xs px-2 h-7" onClick={() => onNavigate('webserver')}>
                    Web Server Config <ExternalLink className="size-3 ml-1" />
                  </Button>
                </div>

                <div className="rounded-lg border border-border p-4 bg-muted/20 flex flex-col gap-2">
                  <h3 className="font-semibold text-foreground flex items-center gap-2">
                    <HelpCircle className="size-4 text-teal-600" />
                    Auto Folder & Welcome Page Creation
                  </h3>
                  <p className="text-xs text-muted-foreground leading-relaxed">
                    When registering new Static or Plain PHP sites, OLS automatically creates the target folder if missing and generates an initial responsive `index.html` or `index.php` welcome page.
                  </p>
                  <Button variant="ghost" size="sm" className="self-start mt-auto text-xs px-2 h-7" onClick={() => onNavigate('sites')}>
                    Add New Site <ExternalLink className="size-3 ml-1" />
                  </Button>
                </div>
              </div>

              <div className="rounded-lg border border-border p-4 flex flex-col gap-2 bg-card">
                <h4 className="font-semibold text-foreground">Need help functioning the app?</h4>
                <p className="text-xs text-muted-foreground">
                  Switch to the <strong>Ask Assistant</strong> tab above to ask any specific question in natural language. The AI is grounded in the full v2.3.0 knowledge base and provides clean, step-by-step guidance without unnecessary fluff.
                </p>
                <Button size="sm" className="self-start mt-1" onClick={() => setActiveTab('chat')}>
                  <MessageSquare className="size-3.5" />
                  Ask AI About OpenLocalServer
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  )
}
