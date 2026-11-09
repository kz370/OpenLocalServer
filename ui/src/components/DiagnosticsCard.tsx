import { AlertTriangle, CheckCircle2, ChevronDown, RefreshCw, Wrench, XCircle } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'

import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { asDiagnostic } from '@/lib/diagnostic'
import { type Diagnostic, type Finding, type HealthItem, runCommand, runCommandWithin } from '@/core'
import {
  DIAGNOSTICS_POLL_INTERVAL_MS,
  buildDiagnosticsBarFindings,
  evaluateDiagnosticsScan,
  subscribeDiagnosticsRescan,
  useStartingServices,
} from '@/lib/diagnosticsNotify'
import { useAction, usePoll } from '@/lib/hooks'
import { openProjectsFilter, openSitesFilter } from '@/lib/nav'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

const DIAGNOSTICS_TIMEOUT_MS = 20_000

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
  const [scanError, setScanError] = useState<Diagnostic | null>(null)
  const [lastChecked, setLastChecked] = useState<number | null>(null)
  const [now, setNow] = useState<number>(() => Date.now())
  const [expanded, setExpanded] = useState(false)
  const { busy, run } = useAction()
  const startingServices = useStartingServices()

  const healthProblemsRef = useRef<HealthItem[]>(healthProblems)
  useEffect(() => {
    healthProblemsRef.current = healthProblems
  }, [healthProblems])

  // Timer ticker for relative time ("just now", "1m ago", etc.)
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 10000)
    return () => clearInterval(timer)
  }, [])

  const evaluateDiagnostics = useCallback(
    (newFindings: Finding[], currentHealth: HealthItem[]) => {
      evaluateDiagnosticsScan(newFindings, currentHealth)
      setLastChecked(Date.now())
    },
    [],
  )

  const scan = useCallback(async () => {
    try {
      const res = await runCommandWithin({ type: 'run_diagnostics' }, DIAGNOSTICS_TIMEOUT_MS)
      if (res.type === 'diagnostics') {
        setFindings(res.findings)
        setScanError(null)
        evaluateDiagnostics(res.findings, healthProblemsRef.current)
      }
    } catch (error) {
      setScanError(asDiagnostic(error))
      throw error
    }
  }, [evaluateDiagnostics])

  const lastRefreshTokenRef = useRef(refreshToken)
  useEffect(() => {
    if (lastRefreshTokenRef.current !== refreshToken) {
      lastRefreshTokenRef.current = refreshToken
      scan().catch(() => undefined)
    }
  }, [scan, refreshToken])

  useEffect(() => {
    return subscribeDiagnosticsRescan(() => {
      scan().catch(() => undefined)
    })
  }, [scan])

  // Poll diagnostics periodically to stay in sync with service operations
  usePoll(
    () => (scanError ? Promise.resolve() : scan().catch(() => undefined)),
    DIAGNOSTICS_POLL_INTERVAL_MS,
  )

  const handleManualCheck = useCallback(async () => {
    await run('scan', async () => {
      const startTime = Date.now()
      let resFindings: Finding[] = []
      let refreshedHealth: HealthItem[] | undefined

      try {
        const res = await runCommandWithin({ type: 'run_diagnostics' }, DIAGNOSTICS_TIMEOUT_MS)
        if (res.type === 'diagnostics') {
          resFindings = res.findings
          setFindings(res.findings)
          setScanError(null)
        }
        const refResult = onRefresh ? await onRefresh() : null
        if (refResult && typeof refResult === 'object' && 'health' in refResult) {
          refreshedHealth = (refResult as { health: HealthItem[] }).health
        }
      } catch (e) {
        setScanError(asDiagnostic(e))
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

      evaluateDiagnostics(resFindings, refreshedHealth ?? healthProblemsRef.current)
    })
  }, [run, onRefresh, evaluateDiagnostics])

  const isMissingFolderFinding = (f: Finding) =>
    f.id.startsWith('site_root_missing:') || f.id.startsWith('project_missing:')

  const allFindings = useMemo(() => {
    if (findings === null) return null
    return buildDiagnosticsBarFindings(findings, healthProblems, startingServices)
  }, [findings, healthProblems, startingServices])

  const active = allFindings?.filter((f) => !f.ignored) ?? []
  const missingFolderFindings = active.filter(isMissingFolderFinding)
  // project_missing = project not listed in Sites (no domain); site_root_missing = site row.
  const hasSiteMissing = missingFolderFindings.some((f) => f.id.startsWith('site_root_missing:'))
  const hasProjectMissing = missingFolderFindings.some((f) => f.id.startsWith('project_missing:'))

  function handleManageInSites() {
    openSitesFilter('missing')
    onNavigate?.('sites')
  }

  function handleManageInProjects() {
    openProjectsFilter('missing')
    onNavigate?.('projects')
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

  // A failed or stalled first read must leave the checking state and offer recovery.
  if (findings === null && scanError) {
    const errorDetails = [scanError.cause, scanError.fix].filter(Boolean).join('\nFix: ')
    return (
      <div className="h-10 min-h-[40px] max-h-[44px] flex items-center justify-between gap-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3.5 shadow-xs w-full min-w-0">
        <div className="flex items-center gap-2 min-w-0">
          <XCircle className="size-4 shrink-0 text-destructive" />
          <span className="text-xs font-medium text-destructive truncate" title={errorDetails}>
            {scanError.problem || 'Diagnostics could not be checked.'}
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs text-muted-foreground shrink-0"
          disabled={busy !== null}
          onClick={handleManualCheck}
          title={errorDetails || 'Retry diagnostics'}
        >
          {busy === 'scan' ? <Spinner className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
          <span className="ml-1 hidden sm:inline">{busy === 'scan' ? 'Checking…' : 'Retry'}</span>
        </Button>
      </div>
    )
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
        'rounded-lg border shadow-xs transition-colors w-full min-w-0',
        hasErrors
          ? 'border-destructive/40 bg-destructive/10 text-destructive-foreground'
          : 'border-warning/40 bg-warning/10 text-foreground'
      )}
    >
    <div className="h-10 min-h-[40px] max-h-[44px] flex items-center justify-between gap-3 px-3.5 w-full min-w-0">
      <button
        type="button"
        className="flex items-center gap-2.5 min-w-0 flex-1 cursor-pointer text-left"
        aria-expanded={expanded}
        title={expanded ? 'Hide the details' : 'Show what the problems are'}
        onClick={() => setExpanded((v) => !v)}
      >
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
        <ChevronDown className={cn('size-4 shrink-0 text-muted-foreground transition-transform', expanded && 'rotate-180')} />
      </button>

      <div className="flex items-center gap-1.5 shrink-0">
        {hasSiteMissing && onNavigate && (
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
        {hasProjectMissing && onNavigate && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 px-2 text-xs text-foreground/80 hover:text-foreground"
            onClick={handleManageInProjects}
            title="Open Projects page"
          >
            Manage in Projects
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
    {expanded && (
      <ul className="flex flex-col divide-y divide-border/50 border-t border-border/50 px-3.5 text-xs">
        {active.map((f) => (
          <li key={f.id} className="flex flex-col gap-0.5 py-2">
            <span className="flex items-center gap-2 font-medium text-foreground">
              {f.severity === 'error' ? <XCircle className="size-3.5 shrink-0 text-destructive" /> : <AlertTriangle className="size-3.5 shrink-0 text-warning" />}
              {f.problem}
            </span>
            {f.cause && <span className="pl-5.5 text-muted-foreground">Why: {f.cause}</span>}
            {f.fix && <span className="pl-5.5 text-muted-foreground">Fix: {f.fix}</span>}
            {f.details.length > 0 && (
              <span className="pl-5.5 font-mono text-[11px] text-muted-foreground break-all">{f.details.join(' · ')}</span>
            )}
          </li>
        ))}
      </ul>
    )}
    </div>
  )
}
