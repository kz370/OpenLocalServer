import {
  BookOpen,
  Bot,
  Check,
  Copy,
  Download,
  Eye,
  EyeOff,
  Info,
  Layers,
  MessageSquare,
  Printer,
  RefreshCw,
  RotateCcw,
  Search,
  Send,
  Sparkles,
  Square,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { Spinner } from '@/components/Spinner'
import { Markdown } from '@/components/ui/Markdown'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Textarea, Toggle } from '@/components/ui/form'
import { type AiAnswer, type AiJobView, type AiPrompt, type CoreCommand, runCommand } from '@/core'
import { useAiState } from '@/lib/ai'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'
import releaseNotes from '../../../release-notes/v2.3.0.md?raw'

interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  status?: 'pending' | 'failed' | 'done' | 'stopped'
  question?: string
  error?: string
  answer?: AiAnswer | null
  timestamp: number
}

function StreamingMarkdown({
  content,
  streaming,
  className,
}: {
  content: string
  streaming: boolean
  className?: string
}) {
  const [visibleContent, setVisibleContent] = useState(() => (streaming ? '' : content))
  const hasStreamedRef = useRef(streaming)

  useEffect(() => {
    if (streaming) hasStreamedRef.current = true

    if (!hasStreamedRef.current) {
      setVisibleContent(content)
      return
    }

    // Streaming finished — flush everything so the plan block is fully visible.
    if (!streaming && visibleContent !== content) {
      setVisibleContent(content)
      return
    }

    if (visibleContent.length >= content.length) {
      if (!streaming) hasStreamedRef.current = false
      return
    }

    const timer = window.setTimeout(() => {
      setVisibleContent((current) => {
        let nextIndex = current.length
        for (let i = 0; i < 2 && nextIndex < content.length; i += 1) {
          const codePoint = content.codePointAt(nextIndex)
          nextIndex += codePoint !== undefined && codePoint > 0xffff ? 2 : 1
        }
        return content.slice(0, nextIndex)
      })
    }, 16)

    return () => window.clearTimeout(timer)
  }, [content, streaming, visibleContent])

  return <Markdown content={visibleContent} className={className} />
}

