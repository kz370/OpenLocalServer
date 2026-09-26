import { AlertTriangle, ChevronDown, ChevronRight, EyeOff, Info, RefreshCw, Wrench, XCircle } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type Finding, runCommand } from '@/core'
import type { AskAi } from '@/lib/ai'
import { useAction } from '@/lib/hooks'

/** What the assistant is asked to explain for one finding. */
export function explainFinding(f: Finding): AskAi {
  return {
    title: 'Explain this problem',
    description: f.problem,
    request: { feature: 'explain', title: f.problem, text: [f.problem, `Cause: ${f.cause}`, `Suggested fix: ${f.fix}`, ...f.details.filter(Boolean)].join('\n') },
    question: 'optional',
  }
}

const ICON = { error: XCircle, warning: AlertTriangle, info: Info }
const COLOR = { error: 'text-destructive', warning: 'text-warning', info: 'text-muted-foreground' }

/** §112: Problem / Cause / Fix, with [Fix], [Ignore] and [Details] on each finding. */
export function DiagnosticsCard() {
  const [findings, setFindings] = useState<Finding[] | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [showIgnored, setShowIgnored] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const scan = useCallback(async () => {
    const res = await runCommand({ type: 'run_diagnostics' })
    if (res.type === 'diagnostics') setFindings(res.findings)
  }, [])

  useEffect(() => {
    scan().catch(() => setFindings([]))
  }, [scan])

  const active = findings?.filter((f) => !f.ignored) ?? []
  const ignored = findings?.filter((f) => f.ignored) ?? []
  const shown = showIgnored ? findings ?? [] : active

  async function fix(f: Finding) {
    if (!f.fix_command) return
    const res = await runCommand(f.fix_command)
    setNotice(res.type === 'process_started' ? 'Started. Follow its output on the Processes page.' : `Applied: ${f.fix}`)
    await scan()
  }

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between space-y-0 pb-2">
        <div>
          <CardTitle className="text-sm">Diagnostics</CardTitle>
          <CardDescription>
            {findings === null ? 'Checking…' : active.length === 0 ? 'No problems found.' : `${active.length} thing${active.length === 1 ? '' : 's'} to look at.`}
          </CardDescription>
        </div>
        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => run('scan', scan)} title="Check again">
          {busy === 'scan' ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />}
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {notice && <p className="text-sm text-success">{notice}</p>}
        {shown.map((f) => {
          const Icon = ICON[f.severity]
          const expanded = open === f.id
          return (
            <div key={f.id} className={`rounded-lg border border-border p-3 ${f.ignored ? 'opacity-60' : ''}`}>
              <div className="flex items-start gap-2">
                <Icon className={`mt-0.5 size-4 shrink-0 ${COLOR[f.severity]}`} />
                <div className="min-w-0 flex-1 text-sm">
                  <div className="font-medium">
                    {f.problem} {f.ignored && <Badge variant="secondary">ignored</Badge>}
                  </div>
                  <div className="text-muted-foreground">
                    <b className="font-medium text-foreground/80">Cause:</b> {f.cause}
                  </div>
                  <div className="text-muted-foreground">
                    <b className="font-medium text-foreground/80">Fix:</b> {f.fix}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {!f.ignored && <AiButton ask={() => explainFinding(f)} />}
                  {f.fix_command && !f.ignored && (
                    <Button size="sm" disabled={busy !== null} onClick={() => run(`fix:${f.id}`, () => fix(f))}>
                      {busy === `fix:${f.id}` ? <Spinner className="size-3.5" /> : <Wrench />} Fix
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy !== null}
                    title={f.ignored ? 'Show this again' : 'Hide this finding'}
                    onClick={() =>
                      run(`ignore:${f.id}`, async () => {
                        const res = await runCommand({ type: 'ignore_diagnostic', id: f.id, ignore: !f.ignored })
                        if (res.type === 'diagnostics') setFindings(res.findings)
                      })
                    }
                  >
                    <EyeOff /> {f.ignored ? 'Unignore' : 'Ignore'}
                  </Button>
                  {f.details.length > 0 && (
                    <Button size="sm" variant="ghost" onClick={() => setOpen(expanded ? null : f.id)}>
                      {expanded ? <ChevronDown /> : <ChevronRight />} Details
                    </Button>
                  )}
                </div>
              </div>
              {expanded && (
                <ul className="mt-2 list-disc pl-10 font-mono text-xs text-muted-foreground">
                  {f.details.filter(Boolean).map((d) => (
                    <li key={d} className="break-all">
                      {d}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )
        })}
        {ignored.length > 0 && (
          <button className="self-start text-xs text-muted-foreground hover:text-foreground" onClick={() => setShowIgnored((v) => !v)}>
            {showIgnored ? 'Hide' : 'Show'} {ignored.length} ignored
          </button>
        )}
      </CardContent>
    </Card>
  )
}
