import { FileCode2, Play, Plus, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, Select } from '@/components/ui/form'
import { type LoadOverview, type LoadRun, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction, usePoll } from '@/lib/hooks'

const STATE: Record<string, { label: string; variant: 'success' | 'destructive' | 'warning' | 'secondary' | 'outline' }> = {
  running: { label: 'running', variant: 'secondary' },
  passed: { label: 'passed', variant: 'success' },
  failed: { label: 'thresholds failed', variant: 'destructive' },
  error: { label: 'error', variant: 'destructive' },
  stopped: { label: 'stopped', variant: 'warning' },
}

const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(2)} s` : `${n.toFixed(0)} ms`)

/** A tiny line chart of one series; no axes, the numbers are shown beside it. */
function Spark({ values, color }: { values: number[]; color: string }) {
  if (values.length < 2) return <div className="h-10 text-xs text-muted-foreground">collecting…</div>
  const max = Math.max(...values, 1)
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 100},${36 - (v / max) * 34}`).join(' ')
  return (
    <svg viewBox="0 0 100 38" preserveAspectRatio="none" className="h-10 w-full" role="img" aria-label="trend">
      <polyline points={pts} fill="none" stroke={color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="text-lg font-semibold tabular-nums">{value}</div>
    </div>
  )
}

function RunView({ run }: { run: LoadRun }) {
  const m = run.metrics
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-6">
        <Stat label="Requests" value={m.requests.toLocaleString()} />
        <Stat label="Per second" value={m.rps.toFixed(1)} />
        <Stat label="p50" value={ms(m.p50_ms)} />
        <Stat label="p95" value={ms(m.p95_ms)} />
        <Stat label="p99" value={ms(m.p99_ms)} />
        <Stat label="Errors" value={`${(m.error_rate * 100).toFixed(2)}%`} />
        <Stat label="Users" value={String(m.vus)} />
        <Stat label="Checks" value={`${m.checks_passed}/${m.checks_passed + m.checks_failed}`} />
      </div>
      <div className="grid gap-3 sm:grid-cols-3">
        <div>
          <div className="text-xs text-muted-foreground">Requests per second</div>
          <Spark values={run.series.map((p) => p.rps)} color="var(--color-primary, #14b8a6)" />
        </div>
        <div>
          <div className="text-xs text-muted-foreground">p95 latency</div>
          <Spark values={run.series.map((p) => p.p95_ms)} color="var(--color-warning, #f59e0b)" />
        </div>
        <div>
          <div className="text-xs text-muted-foreground">Users</div>
          <Spark values={run.series.map((p) => p.vus)} color="var(--color-muted-foreground, #64748b)" />
        </div>
      </div>
      {run.message && <p className={run.state === 'failed' || run.state === 'error' ? 'text-sm text-destructive' : 'text-sm text-muted-foreground'}>{run.message}</p>}
      {run.output.length > 0 && run.state !== 'running' && (
        <pre className="max-h-64 overflow-auto rounded-md bg-muted p-3 text-xs">{run.output.join('\n')}</pre>
      )}
    </div>
  )
}

