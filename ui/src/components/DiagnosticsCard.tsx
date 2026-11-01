import { AlertTriangle, CheckCircle2, RefreshCw, Wrench, XCircle } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { Spinner } from '@/components/Spinner'

import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { type Finding, type HealthItem, runCommand } from '@/core'
import { useAction, usePoll } from '@/lib/hooks'
import { openSitesFilter } from '@/lib/nav'
import { cn } from '@/lib/utils'

/**
 * Diagnostics banner: strictly one slim row (<= 44px) in all states so layout never jumps.
 * - checking: spinner + "Checking diagnostics…"
 * - no issues: green check + "No problems found" + "Check again"
 * - issues: amber/red banner with count, short list and "Fix all"
 */
export function DiagnosticsCard({
  refreshToken = 0,
  onNavigate,
  healthProblems = [],
}: {
  refreshToken?: number
  onNavigate?: (p: Page) => void
  healthProblems?: HealthItem[]
}) {
  const [findings, setFindings] = useState<Finding[] | null>(null)
  const { busy, run } = useAction()

  const scan = useCallback(async () => {
    const res = await runCommand({ type: 'run_diagnostics' })
    if (res.type === 'diagnostics') setFindings(res.findings)
  }, [])

  useEffect(() => {
    scan().catch(() => setFindings([]))
  }, [scan, refreshToken])

  // Poll diagnostics periodically to stay in sync with service operations
  usePoll(() => scan().catch(() => undefined), 8000)

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
    await runCommand({ type: 'apply_repair', project_id: null, ids: [], confirm_destructive: false })
    await scan()
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
            · All services and directories are operational
          </span>
        </div>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs text-muted-foreground hover:text-foreground shrink-0"
          disabled={busy !== null}
          onClick={() => run('scan', scan)}
          title="Check again"
        >
          {busy === 'scan' ? <Spinner className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
          <span className="ml-1 hidden sm:inline">Check again</span>
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
          onClick={() => run('scan', scan)}
          title="Check again"
        >
          {busy === 'scan' ? <Spinner className="size-3 animate-spin" /> : <RefreshCw className="size-3" />}
        </Button>
      </div>
    </div>
  )
}
