import { Eye, EyeOff, FolderOpen, Info, RefreshCw, ShieldAlert, ShieldCheck } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { SecurityDetailsDialog } from '@/components/SecurityDetailsDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Tooltip } from '@/components/ui/tooltip'
import { type AuditResult, type Diagnostic, type EolItem, type ProjectDetail, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'
import { eolLabel, timeAgo } from '@/lib/hooks'
import { eolKey, findingKey } from '@/lib/securityIgnore'
import { useIgnoredSecurity } from '@/lib/useIgnoredSecurity'

const SEVERITY_VARIANT: Record<string, 'destructive' | 'warning' | 'secondary' | 'outline'> = {
  critical: 'destructive',
  high: 'destructive',
  moderate: 'warning',
  low: 'secondary',
  unknown: 'outline',
}

function count(results: AuditResult[], severity: string, ignored: string[]) {
  return results.reduce(
    (n, r) => n + r.tools.reduce((m, t) => m + t.findings.filter((f) => f.severity === severity && !ignored.includes(findingKey(r.project_id, t.tool, f))).length, 0),
    0,
  )
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
  const { ignored, setHidden, showAll } = useIgnoredSecurity()
  const [showHidden, setShowHidden] = useState(false)

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
  const hiddenCount =
    mine.filter((i) => ignored.includes(eolKey(i.runtime, i.version))).length +
    (result?.tools.reduce((n, t) => n + t.findings.filter((f) => ignored.includes(findingKey(projectId, t.tool, f))).length, 0) ?? 0)

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

      {hiddenCount > 0 && (
        <div className="flex items-center justify-between gap-2 rounded-md border border-dashed border-border px-3 py-2 text-xs text-muted-foreground">
          <span>{hiddenCount} hidden</span>
          <span className="flex gap-2">
            <Button size="sm" variant="ghost" className="h-6 cursor-pointer px-2 text-xs" onClick={() => setShowHidden((v) => !v)}>
              {showHidden ? 'Hide them again' : 'Show them'}
            </Button>
            <Button size="sm" variant="ghost" className="h-6 cursor-pointer px-2 text-xs" onClick={showAll}>
              Unhide all
            </Button>
          </span>
        </div>
      )}

      {mine.map((i) => {
        const label = eolLabel(i.status)
        const key = eolKey(i.runtime, i.version)
        const hidden = ignored.includes(key)
        return label && (!hidden || showHidden) ? (
          <div key={i.version} className={`flex items-center gap-2 rounded-md border border-border px-3 py-2 text-sm ${hidden ? 'opacity-60' : ''}`}>
            <ShieldAlert className="size-4" /> PHP {i.version}
            <Badge variant={label.variant}>{label.text}</Badge>
            <Button size="sm" variant="ghost" className="ml-auto h-7 cursor-pointer px-2 text-xs" title={hidden ? 'Show this again' : 'Hide this'} onClick={() => setHidden(key, !hidden)}>
              {hidden ? <Eye className="size-3.5" /> : <EyeOff className="size-3.5" />} {hidden ? 'Unhide' : 'Hide'}
            </Button>
          </div>
        ) : null
      })}

      {result?.tools.map((t) => (
        <section key={t.tool} className="flex flex-col gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            {t.tool}
            {!t.ok && <Badge variant="destructive">failed</Badge>}
            {t.ok && (
              <span className="text-xs font-normal text-muted-foreground">
                {t.findings.length === 0 ? 'No known problems' : `${t.findings.filter((f) => !ignored.includes(findingKey(projectId, t.tool, f))).length} finding(s)`}
              </span>
            )}
          </h3>
          {t.error && <p className="text-xs text-destructive">{t.error}</p>}
          {t.info.map((line) => (
            <p key={line} className="text-xs text-muted-foreground">
              {line}
            </p>
          ))}
          {t.findings.map((f, i) => {
            const key = findingKey(projectId, t.tool, f)
            const hidden = ignored.includes(key)
            if (hidden && !showHidden) return null
            return (
            <div key={`${f.package}:${i}`} className={`flex flex-col gap-1 rounded-lg border border-border px-3 py-2 ${hidden ? 'opacity-60' : ''}`}>
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
                <button type="button" className="ml-auto inline-flex cursor-pointer items-center gap-1 hover:text-foreground" onClick={() => setHidden(key, !hidden)}>
                  {hidden ? <Eye className="size-3" /> : <EyeOff className="size-3" />} {hidden ? 'Unhide' : 'Hide'}
                </button>
              </p>
            </div>
            )
          })}
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
  const [showDetails, setShowDetails] = useState(false)
  const [detailFilter, setDetailFilter] = useState<'critical' | 'high' | 'eol' | null>(null)
  const [projectNames, setProjectNames] = useState<Record<string, string>>({})
  const { ignored, setHidden, setManyHidden, showAll } = useIgnoredSecurity()

  const load = useCallback(async () => {
    try {
      let fresh: AuditResult[] = []
      const a = await runCommand({ type: 'list_audit_results' })
      if (a.type === 'audit_results') {
        setResults(a.results)
        fresh = a.results
      }
      const p = await runCommand({ type: 'list_projects' })
      if (p.type === 'projects') setProjectNames(Object.fromEntries(p.projects.map((x) => [x.id, x.name])))
      const e = await runCommand({ type: 'get_eol_report' })
      if (e.type === 'eol_report') setEol(e.items)
      return fresh
    } catch (err) {
      setError(asDiagnostic(err))
      return []
    }
  }, [])

  // When a check finishes, show what it found instead of leaving only "N finding(s)".
  const checkFinished = useCallback(async () => {
    const fresh = await load()
    if (fresh.some((r) => r.tools.some((t) => t.findings.length > 0))) {
      setTaskId(null)
      setDetailFilter(null)
      setShowDetails(true)
    } else {
      setTaskId(null)
    }
  }, [load])
  useEffect(() => {
    void load()
  }, [load])

  const critical = count(results, 'critical', ignored)
  const high = count(results, 'high', ignored)
  const allEnded = eol.filter((i) => i.status.state === 'ended')
  const ended = allEnded.filter((i) => !ignored.includes(eolKey(i.runtime, i.version)))
  const hiddenFindings = results.reduce(
    (n, r) => n + r.tools.reduce((m, t) => m + t.findings.filter((f) => ignored.includes(findingKey(r.project_id, t.tool, f))).length, 0),
    0,
  )
  const hidden = allEnded.length - ended.length + hiddenFindings
  const dialogResults = useMemo(() => {
    if (detailFilter === 'eol') return []
    if (detailFilter === 'critical' || detailFilter === 'high') {
      return results
        .map((r) => ({
          ...r,
          tools: r.tools
            .map((t) => ({
              ...t,
              findings: t.findings.filter((f) => f.severity === detailFilter),
            }))
            .filter((t) => t.findings.length > 0),
        }))
        .filter((r) => r.tools.length > 0)
    }
    return results
  }, [results, detailFilter])

  const dialogEol = useMemo(() => {
    if (detailFilter === 'critical' || detailFilter === 'high') return []
    return eol
  }, [eol, detailFilter])

  if (results.length === 0 && allEnded.length === 0) return null

  const isChecking = taskId !== null

  const detailsDialog = (
    <SecurityDetailsDialog
      open={showDetails}
      onClose={() => {
        setShowDetails(false)
        setDetailFilter(null)
      }}
      results={dialogResults}
      eol={dialogEol}
      projectNames={projectNames}
      ignored={ignored}
      setHidden={setHidden}
      setManyHidden={setManyHidden}
      onError={(e) => setError(asDiagnostic(e))}
    />
  )

  const hasIssues = critical > 0 || high > 0 || ended.length > 0

  return (
    <Card className="flex flex-wrap items-center justify-between gap-3 px-3 py-2 min-h-[48px] shadow-xs">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />

      {/* Left group: Title + Chips */}
      <div className="flex flex-wrap items-center gap-2.5 min-w-0">
        <div className="flex items-center gap-1.5 shrink-0">
          {hasIssues ? (
            <ShieldAlert className="size-4 text-warning" />
          ) : (
            <ShieldCheck className="size-4 text-success" />
          )}
          <span className="text-sm font-medium">Security</span>
          <Tooltip content="Known package problems and versions that no longer get security fixes.">
            <span
              tabIndex={0}
              role="img"
              aria-label="Known package problems and versions that no longer get security fixes."
              title="Known package problems and versions that no longer get security fixes."
              className="inline-flex cursor-help text-muted-foreground hover:text-foreground transition-colors"
            >
              <Info className="size-3.5" />
            </span>
          </Tooltip>
        </div>

        {/* Severity & EOL chips */}
        <div className="flex flex-wrap items-center gap-1.5">
          {results.length > 0 && (
            <>
              <button
                type="button"
                className="cursor-pointer transition-transform active:scale-95 hover:opacity-85 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-full"
                title="Filter details to critical findings"
                onClick={() => {
                  setDetailFilter('critical')
                  setShowDetails(true)
                }}
              >
                <Badge variant={critical > 0 ? 'destructive' : 'secondary'} className="h-6 text-xs gap-1.5 px-2 font-medium">
                  {critical} critical
                </Badge>
              </button>
              <button
                type="button"
                className="cursor-pointer transition-transform active:scale-95 hover:opacity-85 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-full"
                title="Filter details to high findings"
                onClick={() => {
                  setDetailFilter('high')
                  setShowDetails(true)
                }}
              >
                <Badge variant={high > 0 ? 'destructive' : 'secondary'} className="h-6 text-xs gap-1.5 px-2 font-medium">
                  {high} high
                </Badge>
              </button>
            </>
          )}
          {ended.map((i) => (
            <Badge
              key={`${i.runtime}:${i.version}`}
              variant="warning"
              className="h-6 text-xs gap-1.5 px-2 font-medium cursor-pointer transition-transform active:scale-95 hover:opacity-85"
              title="Filter details to end-of-life runtimes"
              onClick={() => {
                setDetailFilter('eol')
                setShowDetails(true)
              }}
            >
              <span>{i.runtime} {i.version} ended</span>
              <button
                type="button"
                className="cursor-pointer opacity-70 hover:opacity-100 transition-opacity ml-0.5"
                title={`Hide this warning for ${i.runtime} ${i.version}`}
                aria-label={`Hide the ${i.runtime} ${i.version} warning`}
                onClick={(e) => {
                  e.stopPropagation()
                  setHidden(eolKey(i.runtime, i.version), true)
                }}
              >
                <EyeOff className="size-3" />
              </button>
            </Badge>
          ))}
        </div>
      </div>

      {/* Right group: Actions */}
      <div className="flex items-center gap-1.5 shrink-0 ml-auto">
        {hidden > 0 && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground cursor-pointer"
            title="Show the hidden security items again"
            onClick={showAll}
          >
            <Eye className="size-3.5" /> {hidden} hidden
          </Button>
        )}
        {onOpenProjects && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2.5 text-xs font-medium text-primary hover:text-primary hover:bg-primary/10 transition-colors cursor-pointer"
            onClick={onOpenProjects}
          >
            <FolderOpen className="size-3.5" /> Open projects
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2.5 text-xs font-medium text-primary hover:text-primary hover:bg-primary/10 transition-colors cursor-pointer w-[148px] justify-center"
          disabled={isChecking}
          onClick={() =>
            void runCommand({ type: 'audit_all_projects' })
              .then((r) => {
                if (r.type === 'task_started') setTaskId(r.id)
              })
              .catch((err) => setError(asDiagnostic(err)))
          }
        >
          {isChecking ? (
            <>
              <Spinner className="size-3.5 animate-spin" /> Checking…
            </>
          ) : (
            <>
              <RefreshCw className="size-3.5" /> Check all projects
            </>
          )}
        </Button>
      </div>

      {detailsDialog}
      <TaskOutputDialog taskId={taskId} onClose={() => setTaskId(null)} onFinished={() => void checkFinished()} />
    </Card>
  )
}
