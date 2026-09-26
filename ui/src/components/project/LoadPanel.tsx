import { Activity, Braces, Eye, EyeOff, FileCode2, Flame, Play, Plus, SlidersHorizontal, Timer, Trash2, TrendingUp, X, Zap } from 'lucide-react'
import { type ReactNode, useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { StopIcon } from '@/components/StopIcon'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Select, Textarea } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type LoadOverview, type LoadProfile, type LoadRun, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction, usePoll } from '@/lib/hooks'
import { cn } from '@/lib/utils'

const STATE: Record<string, { label: string; variant: 'success' | 'destructive' | 'warning' | 'secondary' | 'outline' }> = {
  running: { label: 'running', variant: 'secondary' },
  passed: { label: 'passed', variant: 'success' },
  failed: { label: 'thresholds failed', variant: 'destructive' },
  error: { label: 'error', variant: 'destructive' },
  stopped: { label: 'stopped', variant: 'warning' },
}

const ICONS: Record<string, ReactNode> = {
  smoke: <Flame />,
  load: <Activity />,
  stress: <TrendingUp />,
  spike: <Zap />,
  soak: <Timer />,
  custom: <SlidersHorizontal />,
}

const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD']

const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(2)} s` : `${n.toFixed(0)} ms`)
const peakOf = (p: LoadProfile) => Math.max(0, ...p.stages.map((s) => s.target))
const totalOf = (p: LoadProfile) => p.stages.reduce((a, s) => a + s.duration_s, 0)
const nice = (s: number) => (s >= 3600 ? `${(s / 3600).toFixed(1)} h` : s >= 120 ? `${Math.round(s / 60)} min` : `${s} s`)
const num = (v: string): number | null => {
  const n = parseFloat(v)
  return Number.isFinite(n) && n >= 0 ? n : null
}

/** The shape of a test: users over time. */
function Shape({ profile, className }: { profile: LoadProfile; className?: string }) {
  const total = Math.max(1, totalOf(profile))
  const peak = Math.max(1, peakOf(profile))
  let t = 0
  const pts = ['0,38', ...profile.stages.map((s) => `${((t += s.duration_s) / total) * 100},${38 - (s.target / peak) * 34}`)].join(' ')
  return (
    <svg viewBox="0 0 100 40" preserveAspectRatio="none" className={cn('h-10 w-full', className)} aria-hidden>
      <polyline points={`${pts} 100,38`} fill="currentColor" fillOpacity="0.12" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

/** A label above a control; every control is the same height so rows line up. */
function L({ label, hint, children, className }: { label: string; hint?: string; children: ReactNode; className?: string }) {
  return (
    <label className={cn('flex min-w-0 flex-col gap-1.5 text-sm', className)}>
      <span className="text-xs font-medium text-muted-foreground">
        {label}
        {hint && <span className="font-normal"> · {hint}</span>}
      </span>
      {children}
    </label>
  )
}

function NumberInput({ value, onChange, min = 0, max, step = 1, placeholder, className }: { value: number | null; onChange: (v: number | null) => void; min?: number; max?: number; step?: number; placeholder?: string; className?: string }) {
  return (
    <Input
      type="number"
      inputMode="decimal"
      min={min}
      max={max}
      step={step}
      value={value ?? ''}
      placeholder={placeholder}
      className={cn('h-8 tabular-nums', className)}
      onChange={(e) => onChange(e.target.value === '' ? null : num(e.target.value))}
    />
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

function RunView({ run }: { run: LoadRun }) {
  const m = run.metrics
  const charts: [string, number[], string][] = [
    ['Requests per second', run.series.map((p) => p.rps), 'var(--color-primary, #14b8a6)'],
    ['p95 latency', run.series.map((p) => p.p95_ms), 'var(--color-warning, #f59e0b)'],
    ['Users', run.series.map((p) => p.vus), 'var(--color-muted-foreground, #64748b)'],
  ]
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-8">
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
        {charts.map(([title, values, color]) => (
          <div key={title}>
            <div className="text-xs text-muted-foreground">{title}</div>
            <Spark values={values} color={color} />
          </div>
        ))}
      </div>
      {run.message && <p className={run.state === 'failed' || run.state === 'error' ? 'text-sm text-destructive' : 'text-sm text-muted-foreground'}>{run.message}</p>}
      {run.output.length > 0 && run.state !== 'running' && <pre className="max-h-64 overflow-auto rounded-md bg-muted p-3 text-xs">{run.output.join('\n')}</pre>}
    </div>
  )
}

/** Stage 18: k6 load tests for the project's own sites, set up from a form. */
export function LoadPanel({ projectId }: { projectId: string }) {
  const [overview, setOverview] = useState<LoadOverview | null>(null)
  const [profiles, setProfiles] = useState<LoadProfile[]>([])
  const [draft, setDraft] = useState<LoadProfile | null>(null)
  const [site, setSite] = useState('')
  const [run, setRun] = useState<LoadRun | null>(null)
  const [runs, setRuns] = useState<LoadRun[]>([])
  const [compare, setCompare] = useState<string[]>([])
  const [script, setScript] = useState<string | null>(null)
  const [content, setContent] = useState('')
  const [dirty, setDirty] = useState(false)
  const [newScript, setNewScript] = useState<string | null>(null)
  const [bodyOpen, setBodyOpen] = useState<number[]>([])
  const [showSecrets, setShowSecrets] = useState(false)
  const { busy, error, setError, run: act } = useAction()

  const load = useCallback(async () => {
    const [o, r, p] = await Promise.all([
      runCommand({ type: 'load_overview', project_id: projectId }),
      runCommand({ type: 'load_runs', project_id: projectId }),
      runCommand({ type: 'load_list_profiles' }),
    ])
    if (o.type === 'load_overview') {
      setOverview(o.overview)
      setSite((s) => s || o.overview.sites.find((x) => !x.public)?.host || o.overview.sites[0]?.host || '')
    }
    if (r.type === 'load_runs') setRuns(r.runs)
    if (p.type === 'load_profiles') {
      setProfiles(p.profiles)
      setDraft((d) => d ?? structuredClone(p.profiles.find((x) => x.id === 'load') ?? p.profiles[0]))
    }
  }, [projectId])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  useEffect(() => {
    if (!script) return
    runCommand({ type: 'load_read_script', project_id: projectId, name: script })
      .then((r) => {
        if (r.type === 'text') {
          setContent(r.text)
          setDirty(false)
        }
      })
      .catch(setError)
  }, [script, projectId, setError])

  usePoll(async () => {
    if (run?.state !== 'running') return
    const r = await runCommand({ type: 'load_status', run_id: run.id }).catch(() => null)
    if (r?.type === 'load_run') {
      setRun(r.run)
      if (r.run.state !== 'running') void load()
    }
  }, 1000)

  const cap = overview?.max_vus ?? 200
  const running = run?.state === 'running'
  const chosen = overview?.sites.find((s) => s.host === site)
  const peak = draft ? peakOf(draft) : 0
  // Values go to k6 when the test runs; they are never written into a script.
  const env: [string, string][] = (draft?.variables ?? []).filter((v) => v.name.trim() && v.value).map((v) => [v.name.trim(), v.value])
  const sameAsBuiltin = draft ? profiles.some((p) => p.builtin && p.id === draft.id && p.name === draft.name) : false

  const change = (patch: Partial<LoadProfile>) => setDraft((d) => (d ? { ...d, ...patch } : d))
  const setStage = (i: number, patch: Partial<LoadProfile['stages'][number]>) => draft && change({ stages: draft.stages.map((s, j) => (j === i ? { ...s, ...patch } : s)) })
  const setRequest = (i: number, patch: Partial<LoadProfile['requests'][number]>) => draft && change({ requests: draft.requests.map((r, j) => (j === i ? { ...r, ...patch } : r)) })
  // Changing the peak scales every stage, so the shape stays the same.
  const setHeader = (i: number, patch: Partial<LoadProfile['headers'][number]>) => draft && change({ headers: draft.headers.map((h, j) => (j === i ? { ...h, ...patch } : h)) })
  const setVariable = (i: number, patch: Partial<LoadProfile['variables'][number]>) => draft && change({ variables: draft.variables.map((v, j) => (j === i ? { ...v, ...patch } : v)) })
  const setPeak = (n: number | null) => {
    if (!draft || n === null) return
    const now = Math.max(1, peakOf(draft))
    const to = Math.min(Math.max(1, Math.round(n)), cap)
    change({ stages: draft.stages.map((s) => ({ ...s, target: s.target === 0 ? 0 : Math.max(1, Math.round((s.target * to) / now)) })) })
  }

  async function confirmPublic(host: string) {
    return confirmAction(`${host} is a public tunnel address. The test sends real traffic through the tunnel provider, and anyone watching sees it.`, 'Test a public address?')
  }

  async function runTest() {
    if (!draft || !chosen) return
    if (chosen.public && !(await confirmPublic(chosen.host))) return
    await act('run', async () => {
      const name = await runCommand({ type: 'load_generate', project_id: projectId, profile: draft, name: null })
      if (name.type !== 'text') return
      const r = await runCommand({ type: 'load_run', project_id: projectId, script: name.text, target: chosen.host, confirm_public: chosen.public, env })
      if (r.type === 'load_run') setRun(r.run)
      await load()
    })
  }

  async function runScript() {
    if (!script || !chosen) return
    if (chosen.public && !(await confirmPublic(chosen.host))) return
    await act('runscript', async () => {
      if (dirty) await runCommand({ type: 'load_save_script', project_id: projectId, name: script, content })
      setDirty(false)
      const r = await runCommand({ type: 'load_run', project_id: projectId, script, target: chosen.host, confirm_public: chosen.public, env })
      if (r.type === 'load_run') setRun(r.run)
    })
  }

  if (!overview || !draft) return <Spinner />

  const picked = runs.filter((r) => compare.includes(r.id)).sort((x, y) => x.started_ms - y.started_ms)
  const [a, b] = picked
  const delta = (x: number, y: number, unit = '') => `${y - x >= 0 ? '+' : ''}${unit === 'ms' ? (y - x).toFixed(0) : (y - x).toFixed(2)}${unit ? ' ' + unit : ''}`
  const siteSelect = (
    <L label="Run against">
      <Select value={site} onChange={(e) => setSite(e.target.value)} className="h-8">
        {overview.sites.map((s) => (
          <option key={s.host} value={s.host}>
            {s.host}
            {s.public ? ' (public tunnel)' : ''}
          </option>
        ))}
      </Select>
    </L>
  )
  const stopButton = running ? (
    <Button className="h-8" variant="destructive" onClick={() => runCommand({ type: 'load_stop', run_id: run!.id }).then((r) => r.type === 'load_run' && setRun(r.run))}>
      <StopIcon /> Stop
    </Button>
  ) : null

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

      <section className="flex flex-col gap-2">
        <h4 className="text-sm font-semibold">Choose a test</h4>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 xl:grid-cols-5">
          {profiles.map((p) => {
            const selected = draft.id === p.id && draft.builtin === p.builtin
            return (
              <div key={p.id} className={cn('group relative rounded-md border px-2.5 py-2 transition-colors', selected ? 'border-primary bg-primary/5' : 'border-border hover:border-primary/40 hover:bg-accent/40')}>
                <button className="flex w-full flex-col gap-1 text-left" title={p.description || 'Your own test plan.'} onClick={() => setDraft(structuredClone(p))}>
                  <div className="flex items-center gap-1.5">
                    <span className="text-muted-foreground [&>svg]:size-3.5">{ICONS[p.icon] ?? ICONS.custom}</span>
                    <span className="truncate text-sm font-medium">{p.name}</span>
                  </div>
                  <Shape profile={p} className={cn('h-6', selected ? 'text-primary' : 'text-muted-foreground')} />
                  <div className="truncate text-[11px] text-muted-foreground">
                    {peakOf(p)} users · {nice(totalOf(p))} · {p.requests.length} path{p.requests.length === 1 ? '' : 's'}
                  </div>
                </button>
                {!p.builtin && (
                  <button
                    className="absolute right-1 top-1 rounded p-0.5 text-muted-foreground opacity-0 hover:bg-accent hover:text-destructive group-hover:opacity-100"
                    title="Delete this test"
                    onClick={async () => {
                      if (await confirmAction(`Delete the test "${p.name}"?`, 'Delete test')) await act('delp', async () => { const r = await runCommand({ type: 'load_delete_profile', id: p.id }); if (r.type === 'load_profiles') setProfiles(r.profiles) })
                    }}
                  >
                    <Trash2 className="size-3.5" />
                  </button>
                )}
              </div>
            )
          })}
        </div>
      </section>

      <section className="flex flex-col gap-3 rounded-lg border border-border p-3">
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-[minmax(0,2fr)_repeat(3,minmax(0,1fr))]">
          <L label="Name">
            <Input className="h-8" value={draft.name} onChange={(e) => change({ name: e.target.value })} />
          </L>
          <L label="Users at peak" hint={`max ${cap}`}>
            <NumberInput value={peak} min={1} max={cap} onChange={setPeak} />
          </L>
          <L label="Pause between rounds" hint="sec">
            <NumberInput value={draft.think_time_s} step={0.5} max={60} onChange={(v) => change({ think_time_s: v ?? 0 })} />
          </L>
          {siteSelect}
        </div>

        <div className="grid gap-4 lg:grid-cols-2">
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">Stages · ramp to each target</span>
              <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" disabled={draft.stages.length >= 20} onClick={() => change({ stages: [...draft.stages, { duration_s: 30, target: peak || 10 }] })}>
                <Plus /> Add
              </Button>
            </div>
            {draft.stages.map((s, i) => (
              <div key={i} className="grid grid-cols-[1rem_1fr_1fr_2rem] items-center gap-1.5">
                <span className="text-[11px] text-muted-foreground">{i + 1}</span>
                <div className="flex items-center gap-1.5">
                  <NumberInput value={s.duration_s} min={1} max={14400} onChange={(v) => setStage(i, { duration_s: Math.max(1, v ?? 1) })} />
                  <span className="text-[11px] text-muted-foreground">s</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <NumberInput value={s.target} max={cap} onChange={(v) => setStage(i, { target: Math.min(v ?? 0, cap) })} />
                  <span className="text-[11px] text-muted-foreground">users</span>
                </div>
                <Button size="icon" variant="ghost" className="size-8" disabled={draft.stages.length <= 1} title="Remove stage" onClick={() => change({ stages: draft.stages.filter((_, j) => j !== i) })}>
                  <X />
                </Button>
              </div>
            ))}
            <div className="flex items-center gap-2">
              <Shape profile={draft} className="h-8 flex-1 text-primary" />
              <span className="shrink-0 text-[11px] text-muted-foreground">
                {nice(totalOf(draft))} · up to {peak}
              </span>
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">Paths · each user visits them in order</span>
              <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" disabled={draft.requests.length >= 20} onClick={() => change({ requests: [...draft.requests, { method: 'GET', path: '/', body: null }] })}>
                <Plus /> Add
              </Button>
            </div>
            {draft.requests.map((r, i) => {
              const canBody = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(r.method)
              return (
                <div key={i} className="flex flex-col gap-1">
                  <div className="grid grid-cols-[5.5rem_1fr_2rem_2rem] items-center gap-1.5">
                    <Select className="h-8" value={r.method} onChange={(e) => setRequest(i, { method: e.target.value, body: ['POST', 'PUT', 'PATCH', 'DELETE'].includes(e.target.value) ? r.body : null })}>
                      {METHODS.map((m) => (
                        <option key={m}>{m}</option>
                      ))}
                    </Select>
                    <Input className="h-8 font-mono text-sm" value={r.path} placeholder="/checkout" onChange={(e) => setRequest(i, { path: e.target.value.startsWith('/') || e.target.value === '' ? e.target.value : `/${e.target.value}` })} />
                    <Button
                      size="icon"
                      variant={r.body ? 'secondary' : 'ghost'}
                      className="size-8"
                      disabled={!canBody}
                      title={canBody ? 'Request body' : 'Only POST, PUT, PATCH and DELETE carry a body'}
                      onClick={() => setBodyOpen(bodyOpen.includes(i) ? bodyOpen.filter((x) => x !== i) : [...bodyOpen, i])}
                    >
                      <Braces />
                    </Button>
                    <Button size="icon" variant="ghost" className="size-8" disabled={draft.requests.length <= 1} title="Remove path" onClick={() => { change({ requests: draft.requests.filter((_, j) => j !== i) }); setBodyOpen([]) }}>
                      <X />
                    </Button>
                  </div>
                  {canBody && (bodyOpen.includes(i) || !!r.body) && (
                    <Textarea rows={2} className="font-mono text-xs" placeholder={'{"email": "a@b.test", "token": "{{TOKEN}}"}'} value={r.body ?? ''} onChange={(e) => setRequest(i, { body: e.target.value || null })} />
                  )}
                </div>
              )
            })}
          </div>
        </div>

        <div className="grid gap-4 lg:grid-cols-2">
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">Headers · sent with every request</span>
              <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" disabled={draft.headers.length >= 30} onClick={() => change({ headers: [...draft.headers, { name: '', value: '' }] })}>
                <Plus /> Add
              </Button>
            </div>
            {draft.headers.length === 0 && <p className="text-[11px] text-muted-foreground">None. Add Authorization, Accept, a cookie…</p>}
            {draft.headers.map((h, i) => (
              <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)_2rem] items-center gap-1.5">
                <Input className="h-8 text-sm" value={h.name} placeholder="Authorization" onChange={(e) => setHeader(i, { name: e.target.value })} />
                <Input className="h-8 font-mono text-sm" value={h.value} placeholder="Bearer {{TOKEN}}" onChange={(e) => setHeader(i, { value: e.target.value })} />
                <Button size="icon" variant="ghost" className="size-8" title="Remove header" onClick={() => change({ headers: draft.headers.filter((_, j) => j !== i) })}>
                  <X />
                </Button>
              </div>
            ))}
          </div>

          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-muted-foreground">Variables · use as {'{{NAME}}'} in a header or body</span>
              <div className="flex items-center">
                <Button size="icon" variant="ghost" className="size-6" title={showSecrets ? 'Hide secret values' : 'Show secret values'} onClick={() => setShowSecrets(!showSecrets)}>
                  {showSecrets ? <EyeOff /> : <Eye />}
                </Button>
                <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" disabled={draft.variables.length >= 30} onClick={() => change({ variables: [...draft.variables, { name: '', value: '', secret: true }] })}>
                  <Plus /> Add
                </Button>
              </div>
            </div>
            {draft.variables.length === 0 && <p className="text-[11px] text-muted-foreground">None. Add TOKEN (a secret) to keep a token out of the script.</p>}
            {draft.variables.map((v, i) => (
              <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.6fr)_auto_2rem] items-center gap-1.5">
                <Input className="h-8 font-mono text-sm uppercase" value={v.name} placeholder="TOKEN" onChange={(e) => setVariable(i, { name: e.target.value.replace(/[^A-Za-z0-9_]/g, '').toUpperCase() })} />
                <Input className="h-8 font-mono text-sm" type={v.secret && !showSecrets ? 'password' : 'text'} value={v.value} placeholder="value" autoComplete="off" onChange={(e) => setVariable(i, { value: e.target.value })} />
                <label className="flex items-center gap-1 text-[11px] text-muted-foreground" title="Secret values are kept in the system keyring, not in the saved test or the script">
                  <input type="checkbox" checked={v.secret} onChange={(e) => setVariable(i, { secret: e.target.checked })} /> secret
                </label>
                <Button size="icon" variant="ghost" className="size-8" title="Remove variable" onClick={() => change({ variables: draft.variables.filter((_, j) => j !== i) })}>
                  <X />
                </Button>
              </div>
            ))}
            {draft.variables.length > 0 && <p className="text-[11px] text-muted-foreground">Handed to k6 when the test runs; never written into the script. Custom scripts read them as __ENV.NAME.</p>}
          </div>
        </div>

        <div className="flex flex-wrap items-end gap-3 border-t border-border pt-3">
          <L label="Pass if p95 under" hint="ms" className="w-28">
            <NumberInput value={draft.thresholds.p95_ms} min={1} placeholder="off" onChange={(v) => change({ thresholds: { ...draft.thresholds, p95_ms: v } })} />
          </L>
          <L label="p99 under" hint="ms" className="w-28">
            <NumberInput value={draft.thresholds.p99_ms} min={1} placeholder="off" onChange={(v) => change({ thresholds: { ...draft.thresholds, p99_ms: v } })} />
          </L>
          <L label="Failed under" hint="%" className="w-28">
            <NumberInput value={draft.thresholds.error_rate_pct} step={0.5} max={100} placeholder="off" onChange={(v) => change({ thresholds: { ...draft.thresholds, error_rate_pct: v } })} />
          </L>
          <div className="ml-auto flex flex-wrap items-center gap-2">
            <Button
              variant="secondary"
              className="h-8"
              disabled={busy !== null || sameAsBuiltin || !draft.name.trim()}
              title={sameAsBuiltin ? 'Change the name to save your own copy' : 'Keep this test in your list'}
              onClick={() =>
                act('savep', async () => {
                  const r = await runCommand({ type: 'load_save_profile', profile: { ...draft, builtin: false, id: draft.builtin ? '' : draft.id } })
                  if (r.type !== 'load_profiles') return
                  setProfiles(r.profiles)
                  const mine = r.profiles.find((p) => !p.builtin && p.name === draft.name)
                  if (mine) setDraft(structuredClone(mine))
                })
              }
            >
              Save as my test
            </Button>
            {stopButton}
            {!running && (
              <Button className="h-8" disabled={busy !== null || !overview.k6.installed || !chosen} onClick={runTest}>
                {busy === 'run' ? <Spinner /> : <Play />} Run test
              </Button>
            )}
          </div>
        </div>
        <p className="-mt-1 text-[11px] text-muted-foreground">
          Leave a limit empty to skip that check.{sameAsBuiltin ? ' Change the name to save your own copy of this ready-made test.' : ''}
        </p>
      </section>

      {run && (
        <section className="flex flex-col gap-2 rounded-lg border border-border p-3">
          <div className="flex items-center gap-2 text-sm">
            <Badge variant={STATE[run.state]?.variant ?? 'outline'}>{STATE[run.state]?.label ?? run.state}</Badge>
            <span className="text-muted-foreground">
              {run.script} on {run.target}
            </span>
          </div>
          <RunView run={run} />
        </section>
      )}

      <section className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <h4 className="text-sm font-semibold">Your scripts</h4>
            <p className="text-xs text-muted-foreground">Any k6 script in this project's .openlocalserver/k6 folder. Tests above write one there; you can also start a new one from the form and edit the code.</p>
          </div>
          <Button size="sm" variant="secondary" onClick={() => setNewScript(draft.name.trim() ? draft.name : 'my-test')}>
            <Plus /> New script
          </Button>
        </div>
        {overview.scripts.length === 0 && <p className="text-sm text-muted-foreground">No scripts yet.</p>}
        <div className="grid gap-4 lg:grid-cols-[14rem_1fr]">
          <div className="flex flex-col gap-1">
            {overview.scripts.map((s) => (
              <button key={s.name} onClick={() => setScript(s.name)} className={cn('flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm', script === s.name ? 'bg-accent' : 'hover:bg-accent/60')}>
                <FileCode2 className="size-4 shrink-0 text-muted-foreground" /> <span className="truncate">{s.name}</span>
              </button>
            ))}
          </div>
          {script && (
            <div className="flex min-w-0 flex-col gap-3">
              <CodeEditor value={content} onChange={(v) => { setContent(v); setDirty(true) }} language="text" height="280px" />
              <div className="flex flex-wrap items-end justify-between gap-3">
                {siteSelect}
                <div className="flex flex-wrap items-center gap-2">
                  <Button variant="ghost" className="h-8" disabled={busy !== null || !dirty} onClick={() => act('save', async () => { await runCommand({ type: 'load_save_script', project_id: projectId, name: script, content }); setDirty(false) })}>
                    Save
                  </Button>
                  <Button variant="ghost" className="h-8" disabled={busy !== null} onClick={async () => { if (await confirmAction(`Delete ${script}?`, 'Delete script')) await act('del', async () => { await runCommand({ type: 'load_delete_script', project_id: projectId, name: script }); setScript(null); await load() }) }}>
                    <Trash2 /> Delete
                  </Button>
                  {stopButton}
                  {!running && (
                    <Button className="h-8" disabled={busy !== null || !overview.k6.installed || !chosen} onClick={runScript}>
                      {busy === 'runscript' ? <Spinner /> : <Play />} Run script
                    </Button>
                  )}
                </div>
              </div>
              <p className="text-xs text-muted-foreground">A script may only use this project's sites (use __ENV.BASE_URL) and up to {cap} users (Settings → Resources).</p>
            </div>
          )}
        </div>
      </section>

      {runs.length > 0 && (
        <section className="flex flex-col gap-2">
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
                      <input type="checkbox" title="Compare" checked={compare.includes(r.id)} onChange={(e) => setCompare(e.target.checked ? [...compare, r.id].slice(-2) : compare.filter((x) => x !== r.id))} />
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
                Change from {timeAgo(a.started_ms)} to {timeAgo(b.started_ms)}
              </div>
              <div className="grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-5">
                <span>p50 {delta(a.metrics.p50_ms, b.metrics.p50_ms, 'ms')}</span>
                <span>p95 {delta(a.metrics.p95_ms, b.metrics.p95_ms, 'ms')}</span>
                <span>p99 {delta(a.metrics.p99_ms, b.metrics.p99_ms, 'ms')}</span>
                <span>per second {delta(a.metrics.rps, b.metrics.rps)}</span>
                <span>errors {delta(a.metrics.error_rate * 100, b.metrics.error_rate * 100, '%')}</span>
              </div>
            </div>
          )}
        </section>
      )}

      <Dialog
        open={newScript !== null}
        onClose={() => setNewScript(null)}
        title="New script"
        description="It starts from the test above; edit the code freely afterwards."
        footer={
          <>
            <Button variant="ghost" onClick={() => setNewScript(null)}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null || !newScript?.trim()}
              onClick={() =>
                act('newscript', async () => {
                  const r = await runCommand({ type: 'load_generate', project_id: projectId, profile: draft, name: newScript })
                  setNewScript(null)
                  await load()
                  if (r.type === 'text') setScript(r.text)
                })
              }
            >
              Create
            </Button>
          </>
        }
      >
        <L label="Script name">
          <Input className="h-8" value={newScript ?? ''} onChange={(e) => setNewScript(e.target.value)} autoFocus />
        </L>
      </Dialog>
    </div>
  )
}
