import { AlertTriangle, Check, CircleDashed, FileCode2, Layers, ListChecks, Play, RotateCcw, Save, Sparkles, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type EnvironmentPlan, type ManifestInfo, type ModeResult, type ModesView, type Profile, type SetupReport, type SetupStepStatus as StepStatus, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const GROUPS: { id: string; title: string }[] = [
  { id: 'install', title: 'Install' },
  { id: 'create', title: 'Create' },
  { id: 'configure', title: 'Configure' },
  { id: 'start', title: 'Start' },
  { id: 'tunnel', title: 'Tunnel' },
  { id: 'check', title: 'Check' },
]

function StatusIcon({ status }: { status: StepStatus | 'planned' | 'present' }) {
  switch (status) {
    case 'done':
    case 'skipped':
    case 'present':
      return <Check className="size-4 shrink-0 text-success" />
    case 'running':
      return <Spinner className="size-4 shrink-0 text-primary" />
    case 'failed':
      return <X className="size-4 shrink-0 text-destructive" />
    case 'rolled_back':
      return <RotateCcw className="size-4 shrink-0 text-warning" />
    default:
      return <CircleDashed className="size-4 shrink-0 text-muted-foreground" />
  }
}

/**
 * §69–78: the project's `.openlocalserver/` manifest, profiles and modes, and the setup that
 * builds the environment from it (plan → dry run → apply, with rollback on failure).
 */
export function EnvironmentPanel({ projectId }: { projectId: string }) {
  const [info, setInfo] = useState<ManifestInfo | null>(null)
  const [text, setText] = useState('')
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [profileId, setProfileId] = useState('')
  const [modes, setModes] = useState<ModesView | null>(null)
  const [modeResult, setModeResult] = useState<ModeResult | null>(null)
  const [plan, setPlan] = useState<EnvironmentPlan | null>(null)
  const [report, setReport] = useState<SetupReport | null>(null)
  const [editing, setEditing] = useState(false)
  const [saveAs, setSaveAs] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const [m, p, md] = await Promise.all([
      runCommand({ type: 'get_manifest', project_id: projectId }),
      runCommand({ type: 'list_profiles' }),
      runCommand({ type: 'get_project_modes', project_id: projectId }),
    ])
    if (m.type === 'manifest_info') {
      setInfo(m.info)
      setText(m.info.text ?? '')
    }
    if (p.type === 'profiles') setProfiles(p.profiles)
    if (md.type === 'modes') setModes(md.view)
  }, [projectId])

  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  // While setup runs (a blocking command), follow its steps.
  usePoll(async () => {
    if (busy !== 'apply') return
    const r = await runCommand({ type: 'get_setup_progress' })
    if (r.type === 'setup_progress' && r.report?.project_id === projectId) setReport(r.report)
  }, 700)

  const makePlan = () =>
    run('plan', async () => {
      setReport(null)
      const r = await runCommand({ type: 'plan_setup', project_id: projectId })
      if (r.type === 'setup_plan') setPlan(r.plan)
    })

  async function apply(dryRun: boolean) {
    if (!dryRun && !(await confirmAction('Set up this environment now? Every change is listed in the plan; a failure undoes the safe changes.', 'Apply the plan'))) return
    await run(dryRun ? 'dry' : 'apply', async () => {
      const r = await runCommand({ type: 'apply_setup', project_id: projectId, dry_run: dryRun })
      if (r.type === 'setup') setReport(r.report)
      if (!dryRun) {
        await load()
        const p = await runCommand({ type: 'plan_setup', project_id: projectId })
        if (p.type === 'setup_plan') setPlan(p.plan)
      }
    })
  }

  const planStatus = (i: number): StepStatus | 'planned' | 'present' => {
    if (report && !report.dry_run && report.steps[i]) return report.steps[i].status
    return plan?.steps[i]?.done ? 'present' : 'planned'
  }

  return (
    <div className="flex flex-col gap-5">
      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {/* Manifest */}
      <section className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="min-w-0">
            <h3 className="flex items-center gap-2 text-sm font-medium">
              <FileCode2 className="size-4" /> Manifest
              {info?.found ? <Badge variant="success">.openlocalserver/environment.yaml</Badge> : <Badge variant="outline">none yet</Badge>}
              {info?.lock && <Badge variant="secondary">locked</Badge>}
            </h3>
            <p className="text-xs text-muted-foreground">
              {info?.found
                ? 'Commit the .openlocalserver folder: `ols setup` rebuilds this environment on another computer.'
                : 'Without a manifest, setup uses what was detected in the project. Save it to make it explicit and shareable.'}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            {!info?.found && (
              <Button
                size="sm"
                variant="secondary"
                disabled={busy !== null}
                onClick={() =>
                  run('create', async () => {
                    await runCommand({ type: 'save_manifest', project_id: projectId, manifest: null })
                    await load()
                    setEditing(true)
                  })
                }
              >
                <Sparkles /> Create from what was detected
              </Button>
            )}
            {info?.found && (
              <Button size="sm" variant="ghost" onClick={() => setEditing(!editing)}>
                <FileCode2 /> {editing ? 'Close editor' : 'Edit'}
              </Button>
            )}
          </div>
        </div>
        {info?.error && <p className="rounded-md bg-destructive/10 p-2 text-xs text-destructive">{info.error}</p>}
        {editing && info?.found && (
          <div className="flex flex-col gap-2">
            <div className="overflow-hidden rounded-lg border border-border">
              <CodeEditor value={text} onChange={setText} language="yaml" height="320px" />
            </div>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={() => setText(info.text ?? '')} disabled={text === (info.text ?? '')}>
                Revert
              </Button>
              <Button
                size="sm"
                disabled={busy !== null || text === (info.text ?? '')}
                onClick={() =>
                  run('save', async () => {
                    await runCommand({ type: 'save_manifest_text', project_id: projectId, text })
                    await load()
                    setPlan(null)
                  })
                }
              >
                <Save /> Save manifest
              </Button>
            </div>
          </div>
        )}
        {info?.lock && (
          <p className="text-xs text-muted-foreground">
            Locked versions:{' '}
            {Object.entries(info.lock)
              .map(([k, v]) => `${k} ${v}`)
              .join(' · ')}
          </p>
        )}
      </section>

      {/* Profiles */}
      <section className="flex flex-col gap-2 rounded-lg border border-border p-3">
        <h3 className="flex items-center gap-2 text-sm font-medium">
          <Layers className="size-4" /> Profile
          {info?.manifest?.profile && <Badge variant="secondary">{profiles.find((p) => p.id === info.manifest?.profile)?.name ?? info.manifest.profile}</Badge>}
        </h3>
        <p className="text-xs text-muted-foreground">Start from a ready-made environment. It writes the manifest (keeping this project's site and database names); nothing is installed until you apply the plan.</p>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={profileId} onChange={(e) => setProfileId(e.target.value)} className="min-w-0 flex-1 sm:max-w-72" aria-label="Profile">
            <option value="">Choose a profile…</option>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
          <Button
            size="sm"
            variant="secondary"
            disabled={!profileId || busy !== null}
            onClick={async () => {
              if (info?.found && !(await confirmAction('Replace this project\'s manifest with the profile? Its own modes are kept.', 'Apply profile'))) return
              await run('profile', async () => {
                await runCommand({ type: 'apply_profile', project_id: projectId, profile_id: profileId })
                await load()
                const p = await runCommand({ type: 'plan_setup', project_id: projectId })
                if (p.type === 'setup_plan') setPlan(p.plan)
              })
            }}
          >
            Use profile
          </Button>
          {saveAs === null ? (
            <Button size="sm" variant="ghost" onClick={() => setSaveAs('')}>
              Save this project as a profile
            </Button>
          ) : (
            <div className="flex items-center gap-2">
              <Input value={saveAs} onChange={(e) => setSaveAs(e.target.value)} placeholder="Profile name" className="h-8 w-44" autoFocus />
              <Button
                size="sm"
                disabled={!saveAs.trim() || busy !== null}
                onClick={() =>
                  run('saveas', async () => {
                    await runCommand({ type: 'profile_from_project', project_id: projectId, name: saveAs.trim() })
                    setSaveAs(null)
                    await load()
                  })
                }
              >
                Save
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setSaveAs(null)}>
                Cancel
              </Button>
            </div>
          )}
        </div>
      </section>

      {/* Modes */}
      {modes && (
        <section className="flex flex-col gap-2">
          <h3 className="text-sm font-medium">Mode</h3>
          <div className="flex flex-wrap gap-2">
            {modes.modes.map((m) => {
              const bits = [
                m.mode.xdebug === true && 'Xdebug on',
                m.mode.xdebug === false && 'Xdebug off',
                m.mode.workers === true && 'workers on',
                m.mode.workers === false && 'workers off',
                m.mode.scheduler === false && 'scheduler paused',
                ...(m.mode.services ?? []).map((s) => `start ${s}`),
                ...Object.entries(m.mode.env ?? {}).map(([k, v]) => `${k}=${v}`),
              ].filter(Boolean)
              return (
                <button
                  key={m.name}
                  disabled={busy !== null}
                  onClick={() =>
                    run(`mode:${m.name}`, async () => {
                      const r = await runCommand({ type: 'set_project_mode', project_id: projectId, mode: m.name })
                      if (r.type === 'mode_result') setModeResult(r.result)
                      await load()
                    })
                  }
                  title={bits.join(', ')}
                  className={cn(
                    'flex min-w-32 flex-col items-start rounded-lg border px-3 py-2 text-left text-sm transition-colors',
                    modes.current === m.name ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:bg-accent/50',
                  )}
                >
                  <span className="flex items-center gap-1.5 font-medium capitalize">
                    {busy === `mode:${m.name}` && <Spinner className="size-3.5" />}
                    {m.name}
                    {m.custom && <Badge variant="secondary">custom</Badge>}
                  </span>
                  <span className="line-clamp-2 text-xs text-muted-foreground">{bits.join(' · ') || 'no changes'}</span>
                </button>
              )
            })}
          </div>
          {modeResult && (
            <div className="rounded-lg border border-border p-3 text-xs">
              <p className="font-medium capitalize">{modeResult.mode} mode</p>
              {modeResult.changes.length === 0 && modeResult.problems.length === 0 && <p className="text-muted-foreground">Everything was already set.</p>}
              {modeResult.changes.map((c) => (
                <p key={c} className="text-success">
                  ✓ {c}
                </p>
              ))}
              {modeResult.problems.map((p) => (
                <p key={p} className="text-warning">
                  ⚠ {p}
                </p>
              ))}
            </div>
          )}
        </section>
      )}

      {/* Setup */}
      <section className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            <ListChecks className="size-4" /> Environment setup
          </h3>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="secondary" onClick={makePlan} disabled={busy !== null}>
              {busy === 'plan' ? <Spinner /> : <ListChecks />} {plan ? 'Plan again' : 'Show the plan'}
            </Button>
            {plan && (
              <>
                <Button size="sm" variant="ghost" onClick={() => apply(true)} disabled={busy !== null} title="Check the plan without changing anything (ols setup --dry-run)">
                  Dry run
                </Button>
                <Button size="sm" onClick={() => apply(false)} disabled={busy !== null || !plan.ok || plan.steps.every((s) => s.done)}>
                  {busy === 'apply' ? <Spinner /> : <Play />} Apply
                </Button>
              </>
            )}
          </div>
        </div>

        {plan && (
          <div className="flex flex-col gap-3 rounded-lg border border-border p-3">
            {!plan.manifest_found && <p className="text-xs text-muted-foreground">Planned from what was detected (no manifest yet).</p>}
            {plan.conflicts.length > 0 && (
              <div className="flex flex-col gap-1.5">
                {plan.conflicts.map((c, i) => (
                  <div key={i} className={cn('flex gap-2 rounded-md p-2 text-xs', c.blocking ? 'bg-destructive/10' : 'bg-warning/10')}>
                    <AlertTriangle className={cn('mt-0.5 size-3.5 shrink-0', c.blocking ? 'text-destructive' : 'text-warning')} />
                    <div>
                      <p className={c.blocking ? 'text-destructive' : ''}>{c.message}</p>
                      {c.resolution && <p className="text-muted-foreground">{c.resolution}</p>}
                    </div>
                  </div>
                ))}
              </div>
            )}
            {GROUPS.map((g) => {
              const rows = plan.steps.map((s, i) => ({ s, i })).filter(({ s }) => s.group === g.id)
              if (rows.length === 0) return null
              return (
                <div key={g.id}>
                  <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">{g.title}</p>
                  <ul className="flex flex-col gap-1">
                    {rows.map(({ s, i }) => {
                      const st = planStatus(i)
                      const detail = report && !report.dry_run ? report.steps[i]?.detail : s.note
                      return (
                        <li key={i} className="flex items-start gap-2 text-sm">
                          <StatusIcon status={st} />
                          <span className="min-w-0">
                            <span className={cn(st === 'present' && 'text-muted-foreground', st === 'failed' && 'text-destructive', st === 'rolled_back' && 'line-through decoration-warning')}>{s.label}</span>
                            {detail && <span className="block text-xs text-muted-foreground">{detail}</span>}
                          </span>
                        </li>
                      )
                    })}
                  </ul>
                </div>
              )
            })}
          </div>
        )}

        {report && !report.running && (
          <div className={cn('rounded-lg border p-3 text-sm', report.dry_run ? 'border-border' : report.ok ? 'border-success/40 bg-success/5' : 'border-destructive/40 bg-destructive/5')}>
            {report.dry_run ? (
              <p>Dry run: nothing was changed. {report.steps.filter((s) => s.status === 'pending').length} step(s) would run.</p>
            ) : report.ok ? (
              <p className="font-medium text-success">The environment is set up.</p>
            ) : (
              <p className="font-medium text-destructive">{report.error}</p>
            )}
            {report.rolled_back.length > 0 && (
              <div className="mt-2 text-xs">
                <p className="font-medium">Undone after the failure:</p>
                {report.rolled_back.map((r) => (
                  <p key={r}>↺ {r}</p>
                ))}
              </div>
            )}
            {report.lock_written && <p className="mt-1 text-xs text-muted-foreground">Wrote {report.lock_written}</p>}
            {report.health && !report.health.ok && <p className="mt-1 text-xs text-warning">The site did not answer yet: check it on the Sites page.</p>}
          </div>
        )}
      </section>
    </div>
  )
}
