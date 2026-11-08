import { ShieldAlert, ShieldCheck } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type AuditResult, type Diagnostic, type EolItem, type ProjectDetail, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'
import { eolLabel, timeAgo } from '@/lib/hooks'

const SEVERITY_VARIANT: Record<string, 'destructive' | 'warning' | 'secondary' | 'outline'> = {
  critical: 'destructive',
  high: 'destructive',
  moderate: 'warning',
  low: 'secondary',
  unknown: 'outline',
}

function count(results: AuditResult[], severity: string) {
  return results.reduce((n, r) => n + r.tools.reduce((m, t) => m + t.findings.filter((f) => f.severity === severity).length, 0), 0)
}

/**
 * §169 Security tab: check this project's Composer, npm and pnpm packages for known
 * vulnerabilities, and see whether its PHP version still gets security fixes. Text from the
 * registries is shown as plain text; links open through the core.
 */
export function SecurityPanel({ detail }: { detail: ProjectDetail }) {
  const projectId = detail.project.id
  const php = detail.resolved.find((r) => r.id === 'php')?.installed_version ?? null
  const [result, setResult] = useState<AuditResult | null>(null)
  const [eol, setEol] = useState<EolItem[]>([])
  const [taskId, setTaskId] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  const load = useCallback(async () => {
    try {
      const a = await runCommand({ type: 'list_audit_results' })
      if (a.type === 'audit_results') setResult(a.results.find((r) => r.project_id === projectId) ?? null)
      const e = await runCommand({ type: 'get_eol_report' })
      if (e.type === 'eol_report') setEol(e.items)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }, [projectId])
  useEffect(() => {
    void load()
  }, [load])

  async function check() {
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'audit_project', project_id: projectId })
      if (r.type === 'task_started') setTaskId(r.id)
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const mine = eol.filter((i) => i.runtime === 'php' && php && i.version === php)

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border px-3 py-2 text-sm">
        <span className="text-muted-foreground">{result ? `Last checked ${timeAgo(result.checked_ms)}` : 'Not checked yet.'}</span>
        <Button size="sm" className="cursor-pointer" disabled={busy} onClick={() => void check()}>
          {busy ? <Spinner /> : <ShieldCheck />} Check now
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">Asks the package registries about known problems in the packages listed in your lock files. It needs the internet and only runs when you press the button.</p>

      {mine.map((i) => {
        const label = eolLabel(i.status)
        return label ? (
          <div key={i.version} className="flex items-center gap-2 rounded-md border border-border px-3 py-2 text-sm">
            <ShieldAlert className="size-4" /> PHP {i.version}
            <Badge variant={label.variant}>{label.text}</Badge>
          </div>
        ) : null
      })}

      {result?.tools.map((t) => (
        <section key={t.tool} className="flex flex-col gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            {t.tool}
            {!t.ok && <Badge variant="destructive">failed</Badge>}
            {t.ok && <span className="text-xs font-normal text-muted-foreground">{t.findings.length === 0 ? 'No known problems' : `${t.findings.length} finding(s)`}</span>}
          </h3>
          {t.error && <p className="text-xs text-destructive">{t.error}</p>}
          {t.info.map((line) => (
            <p key={line} className="text-xs text-muted-foreground">
              {line}
            </p>
          ))}
          {t.findings.map((f, i) => (
            <div key={`${f.package}:${i}`} className="flex flex-col gap-1 rounded-lg border border-border px-3 py-2">
              <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                <Badge variant={SEVERITY_VARIANT[f.severity] ?? 'outline'}>{f.severity}</Badge>
                <span className="font-mono text-xs">{f.package}</span>
                {f.affected && <span className="text-xs font-normal text-muted-foreground">{f.affected}</span>}
              </p>
              <p className="text-sm">{f.title}</p>
              <p className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                {f.fix && <span>Fix: {f.fix}</span>}
                {f.url && (
                  <button type="button" className="cursor-pointer text-primary underline" onClick={() => void runCommand({ type: 'open_url', url: f.url ?? '' }).catch((err) => setError(asDiagnostic(err)))}>
                    {f.advisory ?? 'Details'}
                  </button>
                )}
              </p>
            </div>
          ))}
        </section>
      ))}
      {result && result.tools.length === 0 && <p className="text-sm text-muted-foreground">This project has no Composer, npm or pnpm lock file to check.</p>}
      <TaskOutputDialog taskId={taskId} onClose={() => setTaskId(null)} onFinished={() => void load()} />
    </div>
  )
}

/** Dashboard card: totals across projects and the versions that are past their end of life. */
export function SecuritySummaryCard({ onOpenProjects }: { onOpenProjects?: () => void }) {
  const [results, setResults] = useState<AuditResult[]>([])
  const [eol, setEol] = useState<EolItem[]>([])
  const [taskId, setTaskId] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const load = useCallback(async () => {
    try {
      const a = await runCommand({ type: 'list_audit_results' })
      if (a.type === 'audit_results') setResults(a.results)
      const e = await runCommand({ type: 'get_eol_report' })
      if (e.type === 'eol_report') setEol(e.items)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  const critical = count(results, 'critical')
  const high = count(results, 'high')
  const ended = eol.filter((i) => i.status.state === 'ended')
  if (results.length === 0 && ended.length === 0) return null

  return (
    <Card className="shadow-xs">
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <ShieldAlert className="size-4" /> Security
        </CardTitle>
        <CardDescription>Known package problems and versions that no longer get security fixes.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center justify-between gap-3 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
        <span className="flex flex-wrap items-center gap-2">
          {results.length > 0 && (
            <>
              <Badge variant={critical > 0 ? 'destructive' : 'secondary'}>{critical} critical</Badge>
              <Badge variant={high > 0 ? 'destructive' : 'secondary'}>{high} high</Badge>
            </>
          )}
          {ended.map((i) => (
            <Badge key={`${i.runtime}:${i.version}`} variant="warning">
              {i.runtime} {i.version} ended
            </Badge>
          ))}
        </span>
        <span className="flex gap-2">
          {onOpenProjects && (
            <Button size="sm" variant="ghost" onClick={onOpenProjects}>
              Open projects
            </Button>
          )}
          <Button
            size="sm"
            variant="secondary"
            onClick={() =>
              void runCommand({ type: 'audit_all_projects' })
                .then((r) => {
                  if (r.type === 'task_started') setTaskId(r.id)
                })
                .catch((err) => setError(asDiagnostic(err)))
            }
          >
            Check all projects
          </Button>
        </span>
      </CardContent>
      <TaskOutputDialog taskId={taskId} onClose={() => setTaskId(null)} onFinished={() => void load()} />
    </Card>
  )
}
