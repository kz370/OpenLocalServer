import { AlertTriangle, Check, Info, Stethoscope, Wrench, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { explainFinding } from '@/components/DiagnosticsCard'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type DoctorReport, type RepairReport, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { confirmAction } from '@/lib/confirm'

const MARK = {
  ok: { icon: Check, className: 'text-success' },
  warning: { icon: AlertTriangle, className: 'text-warning' },
  error: { icon: X, className: 'text-destructive' },
  info: { icon: Info, className: 'text-muted-foreground' },
}

/** §113 Environment Doctor, with §114's "repair what is safe" for the whole setup. */
export function DoctorDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [report, setReport] = useState<DoctorReport | null>(null)
  const [repair, setRepair] = useState<RepairReport | null>(null)
  const { busy, error, setError, run } = useAction()

  const check = useCallback(
    () =>
      run('check', async () => {
        const r = await runCommand({ type: 'doctor' })
        if (r.type === 'doctor_report') setReport(r.report)
      }),
    [run],
  )
  useEffect(() => {
    if (open) {
      setRepair(null)
      void check()
    }
  }, [open, check])

  const fixable = report?.findings.filter((f) => f.auto_fixable && !f.ignored).length ?? 0

  return (
    <Dialog
      open={open}
      onClose={() => busy === null && onClose()}
      title="Doctor"
      description="Everything OpenLocalServer depends on, checked in one go (also `ols doctor`)."
      wide
      footer={
        <>
          <Button variant="ghost" onClick={check} disabled={busy !== null}>
            {busy === 'check' ? <Spinner /> : <Stethoscope />} Check again
          </Button>
          <Button
            disabled={busy !== null || fixable === 0}
            onClick={() =>
              run('repair', async () => {
                const r = await runCommand({ type: 'apply_repair', project_id: null, ids: [], confirm_destructive: false })
                if (r.type === 'repair_report') setRepair(r.report)
                const d = await runCommand({ type: 'doctor' })
                if (d.type === 'doctor_report') setReport(d.report)
              })
            }
          >
            {busy === 'repair' ? <Spinner /> : <Wrench />} Fix all safe issues {fixable > 0 && `(${fixable})`}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {!report && <Spinner />}
        {report && (
          <>
            <div className="grid gap-x-6 gap-y-1 sm:grid-cols-2">
              {report.checks.map((c) => {
                const m = MARK[c.status]
                return (
                  <div key={c.label} className="flex items-start gap-2 text-sm">
                    <m.icon className={cn('mt-0.5 size-4 shrink-0', m.className)} />
                    <span className="min-w-0">
                      {c.label}
                      {c.detail && <span className="block truncate text-xs text-muted-foreground" title={c.detail}>{c.detail}</span>}
                    </span>
                  </div>
                )
              })}
            </div>
            {repair && (
              <div className="rounded-lg border border-border p-3 text-sm">
                <p className="font-medium">{repair.fixed} issue(s) fixed</p>
                {repair.steps.map((s, i) => (
                  <p key={i} className={cn('text-xs', s.ok ? 'text-success' : 'text-destructive')}>
                    {s.ok ? '✓' : '✗'} {s.label} {!s.ok && `(${s.detail})`}
                  </p>
                ))}
              </div>
            )}
            {report.findings.length > 0 && (
              <div className="flex flex-col gap-2">
                <p className="text-sm font-medium">
                  {report.errors} error(s), {report.warnings} warning(s)
                </p>
                {report.findings.map((f) => {
                  const m = MARK[f.severity === 'error' ? 'error' : f.severity === 'warning' ? 'warning' : 'info']
                  return (
                    <div key={f.id} className="flex gap-2 rounded-lg border border-border p-2.5 text-sm">
                      <m.icon className={cn('mt-0.5 size-4 shrink-0', m.className)} />
                      <div className="min-w-0">
                        <p className="flex items-center gap-2 font-medium">
                          {f.problem} <AiButton ask={() => explainFinding(f)} />
                        </p>
                        <p className="text-xs text-muted-foreground">{f.cause}</p>
                        <p className="text-xs">
                          {f.fix}
                          {f.fix_command && <span className="text-success"> ({f.auto_fixable ? 'safe to fix' : 'confirmation required'})</span>}
                        </p>
                      </div>
                      {f.fix_command && !f.ignored && (
                        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => run(`fix:${f.id}`, async () => {
                          if (!f.auto_fixable && !(await confirmAction('This fix may replace or remove something. Apply it?'))) return
                          const fixed = await runCommand({ type: 'apply_repair', project_id: null, ids: [f.id], confirm_destructive: !f.auto_fixable })
                          if (fixed.type === 'repair_report') setRepair(fixed.report)
                          const refreshed = await runCommand({ type: 'doctor' })
                          if (refreshed.type === 'doctor_report') setReport(refreshed.report)
                        })}><Wrench /> Fix</Button>
                      )}
                    </div>
                  )
                })}
              </div>
            )}
          </>
        )}
      </div>
    </Dialog>
  )
}