export function AskAiPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [activeTab, setActiveTab] = useState<'chat' | 'manual' | 'version'>('chat')
  const [question, setQuestion] = useState('')
  const [remoteOk, setRemoteOk] = useState(false)
  const [promptPreview, setPromptPreview] = useState<AiPrompt | null>(null)
  const [currentJob, setCurrentJob] = useState<AiJobView | null>(null)
  const [pickedActions, setPickedActions] = useState<number[]>([])
  const [executedActions, setExecutedActions] = useState<number[]>([])
  const [actionResults, setActionResults] = useState<{ label: string; ok: boolean; detail: string }[] | null>(null)
  const [pendingJobId, setPendingJobId] = useState<string | null>(null)
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
          "👋 Hello! I am your OpenLocalServer Assistant.\n\nI can answer questions about OpenLocalServer and take actions across the whole app — create sites, manage services, start tunnels, create databases, install runtimes, issue certificates, and more.\n\nEverything I propose shows up as a plan you review before anything runs. Destructive steps need a separate confirmation.\n\nTry asking:\n• \"Create a new site called myapp.test with PHP 8.3 and MariaDB\"\n• \"Start the Cloudflare tunnel for shop.test\"\n• \"Export a bundle for shop.test\"\n• \"Why is MariaDB not running?\"",
        timestamp: Date.now(),
      },
    ]
  })

  const aiState = useAiState()
  const { busy, error, setError, run } = useAction()
  const chatFeedRef = useRef<HTMLDivElement>(null)

  // Persist conversation history in session storage
  useEffect(() => {
    try {
      sessionStorage.setItem('ols:askai-history', JSON.stringify(messages))
    } catch {
      // ignore
    }
  }, [messages])

  // Scroll to bottom when new messages arrive (respect user scrolling up)
  useEffect(() => {
    if (activeTab === 'chat') {
      const feed = chatFeedRef.current
      if (feed && feed.scrollHeight - feed.scrollTop <= feed.clientHeight + 2) {
        feed.scrollTop = feed.scrollHeight
      }
    }
  }, [messages, activeTab])

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
    if (!currentJob || currentJob.state !== 'running' || !pendingJobId) return
    const id = currentJob.id
    const timer = setInterval(() => {
      runCommand({ type: 'ai_job', job_id: id })
        .then((r) => {
          if (r.type === 'ai_job') {
            setCurrentJob(r.job)
            const job = r.job
            if (job.state === 'running') {
              setMessages((prev) => prev.map((m) =>
                m.id === pendingJobId ? { ...m, content: job.partial } : m))
            }
            if (job.state === 'done' && job.answer && (job.answer.text.trim() || job.answer.actions.length > 0)) {
              const answer = job.answer
              setMessages((prev) => prev.map((m) =>
                m.id === pendingJobId
                  ? { ...m, content: answer.text, status: 'done', answer, timestamp: Date.now() }
                  : m))
              if (answer.actions.length > 0) {
                setPickedActions(answer.actions.filter((a) => !a.destructive).map((_, i) => i))
              }
              setCurrentJob(null)
              setPendingJobId(null)
            } else if (job.state === 'failed' || (job.state === 'done' && !job.answer?.text?.trim())) {
              setMessages((prev) => prev.map((m) =>
                m.id === pendingJobId
                  ? { ...m, status: 'failed', error: job.error ?? 'The model returned an empty answer.', timestamp: Date.now() }
                  : m))
              setCurrentJob(null)
              setPendingJobId(null)
            }
          }
        })
        .catch(() => undefined)
    }, 400)
    return () => clearInterval(timer)
  }, [currentJob?.id, currentJob?.state, pendingJobId, setError]) // eslint-disable-line react-hooks/exhaustive-deps

  const provider = useMemo(() => {
    if (!aiState?.settings) return undefined
    const featureId = aiState.settings.features['assistant']
    return aiState.settings.providers.find((p) => p.id === featureId) ?? aiState.settings.providers[0]
  }, [aiState])

  const isAiReady = !!aiState?.settings.enabled && !!provider && !!provider.model

  const askQuestion = (qText: string) => {
    const trimmed = qText.trim()
    if (!trimmed) return
    setError(null)
    setPromptPreview(null)
    setActionResults(null)
    setExecutedActions([])

    // Add user message to chat immediately
    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: trimmed,
      timestamp: Date.now(),
    }
    // Conversation history before this turn so the assistant remembers context.
    const history = messages.map((m) => ({ role: m.role, content: m.content }))
    setMessages((prev) => [...prev, userMsg])
    setQuestion('')

    void run('ask', async () => {
      let r
      try {
        r = await runCommand({
          type: 'ai_start',
          request: {
            feature: 'assistant',
            question: trimmed,
            messages: history,
          },
          confirm_remote: remoteOk,
        })
      } catch (cause) {
        setError({
          problem: 'The assistant request was refused.',
          cause: cause instanceof Error ? cause.message : String(cause),
          fix: 'Check Settings → AI assistant: ensure it is on, a provider is added, and a model is chosen.',
        })
        return
      }
      if (r.type === 'ai_job') {
        const job = r.job
        setCurrentJob(job)
        if (job.state === 'done' && job.answer && (job.answer.text.trim() || job.answer.actions.length > 0)) {
          // Immediate answer — show it as a completed assistant message.
          const answer = job.answer
          const pendingId = `job-${Date.now()}`
          setPendingJobId(pendingId)
          setMessages((prev) => [
            ...prev,
            {
              id: pendingId,
              role: 'assistant',
              content: answer.text,
              status: 'done',
              answer,
              question: trimmed,
              timestamp: Date.now(),
            },
          ])
          if (answer.actions.length > 0) {
            setPickedActions(answer.actions.filter((a) => !a.destructive).map((_, i) => i))
          }
          setCurrentJob(null)
          setPendingJobId(null)
        } else if (job.state === 'running') {
          const pendingId = `job-${Date.now()}`
          setPendingJobId(pendingId)
          setMessages((prev) => [
            ...prev,
            {
              id: pendingId,
              role: 'assistant',
              content: job.partial,
              status: 'pending',
              question: trimmed,
              timestamp: Date.now(),
            },
          ])
        } else {
          // done with empty answer, or failed — show as failed so user can resend.
          setMessages((prev) => [
            ...prev,
            {
              id: `job-${Date.now()}`,
              role: 'assistant',
              content: job.answer?.text ?? 'The model returned an empty answer. Try again, or pick another model.',
              status: 'failed',
              error: job.error ?? 'empty answer',
              question: trimmed,
              timestamp: Date.now(),
            },
          ])
          setCurrentJob(null)
          setPendingJobId(null)
        }
      } else {
        setError({
          problem: 'The assistant did not return a usable response.',
          cause: `unexpected response type: ${(r as { type: string }).type}`,
          fix: 'Try again, or pick another model in Settings.',
        })
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
          feature: 'assistant',
          question: trimmed,
        },
      })
      if (r.type === 'ai_prompt') {
        setPromptPreview(r.prompt)
      }
    })
  }

  const handleCancelJob = async () => {
    const jobId = currentJob?.id
    if (!jobId) return
    try {
      await runCommand({ type: 'ai_cancel', job_id: jobId })
      setMessages((prev) => prev.map((m) =>
        m.id === pendingJobId && m.status === 'pending'
          ? { ...m, status: 'stopped', timestamp: Date.now() }
          : m))
      setCurrentJob((job) => job?.id === jobId ? null : job)
      setPendingJobId(null)
    } catch (cause) {
      setError({
        problem: 'The assistant could not stop generating.',
        cause: cause instanceof Error ? cause.message : String(cause),
        fix: 'Try stopping the response again.',
      })
    }
  }

  const handleResend = (q: string) => {
    setError(null)
    setPromptPreview(null)
    setActionResults(null)
    setExecutedActions([])

    // Conversation history before this resend so the assistant remembers context.
    const history = messages.map((m) => ({ role: m.role, content: m.content }))

    // Replace the failed assistant message with a fresh pending one; don't add a duplicate user message.
    setMessages((prev) => prev.map((m) =>
      m.role === 'assistant' && m.status === 'failed' && m.question === q
        ? { ...m, status: 'pending', content: '', timestamp: Date.now() }
        : m
    ))
    void run('ask', async () => {
      const r = await runCommand({
        type: 'ai_start',
        request: {
          feature: 'assistant',
          question: q,
          messages: history,
        },
        confirm_remote: remoteOk,
      })
      if (r.type === 'ai_job') {
        const job = r.job
        setCurrentJob(job)
        if (job.state === 'done' && job.answer && (job.answer.text.trim() || job.answer.actions.length > 0)) {
          const answer = job.answer
          const pendingId = `job-${Date.now()}`
          setPendingJobId(pendingId)
          setMessages((prev) => prev.map((m) =>
            m.role === 'assistant' && m.status === 'pending' && m.question === q
              ? { ...m, id: pendingId, content: answer.text, status: 'done', answer, timestamp: Date.now() }
              : m
          ))
          if (answer.actions.length > 0) {
            setPickedActions(answer.actions.filter((a) => !a.destructive).map((_, i) => i))
          }
          setCurrentJob(null)
          setPendingJobId(null)
        } else if (job.state === 'running') {
          const pendingId = `job-${Date.now()}`
          setPendingJobId(pendingId)
          setMessages((prev) => prev.map((m) =>
            m.role === 'assistant' && m.status === 'pending' && m.question === q
              ? { ...m, id: pendingId, content: job.partial, status: 'pending', timestamp: Date.now() }
              : m
          ))
        } else {
          setMessages((prev) => prev.map((m) =>
            m.role === 'assistant' && m.status === 'pending' && m.question === q
              ? { ...m, status: 'failed', error: job.error ?? 'empty answer', timestamp: Date.now() }
              : m
          ))
          setCurrentJob(null)
          setPendingJobId(null)
        }
      } else {
        setError({
          problem: 'The assistant did not return a usable response.',
          cause: `unexpected response type: ${(r as { type: string }).type}`,
          fix: 'Try again, or pick another model in Settings.',
        })
      }
    })
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
    void run('apply', async () => {
      const r = await runCommand({
        type: 'ai_apply',
        actions: chosen.map((c) => c.command as CoreCommand),
        confirm_destructive: confirmDestructive,
      })
      if (r.type === 'ai_applied') {
        setActionResults(r.steps)
        setExecutedActions((prev) => [...new Set([...prev, ...pickedActions])])
        setPickedActions([])
      } else {
        setError({
          problem: 'The assistant could not run those steps.',
          cause: 'unexpected response from ai_apply',
          fix: 'Check the settings and try again, or run the steps manually.',
        })
      }
    })
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
    <div className={`mx-auto flex max-w-6xl flex-col gap-6 ${activeTab === 'chat' ? 'h-full min-h-0 pb-3' : 'pb-12'}`}>
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
          Version 2.3.0 Notes
        </button>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {/* TAB 1: ASK AI (CHAT) */}
      {activeTab === 'chat' && (
        <div className="flex min-h-0 flex-1 flex-col gap-3">
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

          {/* Chat Messages Feed */}
          <div ref={chatFeedRef} className="flex min-h-[180px] min-w-0 flex-1 flex-col gap-4 overflow-y-auto rounded-xl border border-border bg-card/60 p-3">
            {messages.map((m) => (
              <div
                key={m.id}
                className={`flex min-w-0 gap-3 text-sm items-start ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}
              >
                {m.role === 'assistant' && (
                  <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-teal-500/15 text-teal-600 dark:text-teal-400 mt-0.5">
                    <Bot className="size-4" />
                  </div>
                )}
                <div className={`flex min-w-0 max-w-[85%] flex-col gap-1 ${m.role === 'user' ? 'items-end' : 'items-start'}`}>
                  {m.role === 'user' ? (
                    <>
                      <div className="flex items-center gap-2 self-end px-1 text-[10px] text-muted-foreground">
                        <span className="font-medium text-foreground/80">You</span>
                        <time>
                          {new Date(m.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                        </time>
                      </div>
                      <div className="max-w-full break-words rounded-lg rounded-br-none bg-primary px-4 py-3 font-sans text-xs leading-relaxed text-primary-foreground shadow-sm whitespace-pre-wrap sm:text-sm">
                        {m.content}
                      </div>
                    </>
                  ) : (
                    <>
                      <div className="min-w-0 max-w-full rounded-lg rounded-bl-none border border-border bg-muted/40 px-4 py-3 leading-relaxed text-foreground shadow-xs">
                        <StreamingMarkdown
                          content={m.content}
                          streaming={m.status === 'pending'}
                          className="text-xs sm:text-sm"
                        />
                        {m.status === 'pending' ? (
                          <>
                            {!m.content.trim() && (
                              <div role="status" aria-label="Assistant is thinking" className="mt-2 flex items-center gap-1.5 text-teal-600 dark:text-teal-400">
                                {[0, 1, 2].map((dot) => (
                                  <span
                                    key={dot}
                                    className="size-1.5 animate-bounce rounded-full bg-current"
                                    style={{ animationDelay: `${dot * 120}ms` }}
                                  />
                                ))}
                              </div>
                            )}
                          </>
                        ) : (
                          <>
                            {m.status === 'stopped' && (
                              <p className="mt-1 text-[10px] text-muted-foreground">Response stopped.</p>
                            )}
                            {m.status === 'failed' && m.error && (
                              <p className="text-[10px] text-destructive/90 mt-1">{m.error}</p>
                            )}
                            {m.status === 'failed' && (
                              <Button
                                size="sm"
                                variant="outline"
                                className="self-start mt-2 h-7 px-2 text-xs text-destructive border-destructive/50 hover:bg-destructive/10"
                                onClick={() => void handleResend(m.question ?? '')}
                              >
                                <RefreshCw className="size-3.5 mr-1" /> Resend
                              </Button>
                            )}
                          </>
                        )}

                        {/* Actions attached to answer */}
                        {m.status === 'done' && m.answer && m.answer.actions.length > 0 && (
                          <div className="mt-3 flex flex-col gap-2 rounded-md border border-border bg-card/80 p-3 text-xs">
                            <p className="font-semibold text-foreground">Suggested executable steps (reviewed by safety gate):</p>
                            {m.answer.actions.map((act, idx) => (
                                <label key={idx} className={`flex cursor-pointer items-start gap-2 text-muted-foreground ${executedActions.includes(idx) ? 'opacity-50 pointer-events-none' : ''}`}>
                                  <Checkbox
                                    className="mt-0.5"
                                    checked={pickedActions.includes(idx) && !executedActions.includes(idx)}
                                    disabled={executedActions.includes(idx)}
                                    onChange={(checked) =>
                                      setPickedActions((prev) =>
                                        checked ? [...prev, idx] : prev.filter((i) => i !== idx),
                                      )
                                    }
                                  />
                                <span className="flex flex-col gap-1 min-w-0">
                                  <div className="flex items-center gap-2">
                                    <strong className="text-foreground">{act.label}</strong>
                                    {executedActions.includes(idx) && (
                                      <Badge variant="success" className="text-[10px]">Done</Badge>
                                    )}
                                    {act.destructive && !executedActions.includes(idx) && (
                                      <Badge variant="destructive" className="ml-1.5 text-[10px]">
                                        replaces or removes data
                                      </Badge>
                                    )}
                                  </div>
                                  {!executedActions.includes(idx) && (
                                    <details className="text-xs text-muted-foreground mt-0.5">
                                      <summary className="cursor-pointer">View parameters</summary>
                                      <pre className="mt-1 break-all font-mono text-[10px]">
                                        {JSON.stringify(act.command, null, 2)}
                                      </pre>
                                    </details>
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

                        {m.status !== 'pending' && (
                          <time className="mt-2 block text-right text-[10px] text-muted-foreground">
                            {new Date(m.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                          </time>
                        )}
                      </div>
                    </>
                  )}
                </div>
              </div>
            ))}
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
          <div className="shrink-0 rounded-xl border border-border bg-card p-2 shadow-md">
            {isAiReady && provider && !provider.local && (
              <div className="mb-2 flex items-center justify-between border-b border-border/60 pb-2">
                <Toggle
                  checked={remoteOk}
                  onChange={setRemoteOk}
                  label={`Allow cloud AI (${provider.name})`}
                  hint="Prompt leaves this PC; secrets are scrubbed."
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
                disabled={busy !== null || currentJob?.state === 'running' || !isAiReady}
                placeholder={
                  isAiReady
                    ? 'Ask about OpenLocalServer…'
                    : 'Configure an AI provider in Settings → AI assistant to ask questions.'
                }
                rows={1}
                className="h-10 min-h-10 max-h-28 min-w-0 flex-1 resize-none overflow-y-auto py-2 font-sans text-sm"
              />

              <div className="flex shrink-0 items-center gap-1">
                {currentJob?.state === 'running' ? (
                  <Button variant="destructive" onClick={() => void handleCancelJob()} className="h-9 px-3">
                    <Square className="size-3.5" />
                    Stop
                  </Button>
                ) : (
                  <Button
                    onClick={() => askQuestion(question)}
                    disabled={
                      busy !== null ||
                      !isAiReady ||
                      !question.trim() ||
                      (provider && !provider.local && !remoteOk)
                    }
                    className="h-9 px-3"
                  >
                    {busy === 'ask' ? <Spinner /> : <Send className="size-4" />}
                    Ask
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  title="Clear conversation history"
                  aria-label="Clear conversation history"
                  onClick={handleClearHistory}
                  className="size-9 text-muted-foreground hover:text-foreground"
                >
                  <RotateCcw className="size-4" />
                </Button>
              </div>
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
                  <Markdown content={sec.content} className="pl-8 text-xs sm:text-sm" />
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {/* TAB 3: VERSION 2.3.0 RELEASE NOTES */}
      {activeTab === 'version' && (
        <div className="flex flex-col gap-4 rounded-xl border border-border bg-card p-5 shadow-xs sm:p-7">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/70 pb-4">
            <div>
              <h2 className="text-lg font-semibold tracking-tight">OpenLocalServer 2.3.0</h2>
              <p className="mt-1 text-xs text-muted-foreground">Release notes · October 23, 2026</p>
            </div>
            <Badge variant="success">Current release</Badge>
          </div>
          <Markdown content={releaseNotes} className="text-sm" />
        </div>
      )}
    </div>
  )
}
