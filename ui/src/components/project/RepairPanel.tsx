import { AlertTriangle, Check, Info, Stethoscope, Wrench, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { type RepairPlan, type RepairReport, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const ICON = { error: X, warning: AlertTriangle, info: Info }

/** §112–115: diagnose the project, explain each issue, then apply the chosen fixes and check again. */
export function RepairPanel({ projectId }: { projectId: string }) {
  const [plan, setPlan] = useState<RepairPlan | null>(null)
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [report, setReport] = useState<RepairReport | null>(null)
  const { busy, error, setError, run } = useAction()

  const diagnose = useCallback(
    () =>
      run('diagnose', async () => {
        const r = await runCommand({ type: 'plan_repair', project_id: projectId })
        if (r.type === 'repair_plan') {
          setPlan(r.plan)
          setChosen(new Set(r.plan.actions.filter((a) => !a.destructive).map((a) => a.finding_id)))
        }
      }),
    [projectId, run],
  )
  useEffect(() => {
    void diagnose()
  }, [diagnose])

  async function repair() {
    if (!plan) return
    const picked = plan.actions.filter((a) => chosen.has(a.finding_id))
    const destructive = picked.filter((a) => a.destructive)
    let confirmDestructive = false
    if (destructive.length > 0) {
      confirmDestructive = await confirmAction(`These fixes replace or remove something:\n\n${destructive.map((d) => `• ${d.label}`).join('\n')}\n\nApply them too?`, 'Destructive fixes')
      if (!confirmDestructive && destructive.length === picked.length) return
    }
    await run('repair', async () => {
      const r = await runCommand({ type: 'apply_repair', project_id: projectId, ids: [...chosen], confirm_destructive: confirmDestructive })
      if (r.type === 'repair_report') setReport(r.report)
      const p = await runCommand({ type: 'plan_repair', project_id: projectId })
      if (p.type === 'repair_plan') {
        setPlan(p.plan)
        setChosen(new Set(p.plan.actions.filter((a) => !a.destructive).map((a) => a.finding_id)))
      }
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">Checks the project and everything it relies on, then fixes what it safely can.</p>
        <div className="flex gap-2">
          <Button size="sm" variant="secondary" onClick={diagnose} disabled={busy !== null}>
            {busy === 'diagnose' ? <Spinner /> : <Stethoscope />} Diagnose
          </Button>
          <Button size="sm" onClick={repair} disabled={busy !== null || chosen.size === 0}>
            {busy === 'repair' ? <Spinner /> : <Wrench />} Repair {chosen.size > 0 && `(${chosen.size})`}
          </Button>
        </div>
      </div>
      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {report && (
        <div className="rounded-lg border border-border p-3 text-sm">
          <p className="font-medium">
            {report.fixed} issue{report.fixed === 1 ? '' : 's'} fixed · {report.after.length} left
          </p>
          {report.steps.map((s, i) => (
            <p key={i} className={cn('text-xs', s.ok ? 'text-success' : 'text-destructive')}>
              {s.ok ? '✓' : '✗'} {s.label} {s.detail && s.detail !== 'done' && <span className="text-muted-foreground">({s.detail})</span>}
            </p>
          ))}
        </div>
      )}

      {plan && plan.findings.length === 0 && (
        <p className="flex items-center gap-2 text-sm text-success">
          <Check className="size-4" /> Nothing wrong found.
        </p>
      )}

      {plan?.findings.map((f) => {
        const action = plan.actions.find((a) => a.finding_id === f.id)
        const Icon = ICON[f.severity]
        return (
          <div key={f.id} className="flex gap-3 rounded-lg border border-border p-3">
            <Icon className={cn('mt-0.5 size-4 shrink-0', f.severity === 'error' ? 'text-destructive' : f.severity === 'warning' ? 'text-warning' : 'text-muted-foreground')} />
            <div className="min-w-0 flex-1 text-sm">
              <p className="font-medium">{f.problem}</p>
              <p className="text-muted-foreground">
                <span className="font-medium text-foreground">Cause:</span> {f.cause}
              </p>
              <p>
                <span className="font-medium">Fix:</span> {f.fix}
              </p>
              {f.details.length > 0 && (
                <details className="mt-1 text-xs text-muted-foreground">
                  <summary className="cursor-pointer">Details</summary>
                  {f.details.map((d) => (
                    <p key={d} className="break-all">
                      {d}
                    </p>
                  ))}
                </details>
              )}
            </div>
            <div className="flex shrink-0 flex-col items-end gap-1">
              {action ? (
                <label className="flex cursor-pointer items-center gap-1.5 text-xs">
                  <input
                    type="checkbox"
                    className="size-4 accent-[var(--primary)]"
                    checked={chosen.has(f.id)}
                    onChange={(e) => {
                      const next = new Set(chosen)
                      if (e.target.checked) next.add(f.id)
                      else next.delete(f.id)
                      setChosen(next)
                    }}
                  />
                  Fix
                </label>
              ) : (
                <Badge variant="outline">manual</Badge>
              )}
              {action?.destructive && <Badge variant="warning">destructive</Badge>}
              <Button
                size="sm"
                variant="ghost"
                className="h-6 text-xs"
                onClick={() =>
                  run(`ignore:${f.id}`, async () => {
                    await runCommand({ type: 'ignore_diagnostic', id: f.id, ignore: true })
                    await diagnose()
                  })
                }
              >
                Ignore
              </Button>
            </div>
          </div>
        )
      })}
    </div>
  )
}
