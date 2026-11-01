import { AlertTriangle, CheckCircle2, RefreshCw, Wrench, XCircle } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'

import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { asDiagnostic } from '@/components/ErrorCard'
import { type Finding, type HealthItem, runCommand } from '@/core'
import { useAction, usePoll } from '@/lib/hooks'
import { openSitesFilter } from '@/lib/nav'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

function formatCheckedRelative(timestamp: number | null, currentTime: number): string {
  if (!timestamp) return 'just now'
  const sec = Math.max(0, Math.floor((currentTime - timestamp) / 1000))
  if (sec < 45) return 'just now'
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m ago`
  const hrs = Math.floor(min / 60)
  if (hrs < 24) return `${hrs}h ago`
  return `${Math.floor(hrs / 24)}d ago`
}

/**
 * Diagnostics banner: strictly one slim row (<= 44px) in all states so layout never jumps.
 * - checking: spinner + "Checking diagnostics…"
 * - no issues: green check + "No problems found" + "Check again" (with active feedback)
 * - issues: amber/red banner with count, short list and "Fix all"
 */
export function DiagnosticsCard({
  refreshToken = 0,
  onNavigate,
  onRefresh,
  healthProblems = [],
}: {
  refreshToken?: number
  onNavigate?: (p: Page) => void
  onRefresh?: () => Promise<unknown>
  healthProblems?: HealthItem[]
}) {
  const [findings, setFindings] = useState<Finding[] | null>(null)
  const [lastChecked, setLastChecked] = useState<number | null>(null)
  const [now, setNow] = useState<number>(Date.now())
  const { busy, run } = useAction()

  // Track previous scan state for Rule 2, 3, 5
  const previousStateRef = useRef<{
    issueCount: number
    issueIds: Set<string>
    hasRunOnce: boolean
  }>({
    issueCount: 0,
    issueIds: new Set(),
    hasRunOnce: false,
  })

  // Timer ticker for relative time ("just now", "1m ago", etc.)
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 10000)
    return () => clearInterval(timer)
  }, [])

  const evaluateDiagnostics = useCallback(
    (newFindings: Finding[], currentHealth: HealthItem[]) => {
      const activeFindings = newFindings.filter((f) => !f.ignored)
      const activeHealth = currentHealth.filter((h) => h.status !== 'ok')
      const totalProblems = activeFindings.length + activeHealth.length
      const currentIssueIds = new Set([
        ...activeFindings.map((f) => f.id),
        ...activeHealth.map((h) => `health:${h.id}`),
      ])

      const prev = previousStateRef.current

      // Rule 2 & Rule 5: Check finds problems that were not there before
      if (prev.hasRunOnce && totalProblems > 0) {
        const hasNewIssue = [...currentIssueIds].some((id) => !prev.issueIds.has(id))
        const countIncreased = totalProblems > prev.issueCount

        if (hasNewIssue || countIncreased) {
          // Add history entry (and unread badge) only if the issue count increased
          // or new issue types appeared. NO toast.
          const issueSummary = activeFindings.slice(0, 3).map((f) => f.problem).join('\n')
          notify.recordHistory(
            'warning',
            `Diagnostics: ${totalProblems} ${totalProblems === 1 ? 'issue requires' : 'issues require'} attention.`,
            {
              title: 'Diagnostics',
              details: issueSummary || undefined,
              logSource: 'diagnostics',
              read: false,
              needsAttention: true,
            },
          )
        }
      } else if (prev.hasRunOnce && prev.issueCount > 0 && totalProblems === 0) {
        // Rule 3: Issues resolved (from problems to none): the bar updates; no toast.
        // Optional: quiet "Resolved" history entry, no unread badge.
        notify.recordHistory('info', 'Diagnostics: All issues resolved.', {
          title: 'Diagnostics',
          logSource: 'diagnostics',
          read: true,
          needsAttention: false,
        })
      }
      // Rule 1: Manual or automatic check with no problems: NO toast, NO history entry, NO unread badge.

      previousStateRef.current = {
        issueCount: totalProblems,
        issueIds: currentIssueIds,
        hasRunOnce: true,
      }
      setLastChecked(Date.now())
    },
    [],
  )

  const scan = useCallback(async () => {
    try {
      const res = await runCommand({ type: 'run_diagnostics' })
      if (res.type === 'diagnostics') {
        setFindings(res.findings)
        evaluateDiagnostics(res.findings, healthProblems)
      }
    } catch {
      // background scan silently catches
    }
  }, [evaluateDiagnostics, healthProblems])

  useEffect(() => {
    scan().catch(() => setFindings([]))
  }, [scan, refreshToken])

  // Poll diagnostics periodically to stay in sync with service operations
  usePoll(() => scan().catch(() => undefined), 8000)

  const handleManualCheck = useCallback(async () => {
    await run('scan', async () => {
      const startTime = Date.now()
      let resFindings: Finding[] = []
      let refreshedHealth: HealthItem[] | undefined

      try {
        const [res, refResult] = await Promise.all([
          runCommand({ type: 'run_diagnostics' }),
          onRefresh ? onRefresh() : Promise.resolve(null),
        ])
        if (res.type === 'diagnostics') {
          resFindings = res.findings
          setFindings(res.findings)
        }
        if (refResult && typeof refResult === 'object' && 'health' in refResult) {
          refreshedHealth = (refResult as { health: HealthItem[] }).health
        }
      } catch (e) {
        // Rule 4: Check fails to run (exception, timeout): error toast that stays until dismissed,
        // with "Copy details", plus a history entry.
        const diag = asDiagnostic(e)
        const problem = diag.problem || 'Diagnostics check failed to run.'
        const details = diag.cause
          ? `${diag.cause}${diag.fix ? `\nFix: ${diag.fix}` : ''}`
          : e instanceof Error
            ? e.message
            : String(e)

        notify.error(problem, {
          title: 'Diagnostics check error',
          details,
          logSource: 'diagnostics',
          action: {
            label: 'Copy details',
            onClick: async () => {
              try {
                await navigator.clipboard.writeText(`${problem}\n${details}`)
              } catch {
                // ignore
              }
            },
          },
        })
        return
      }

      // Rule 6: Show spinner and "Checking..." for at least ~400ms
      const elapsed = Date.now() - startTime
      if (elapsed < 400) {
        await new Promise((resolve) => setTimeout(resolve, 400 - elapsed))
      }

      evaluateDiagnostics(resFindings, refreshedHealth ?? healthProblems)
    })
  }, [run, onRefresh, healthProblems, evaluateDiagnostics])

  const isMissingFolderFinding = (f: Finding) =>
    f.id.startsWith('site_root_missing:') || f.id.startsWith('project_missing:')

  const syntheticHealthFindings = useMemo<Finding[]>(() => {
    return healthProblems
      .filter((h) => h.status !== 'ok')
      .map((h) => ({
        id: `health:${h.id}`,
        problem: h.label,
        cause: h.detail,
        fix: h.fix || 'Check service status in the Services panel.',
        severity: (h.status === 'warn' ? 'warning' : 'error') as 'warning' | 'error',
        details: [],
        ignored: false,
        auto_fixable: false,
        fix_command: null,
      }))
  }, [healthProblems])

  const allFindings = useMemo(() => {
    if (findings === null) return null
    return [...findings, ...syntheticHealthFindings]
  }, [findings, syntheticHealthFindings])

  const active = allFindings?.filter((f) => !f.ignored) ?? []
  const missingFolderFindings = active.filter(isMissingFolderFinding)

  function handleManageInSites() {
    openSitesFilter('missing')
    onNavigate?.('sites')
  }

  async function fixAll() {
    const res = await runCommand({ type: 'apply_repair', project_id: null, ids: [], confirm_destructive: false })
    if (res.type === 'repair_report') {
      const failed = res.report.steps.filter((s) => !s.ok)
      notify.batch({
        operation: 'issues repaired',
        total: res.report.steps.length,
        succeeded: res.report.fixed,
        failed: failed.length,
        failedItems: failed.map((s) => `${s.label}: ${s.detail}`),
      })
    }
    await scan()
    if (onRefresh) {
      await onRefresh().catch(() => undefined)
    }
  }

  // 1. Checking state: reserved 40px height, spinner + checking text
  if (findings === null) {
    return (
      <div className="h-10 min-h-[40px] max-h-[44px] flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/60 px-3.5 shadow-xs transition-colors w-full min-w-0">
        <div className="flex items-center gap-2 min-w-0">
          <Spinner className="size-4 shrink-0 text-muted-foreground animate-spin" />
          <span className="text-xs font-medium text-muted-foreground">Checking diagnostics…</span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs text-muted-foreground"
          disabled
        >
          <RefreshCw className="size-3" />
        </Button>
      </div>
    )
  }

  // 2. Clean state (no issues): reserved 40px height, green check + no problems found
  if (active.length === 0) {
    return (
      <div className="h-10 min-h-[40px] max-h-[44px] flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-card/60 px-3.5 shadow-xs transition-colors w-full min-w-0">
        <div className="flex items-center gap-2 min-w-0">
          <CheckCircle2 className="size-4 shrink-0 text-success" />
          <span className="text-xs font-medium text-foreground">No problems found</span>
          <span className="hidden text-xs text-muted-foreground sm:inline truncate">
            {busy === 'scan'
              ? '· Checking diagnostics…'
              : `· Checked ${formatCheckedRelative(lastChecked, now)} · All services and directories are operational`}
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground shrink-0"
          disabled={busy !== null}
          onClick={handleManualCheck}
          title="Check again"
        >
          {busy === 'scan' ? <Spinner className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
          <span className="ml-1 hidden sm:inline">{busy === 'scan' ? 'Checking…' : 'Check again'}</span>
        </Button>
      </div>
    )
  }

  // 3. Issues found: reserved 40px height, amber/red banner with count, short list and Fix all
  const hasErrors = active.some((f) => f.severity === 'error')
  const canFixAll = active.some((f) => f.auto_fixable)

  return (
    <div
      className={cn(
        'h-10 min-h-[40px] max-h-[44px] flex items-center justify-between gap-3 rounded-lg border px-3.5 shadow-xs transition-colors w-full min-w-0',
        hasErrors
          ? 'border-destructive/40 bg-destructive/10 text-destructive-foreground'
          : 'border-warning/40 bg-warning/10 text-foreground'
      )}
    >
      <div className="flex items-center gap-2.5 min-w-0 flex-1">
        {hasErrors ? (
          <XCircle className="size-4 shrink-0 text-destructive" />
        ) : (
          <AlertTriangle className="size-4 shrink-0 text-warning" />
        )}
        <Badge
          variant="outline"
          className={cn(
            'shrink-0 text-xs font-semibold px-2 py-0',
            hasErrors ? 'border-destructive/50 text-destructive' : 'border-warning/50 text-warning'
          )}
        >
          {active.length} {active.length === 1 ? 'issue' : 'issues'}
        </Badge>
        <span
          className="text-xs font-medium text-foreground/90 truncate min-w-0"
          title={active.map((f) => f.problem).join(' · ')}
        >
          {active.map((f) => f.problem).slice(0, 2).join(' · ')}
          {active.length > 2 ? ` (+${active.length - 2} more)` : ''}
        </span>
      </div>

      <div className="flex items-center gap-1.5 shrink-0">
        {missingFolderFindings.length > 0 && onNavigate && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs text-foreground/80 hover:text-foreground"
            onClick={handleManageInSites}
            title="Open Sites page"
          >
            Manage in Sites
          </Button>
        )}
        {canFixAll && (
          <Button
            size="sm"
            variant="secondary"
            className={cn(
              'h-7 px-2.5 text-xs font-medium gap-1.5',
              hasErrors
                ? 'bg-destructive/20 hover:bg-destructive/30 text-destructive border border-destructive/30'
                : 'bg-warning/20 hover:bg-warning/30 text-foreground border border-warning/30'
            )}
            disabled={busy !== null}
            onClick={() => run('fix-all', fixAll)}
            title="Automatically repair fixable problems"
          >
            {busy === 'fix-all' ? <Spinner className="size-3 animate-spin" /> : <Wrench className="size-3" />}
            Fix all
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          className="h-7 px-2 text-xs text-muted-foreground hover:text-foreground"
          disabled={busy !== null}
          onClick={handleManualCheck}
          title="Check again"
        >
          {busy === 'scan' ? <Spinner className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
        </Button>
      </div>
    </div>
  )
}
