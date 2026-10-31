import { AlertTriangle, ChevronDown, ChevronRight, ChevronUp, Eye, EyeOff, FolderOpen, FolderX, HardDrive, Info, RefreshCw, Wrench, XCircle } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type Finding, runCommand } from '@/core'
import { useAction, usePoll } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'
import { openSitesFilter } from '@/lib/nav'

const ICON = { error: XCircle, warning: AlertTriangle, info: Info }
const COLOR = { error: 'text-destructive', warning: 'text-warning', info: 'text-muted-foreground' }

/** §112: Problem / Cause / Fix, with [Fix], [Ignore] and [Details] on each finding. */
export function DiagnosticsCard({
  refreshToken = 0,
  onNavigate,
}: {
  refreshToken?: number
  onNavigate?: (p: Page) => void
}) {
  const [findings, setFindings] = useState<Finding[] | null>(null)
  const [open, setOpen] = useState<string | null>(null)
  const [showIgnored, setShowIgnored] = useState(false)
  const [missingExpanded, setMissingExpanded] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const scan = useCallback(async () => {
    const res = await runCommand({ type: 'run_diagnostics' })
    if (res.type === 'diagnostics') setFindings(res.findings)
  }, [])

  useEffect(() => {
    scan().catch(() => setFindings([]))
  }, [scan, refreshToken])

  // Dashboard polls get_dashboard every 3s; findings must follow too.
  // Without this, starting services clears health but leaves old diagnostics on screen.
  usePoll(() => scan().catch(() => undefined), 8000)

  const isMissingFolderFinding = (f: Finding) =>
    f.id.startsWith('site_root_missing:') || f.id.startsWith('project_missing:')

  const active = findings?.filter((f) => !f.ignored) ?? []
  const ignored = findings?.filter((f) => f.ignored) ?? []
  const shown = showIgnored ? findings ?? [] : active

  const missingFolderFindings = shown.filter(isMissingFolderFinding)
  const standardFindings = shown.filter((f) => !isMissingFolderFinding(f))

  const anyMissingActive = missingFolderFindings.some((f) => !f.ignored)

  const offlineCount = missingFolderFindings.filter(
    (f) =>
      f.problem.toLowerCase().includes('drive') ||
      f.cause.toLowerCase().includes('disconnected'),
  ).length
  const missingCount = missingFolderFindings.length - offlineCount

  const summaryTitle =
    offlineCount > 0 && missingCount > 0
      ? `${missingCount} folder${missingCount === 1 ? '' : 's'} missing, ${offlineCount} drive${offlineCount === 1 ? '' : 's'} disconnected`
      : offlineCount > 0
        ? `Drive disconnected for ${offlineCount} item${offlineCount === 1 ? '' : 's'}`
        : `${missingCount} item${missingCount === 1 ? '' : 's'} missing folder on disk`

  const summaryCause =
    offlineCount > 0 && missingCount > 0
      ? 'Folders were moved, renamed or deleted, or their storage drives are currently disconnected.'
      : offlineCount > 0
        ? 'The external drive or volume where these items are stored is currently disconnected or unavailable.'
        : 'Folders were moved, renamed or deleted from their registered locations.'

  const summaryFix =
    offlineCount > 0
      ? 'Reconnect the drive, or review and update paths in the Sites page.'
      : 'Restore the folders, or review and clean up from the Sites page.'

  function handleManageInSites() {
    openSitesFilter('missing')
    onNavigate?.('sites')
  }

  async function ignoreAllMissing(ignore: boolean) {
    const targets = missingFolderFindings.filter((f) => f.ignored !== ignore)
    if (targets.length === 0) return
    await run('ignore-all-missing', async () => {
      await Promise.all(
        targets.map((f) =>
          runCommand({ type: 'ignore_diagnostic', id: f.id, ignore }),
        ),
      )
      setNotice(
        ignore
          ? `Ignored ${targets.length} missing folder alert${targets.length === 1 ? '' : 's'}.`
          : `Restored ${targets.length} missing folder alert${targets.length === 1 ? '' : 's'}.`,
      )
      await scan()
    })
  }

  async function fix(f: Finding) {
    if (!f.fix_command) return
    if (!f.auto_fixable && !(await confirmAction('This fix may replace or remove something. Apply it?'))) return
    const res = await runCommand({ type: 'apply_repair', project_id: null, ids: [f.id], confirm_destructive: !f.auto_fixable })
    if (res.type === 'repair_report') setNotice(res.report.steps[0]?.ok ? `Applied: ${f.fix}` : res.report.steps[0]?.detail ?? 'Could not apply this fix.')
    await scan()
  }

  async function fixAll() {
    const res = await runCommand({ type: 'apply_repair', project_id: null, ids: [], confirm_destructive: false })
    if (res.type === 'repair_report') setNotice(`${res.report.fixed} issue(s) fixed. Destructive fixes still need confirmation.`)
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
        <div className="flex items-center gap-1">
          <Button size="sm" variant="secondary" disabled={busy !== null || !active.some((f) => f.auto_fixable)} onClick={() => run('fix-all', fixAll)}>
            {busy === 'fix-all' ? <Spinner className="size-3.5" /> : <Wrench />} Fix all
          </Button>
          <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => run('scan', scan)} title="Check again">
            {busy === 'scan' ? <Spinner className="size-3.5" /> : <RefreshCw className="size-3.5" />}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {notice && <p className="text-sm text-success">{notice}</p>}

        {missingFolderFindings.length > 0 && (
          <div className="rounded-lg border border-warning/40 bg-warning/5 p-3.5 transition-all">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="flex items-start gap-2.5 min-w-0 flex-1">
                {offlineCount > 0 && missingCount === 0 ? (
                  <HardDrive className="mt-0.5 size-4.5 shrink-0 text-warning" />
                ) : (
                  <FolderX className="mt-0.5 size-4.5 shrink-0 text-rose-500" />
                )}
                <div className="min-w-0 flex-1 text-sm">
                  <div className="flex flex-wrap items-center gap-2 font-medium text-foreground">
                    <span>{summaryTitle}</span>
                    <Badge variant="outline" className="border-warning/40 text-warning text-xs font-semibold">
                      {missingFolderFindings.length} affected
                    </Badge>
                    {!anyMissingActive && <Badge variant="secondary">all ignored</Badge>}
                  </div>
                  <div className="mt-0.5 text-xs text-muted-foreground">
                    <b className="font-medium text-foreground/80">Cause:</b> {summaryCause}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    <b className="font-medium text-foreground/80">Fix:</b> {summaryFix}
                  </div>
                </div>
              </div>

              <div className="flex w-full flex-wrap items-center gap-1.5 sm:w-auto sm:shrink-0">
                {onNavigate && (
                  <Button
                    size="sm"
                    variant="secondary"
                    className="h-8 gap-1.5 font-medium shadow-sm"
                    onClick={handleManageInSites}
                    title="Open the Sites page with the 'Folder not found' tab active"
                  >
                    <FolderOpen className="size-3.5 text-primary" /> Manage in Sites
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-8 gap-1.5 text-muted-foreground hover:text-foreground"
                  disabled={busy !== null}
                  onClick={() => void ignoreAllMissing(anyMissingActive)}
                  title={anyMissingActive ? 'Ignore all missing folder alerts' : 'Show all missing folder alerts'}
                >
                  {busy === 'ignore-all-missing' ? (
                    <Spinner className="size-3.5" />
                  ) : anyMissingActive ? (
                    <EyeOff className="size-3.5" />
                  ) : (
                    <Eye className="size-3.5" />
                  )}
                  {anyMissingActive ? 'Ignore all' : 'Unignore all'}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-8 gap-1"
                  onClick={() => setMissingExpanded((v) => !v)}
                  title={missingExpanded ? 'Hide items list' : 'Show all affected items'}
                >
                  {missingExpanded ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
                  {missingExpanded ? 'Hide' : `${missingFolderFindings.length} items`}
                </Button>
              </div>
            </div>

            {missingExpanded && (
              <div className="mt-3 flex flex-col gap-1.5 border-t border-border/60 pt-2.5">
                {missingFolderFindings.map((f) => {
                  const isOffline =
                    f.problem.toLowerCase().includes('drive') ||
                    f.cause.toLowerCase().includes('disconnected')
                  const path = f.details[0] || ''
                  const extraDetails = f.details.slice(1)
                  const isItemDetailOpen = open === f.id
                  return (
                    <div
                      key={f.id}
                      className={`flex flex-wrap items-center justify-between gap-2 rounded-md border border-border/50 bg-background/60 px-3 py-2 text-xs transition-colors hover:bg-muted/30 ${
                        f.ignored ? 'opacity-50' : ''
                      }`}
                    >
                      <div className="flex min-w-0 flex-1 items-start gap-2">
                        {isOffline ? (
                          <HardDrive className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
                        ) : (
                          <FolderX className="mt-0.5 size-3.5 shrink-0 text-rose-500" />
                        )}
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-1.5 font-medium">
                            <span className="truncate text-foreground">{f.problem}</span>
                            {isOffline ? (
                              <Badge variant="outline" className="border-amber-500/30 text-amber-600 dark:text-amber-400 py-0 text-[10px]">
                                Drive offline
                              </Badge>
                            ) : (
                              <Badge variant="outline" className="border-destructive/30 text-destructive py-0 text-[10px]">
                                Folder missing
                              </Badge>
                            )}
                            {f.ignored && <Badge variant="secondary" className="py-0 text-[10px]">ignored</Badge>}
                          </div>
                          {path && (
                            <div className="truncate font-mono text-[11px] text-muted-foreground/80" title={path}>
                              {path}
                            </div>
                          )}
                          {isItemDetailOpen && extraDetails.length > 0 && (
                            <ul className="mt-1 list-disc pl-4 font-mono text-[11px] text-muted-foreground">
                              {extraDetails.map((d, i) => (
                                <li key={i}>{d}</li>
                              ))}
                            </ul>
                          )}
                        </div>
                      </div>
                      <div className="flex items-center gap-1 shrink-0">
                        {extraDetails.length > 0 && (
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-6 px-1.5 text-[11px]"
                            onClick={() => setOpen(isItemDetailOpen ? null : f.id)}
                          >
                            {isItemDetailOpen ? 'Less' : 'Details'}
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-2 text-[11px]"
                          disabled={busy !== null}
                          onClick={() =>
                            run(`ignore:${f.id}`, async () => {
                              const res = await runCommand({ type: 'ignore_diagnostic', id: f.id, ignore: !f.ignored })
                              if (res.type === 'diagnostics') setFindings(res.findings)
                            })
                          }
                        >
                          {f.ignored ? 'Unignore' : 'Ignore'}
                        </Button>
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        )}

        {standardFindings.map((f) => {
          const Icon = ICON[f.severity]
          const expanded = open === f.id
          return (
            <div key={f.id} className={`rounded-lg border border-border p-3 ${f.ignored ? 'opacity-60' : ''}`}>
              <div className="flex flex-wrap items-start gap-2">
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
                <div className="flex w-full flex-wrap items-center gap-1 sm:w-auto sm:shrink-0">
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