/** Stage 18: k6 load tests for the project's own sites. */
export function LoadPanel({ projectId }: { projectId: string }) {
  const [overview, setOverview] = useState<LoadOverview | null>(null)
  const [script, setScript] = useState<string | null>(null)
  const [content, setContent] = useState('')
  const [dirty, setDirty] = useState(false)
  const [site, setSite] = useState('')
  const [run, setRun] = useState<LoadRun | null>(null)
  const [runs, setRuns] = useState<LoadRun[]>([])
  const [compare, setCompare] = useState<string[]>([])
  const { busy, error, setError, run: act } = useAction()

  const load = useCallback(async () => {
    const [o, r] = await Promise.all([runCommand({ type: 'load_overview', project_id: projectId }), runCommand({ type: 'load_runs', project_id: projectId })])
    if (o.type === 'load_overview') {
      setOverview(o.overview)
      setSite((s) => s || o.overview.sites.find((x) => !x.public)?.host || o.overview.sites[0]?.host || '')
      setScript((s) => s ?? o.overview.scripts[0]?.name ?? null)
    }
    if (r.type === 'load_runs') setRuns(r.runs)
  }, [projectId])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  useEffect(() => {
    if (!script) return
    runCommand({ type: 'load_read_script', project_id: projectId, name: script }).then((r) => {
      if (r.type === 'text') {
        setContent(r.text)
        setDirty(false)
      }
    }).catch(setError)
  }, [script, projectId, setError])

  // Follow a running test.
  usePoll(async () => {
    if (run?.state !== 'running') return
    const r = await runCommand({ type: 'load_status', run_id: run.id }).catch(() => null)
    if (r?.type === 'load_run') {
      setRun(r.run)
      if (r.run.state !== 'running') void load()
    }
  }, 1000)

  if (!overview) return <Spinner />
  const chosen = overview.sites.find((s) => s.host === site)
  const running = run?.state === 'running'

  async function start() {
    if (!script || !chosen) return
    if (chosen.public && !(await confirmAction(`${chosen.host} is a public tunnel address. The test sends real traffic through the tunnel provider, and anyone watching sees it.`, 'Test a public address?'))) return
    await act('run', async () => {
      if (dirty) await runCommand({ type: 'load_save_script', project_id: projectId, name: script, content })
      setDirty(false)
      const r = await runCommand({ type: 'load_run', project_id: projectId, script, target: chosen.host, confirm_public: chosen.public })
      if (r.type === 'load_run') setRun(r.run)
    })
  }

  async function generate(kind: 'smoke' | 'load' | 'spike') {
    await act('gen', async () => {
      const r = await runCommand({ type: 'load_generate', project_id: projectId, kind, paths: ['/'] })
      await load()
      if (r.type === 'text') setScript(r.text)
    })
  }

  const picked = runs.filter((r) => compare.includes(r.id)).sort((x, y) => x.started_ms - y.started_ms)
  const [a, b] = picked
  const delta = (x: number, y: number, unit = '') => {
    const d = y - x
    return `${d >= 0 ? '+' : ''}${unit === 'ms' ? d.toFixed(0) : d.toFixed(2)}${unit ? ' ' + unit : ''}`
  }

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex flex-wrap items-center gap-2">
        <TechIcon id="k6" className="size-5" />
        <h3 className="text-base font-semibold">Load testing</h3>
        {overview.k6.installed ? <Badge variant="outline">k6 {overview.k6.version?.split(' ')[1] ?? overview.k6.version}</Badge> : <Badge variant="warning">k6 not installed</Badge>}
      </div>
      {!overview.k6.installed && <p className="text-sm text-muted-foreground">Install k6 from the Runtimes page (or `ols runtime install k6`), then come back.</p>}
      {overview.sites.length === 0 && <p className="text-sm text-muted-foreground">This project has no site yet. Give it a domain on the Sites page; a load test only runs against the project's own sites.</p>}

      <div className="grid gap-4 lg:grid-cols-[14rem_1fr]">
        <div className="flex flex-col gap-1">
          {overview.scripts.map((s) => (
            <button key={s.name} onClick={() => setScript(s.name)} className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${script === s.name ? 'bg-accent' : 'hover:bg-accent/60'}`}>
              <FileCode2 className="size-4 text-muted-foreground" /> {s.name}
            </button>
          ))}
          <div className="mt-1 flex flex-wrap gap-1">
            {(['smoke', 'load', 'spike'] as const).map((k) => (
              <Button key={k} size="sm" variant="secondary" disabled={busy !== null} onClick={() => generate(k)} title={`Write a ${k} test from this project's site`}>
                <Plus /> {k}
              </Button>
            ))}
          </div>
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          {script ? (
            <>
              <CodeEditor value={content} onChange={(v) => { setContent(v); setDirty(true) }} language="text" height="300px" />
              <div className="flex flex-wrap items-end gap-2">
                <Field label="Run against">
                  <Select value={site} onChange={(e) => setSite(e.target.value)} className="w-64">
                    {overview.sites.map((s) => (
                      <option key={s.host} value={s.host}>
                        {s.host}
                        {s.public ? ' (public tunnel)' : ''}
                      </option>
                    ))}
                  </Select>
                </Field>
                {running ? (
                  <Button variant="destructive" onClick={() => runCommand({ type: 'load_stop', run_id: run!.id }).then((r) => r.type === 'load_run' && setRun(r.run))}>
                    <StopIcon /> Stop
                  </Button>
                ) : (
                  <Button disabled={busy !== null || !overview.k6.installed || !chosen} onClick={start}>
                    {busy === 'run' ? <Spinner /> : <Play />} Run
                  </Button>
                )}
                <Button
                  variant="ghost"
                  disabled={busy !== null || !dirty}
                  onClick={() => act('save', async () => { await runCommand({ type: 'load_save_script', project_id: projectId, name: script, content }); setDirty(false) })}
                >
                  Save
                </Button>
                <Button
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={async () => {
                    if (await confirmAction(`Delete ${script}?`, 'Delete script')) await act('del', async () => { await runCommand({ type: 'load_delete_script', project_id: projectId, name: script }); setScript(null); await load() })
                  }}
                >
                  <Trash2 /> Delete
                </Button>
                <span className="text-xs text-muted-foreground">Up to {overview.max_vus} virtual users (Settings → Resources).</span>
              </div>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">No script yet. Write a first one with smoke, load or spike; it uses the site's address and you can edit it freely.</p>
          )}
        </div>
      </div>

      {run && (
        <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
          <div className="flex items-center gap-2 text-sm">
            <Badge variant={STATE[run.state]?.variant ?? 'outline'}>{STATE[run.state]?.label ?? run.state}</Badge>
            <span className="text-muted-foreground">
              {run.script} on {run.target}
            </span>
          </div>
          <RunView run={run} />
        </div>
      )}

      {runs.length > 0 && (
        <div className="flex flex-col gap-2">
          <h4 className="text-sm font-semibold">Earlier runs</h4>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="text-left text-xs text-muted-foreground">
                <tr>
                  <th className="py-1 pr-2" />
                  <th className="pr-3">When</th>
                  <th className="pr-3">Script</th>
                  <th className="pr-3">Result</th>
                  <th className="pr-3">p95</th>
                  <th className="pr-3">Per second</th>
                  <th className="pr-3">Errors</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {runs.map((r) => (
                  <tr key={r.id} className="border-t border-border">
                    <td className="py-1 pr-2">
                      <input
                        type="checkbox"
                        title="Compare"
                        checked={compare.includes(r.id)}
                        onChange={(e) => setCompare(e.target.checked ? [...compare, r.id].slice(-2) : compare.filter((x) => x !== r.id))}
                      />
                    </td>
                    <td className="pr-3">
                      <button className="hover:underline" onClick={() => setRun(r)}>
                        {timeAgo(r.started_ms)}
                      </button>
                    </td>
                    <td className="pr-3">{r.script}</td>
                    <td className="pr-3">
                      <Badge variant={STATE[r.state]?.variant ?? 'outline'}>{STATE[r.state]?.label ?? r.state}</Badge>
                    </td>
                    <td className="pr-3 tabular-nums">{ms(r.metrics.p95_ms)}</td>
                    <td className="pr-3 tabular-nums">{r.metrics.rps.toFixed(1)}</td>
                    <td className="pr-3 tabular-nums">{(r.metrics.error_rate * 100).toFixed(2)}%</td>
                    <td>
                      <Button size="sm" variant="ghost" onClick={() => act('rm', async () => { await runCommand({ type: 'load_delete_run', project_id: projectId, run_id: r.id }); await load() })}>
                        <Trash2 />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {a && b && (
            <div className="rounded-md border border-border p-3 text-sm">
              <div className="mb-1 font-medium">
                Change from {timeAgo(a.started_ms)} to {timeAgo(b.started_ms)} (older run first)
              </div>
              <div className="grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-4">
                <span>p50 {delta(a.metrics.p50_ms, b.metrics.p50_ms, 'ms')}</span>
                <span>p95 {delta(a.metrics.p95_ms, b.metrics.p95_ms, 'ms')}</span>
                <span>p99 {delta(a.metrics.p99_ms, b.metrics.p99_ms, 'ms')}</span>
                <span>per second {delta(a.metrics.rps, b.metrics.rps)}</span>
                <span>errors {delta(a.metrics.error_rate * 100, b.metrics.error_rate * 100, '%')}</span>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
