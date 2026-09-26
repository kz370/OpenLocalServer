import { open, save } from '@tauri-apps/plugin-dialog'
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  FolderSearch,
  Loader2,
  Pencil,
  Plus,
  Rocket,
  Star,
  Trash2,
  Upload,
  XCircle,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { TechIcon, TechTile } from '@/components/TechIcon'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import type { Page } from '@/components/layout/Sidebar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { Field, FormSection, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import {
  type CatalogEntry,
  type Diagnostic,
  type QuickEntryDetail,
  type QuickEntryView,
  type QuickPlanResult,
  type QuickVariable,
  type RunView,
  runCommand,
} from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmThen } from '@/lib/confirm'

const CATEGORIES = ['all', 'php', 'node', 'python', 'static', 'proxy', 'custom']
/** Built-in recipes with their own brand mark; anything else shows its category's. */
const BRANDED_APPS: Record<string, true> = Object.fromEntries(
  ['laravel', 'symfony', 'wordpress', 'plain-php', 'static-html', 'react-vite', 'vue-vite', 'nextjs', 'express-api', 'django', 'fastapi', 'custom-app'].map((id) => [id, true]),
)

const NEW_APP_YAML = `id: my-recipe
name: My Recipe
description: What this creates
category: custom

variables:
  - name: project_name
    label: Project name
    type: text
    required: true
    validation: "[A-Za-z][A-Za-z0-9_-]*"
  - name: parent_dir
    label: Create in
    type: directory
    default: "{{ default_projects_dir }}"
  - name: domain
    label: Domain
    type: domain
    default: "{{ project_name | slug }}.test"
  - name: https
    label: HTTPS
    type: boolean
    default: true

files:
  - path: index.html
    content: |
      <!doctype html>
      <title>{{ project_name }}</title>
      <h1>{{ project_name }}</h1>

domain:
  hostname: "{{ domain }}"
  kind: static
  root: "{{ project_path }}"
`

/** Mirrors the core's condition evaluator (§85): ==, !=, !x, bare truthiness, && / ||. */
function evalCondition(expr: string, values: Record<string, string>): boolean {
  const lookup = (n: string) => values[n.trim()] ?? ''
  const truthy = (v: string) => !['', 'false', '0', 'no', 'none', 'off'].includes(v.trim().toLowerCase())
  const unq = (s: string) => s.trim().replace(/^["']|["']$/g, '')
  const norm = expr.replace(/ and /g, ' && ').replace(/ or /g, ' || ')
  return norm.split('||').some((orPart) =>
    orPart.split('&&').every((raw) => {
      const atom = raw.trim()
      if (atom.includes('==')) {
        const [l, r] = atom.split('==')
        return lookup(l).toLowerCase() === unq(r).toLowerCase()
      }
      if (atom.includes('!=')) {
        const [l, r] = atom.split('!=')
        return lookup(l).toLowerCase() !== unq(r).toLowerCase()
      }
      if (atom.startsWith('!')) return !truthy(lookup(atom.slice(1)))
      if (atom.startsWith('not ')) return !truthy(lookup(atom.slice(4)))
      return truthy(lookup(atom))
    }),
  )
}

export function QuickAppsPage({ onNavigate }: { onNavigate: (p: Page) => void }) {
  const [apps, setApps] = useState<QuickEntryView[]>([])
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState('all')
  const [favOnly, setFavOnly] = useState(false)
  const [wizardId, setWizardId] = useState<string | null>(null)
  const [editor, setEditor] = useState<{ yaml: string; isNew: boolean } | null>(null)
  const [importOpen, setImportOpen] = useState(false)
  const [dup, setDup] = useState<QuickEntryView | null>(null)
  const { busy, error, setError, run } = useAction()

  async function refresh() {
    const r = await runCommand({ type: 'list_quick_apps' })
    if (r.type === 'quick_apps') setApps(r.apps)
  }
  useEffect(() => {
    refresh().catch((e) => setError(asDiagnostic(e)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const shown = useMemo(
    () =>
      apps
        .filter((a) => (category === 'all' || a.category === category) && (!favOnly || a.favorite))
        .filter((a) => `${a.name} ${a.description}`.toLowerCase().includes(query.toLowerCase()))
        .sort((a, b) => Number(b.favorite) - Number(a.favorite) || a.name.localeCompare(b.name)),
    [apps, query, category, favOnly],
  )

  async function openEditor(id: string) {
    const r = await runCommand({ type: 'get_quick_app', id })
    if (r.type === 'quick_app') setEditor({ yaml: r.detail.yaml, isNew: false })
  }

  async function exportApp(id: string) {
    const dest = await save({ title: 'Export Quick App', defaultPath: `${id}.yaml`, filters: [{ name: 'YAML', extensions: ['yaml', 'yml'] }] })
    if (dest) await runCommand({ type: 'export_quick_app', id, dest })
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Quick Apps</h1>
          <p className="text-sm text-muted-foreground">
            Editable recipes that create a project and its whole environment: runtimes, database, domain, HTTPS (§79–88).
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setImportOpen(true)}>
            <Upload /> Import
          </Button>
          <Button onClick={() => setEditor({ yaml: NEW_APP_YAML, isNew: true })}>
            <Plus /> New Quick App
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />

      <div className="flex flex-wrap items-center gap-3">
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" className="w-64" />
        <div className="flex gap-1">
          {CATEGORIES.map((c) => (
            <Button key={c} size="sm" variant={category === c ? 'secondary' : 'ghost'} onClick={() => setCategory(c)} className="capitalize">
              <TechIcon id={c} /> {c}
            </Button>
          ))}
        </div>
        <Toggle checked={favOnly} onChange={setFavOnly} label="Favorites only" />
      </div>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {shown.map((a) => (
          <Card key={a.id} className="flex flex-col">
            <CardHeader className="flex-row items-start justify-between gap-3 space-y-0 pb-2">
              <TechTile id={a.id in BRANDED_APPS ? a.id : a.category} />
              <div className="min-w-0 flex-1">
                <CardTitle className="flex items-center gap-2 text-base">
                  {a.name}
                  {a.source === 'imported' && <Badge variant={a.trusted ? 'secondary' : 'warning'}>{a.trusted ? 'imported' : 'untrusted'}</Badge>}
                  {a.overrides_builtin && <Badge variant="secondary">modified</Badge>}
                </CardTitle>
                <CardDescription className="mt-1 line-clamp-2">{a.description}</CardDescription>
              </div>
              <button
                onClick={() => run('fav', async () => { await runCommand({ type: 'favorite_quick_app', id: a.id, favorite: !a.favorite }); await refresh() })}
                className={a.favorite ? 'text-warning' : 'text-muted-foreground hover:text-foreground'}
                title="Favorite"
              >
                <Star className="size-4" fill={a.favorite ? 'currentColor' : 'none'} />
              </button>
            </CardHeader>
            <CardContent className="mt-auto flex items-center justify-between gap-2 pt-2">
              <div className="flex gap-0.5">
                <Button size="sm" variant="ghost" title="Edit" onClick={() => run('edit', () => openEditor(a.id))}>
                  <Pencil className="size-3.5" />
                </Button>
                <Button size="sm" variant="ghost" title="Duplicate" onClick={() => setDup(a)}>
                  <Copy className="size-3.5" />
                </Button>
                <Button size="sm" variant="ghost" title="Export" onClick={() => run('export', () => exportApp(a.id))}>
                  <Download className="size-3.5" />
                </Button>
                {(a.source !== 'builtin' || a.overrides_builtin) && (
                  <Button
                    size="sm"
                    variant="ghost"
                    title={a.overrides_builtin ? 'Revert to the built-in version' : 'Delete'}
                    onClick={() => confirmThen(a.overrides_builtin ? 'Discard your changes and restore the built-in recipe?' : `Delete "${a.name}"?`, () => run('delete', async () => { await runCommand({ type: 'delete_quick_app', id: a.id }); await refresh() }))}
                  >
                    <Trash2 className="size-3.5" />
                  </Button>
                )}
              </div>
              <Button size="sm" onClick={() => setWizardId(a.id)} disabled={busy !== null}>
                <Rocket /> Create
              </Button>
            </CardContent>
          </Card>
        ))}
        {shown.length === 0 && <p className="text-sm text-muted-foreground">Nothing matches.</p>}
      </div>

      {wizardId && <Wizard key={wizardId} id={wizardId} onClose={() => { setWizardId(null); void refresh() }} onNavigate={onNavigate} />}

      <YamlEditorDialog
        state={editor}
        onClose={() => setEditor(null)}
        onSaved={async () => { setEditor(null); await refresh() }}
      />
      <ImportDialog open={importOpen} onClose={() => setImportOpen(false)} onDone={async () => { setImportOpen(false); await refresh() }} />

      <DuplicateDialog app={dup} onClose={() => setDup(null)} onDone={async () => { setDup(null); await refresh() }} />
    </div>
  )
}

function DuplicateDialog({ app, onClose, onDone }: { app: QuickEntryView | null; onClose: () => void; onDone: () => void }) {
  const [id, setId] = useState('')
  const [name, setName] = useState('')
  const { busy, error, setError, run } = useAction()
  useEffect(() => {
    if (app) {
      setId(`${app.id}-copy`)
      setName(`${app.name} (copy)`)
    }
  }, [app])
  return (
    <Dialog
      open={!!app}
      onClose={onClose}
      title={`Duplicate ${app?.name ?? ''}`}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button disabled={busy !== null} onClick={() => run('dup', async () => { await runCommand({ type: 'duplicate_quick_app', id: app!.id, new_id: id, new_name: name }); onDone() })}>Duplicate</Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="Id" hint="Lowercase letters, digits and dashes">
          <Input value={id} onChange={(e) => setId(e.target.value)} />
        </Field>
        <Field label="Name">
          <Input value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
      </div>
    </Dialog>
  )
}

function YamlEditorDialog({ state, onClose, onSaved }: { state: { yaml: string; isNew: boolean } | null; onClose: () => void; onSaved: () => void }) {
  const [yaml, setYaml] = useState('')
  const { busy, error, setError, run } = useAction()
  useEffect(() => {
    if (state) setYaml(state.yaml)
  }, [state])
  return (
    <Dialog
      wide
      open={!!state}
      onClose={onClose}
      title={state?.isNew ? 'New Quick App' : 'Edit Quick App'}
      description="Editing a built-in creates your own copy that replaces it; deleting that copy restores the original."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button disabled={busy !== null} onClick={() => run('save', async () => { await runCommand({ type: 'save_quick_app', yaml }); onSaved() })}>Save</Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <CodeEditor value={yaml} onChange={setYaml} language="yaml" height="55vh" />
      </div>
    </Dialog>
  )
}

function ImportDialog({ open: isOpen, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => void }) {
  const [source, setSource] = useState('')
  const { busy, error, setError, run } = useAction()
  return (
    <Dialog
      open={isOpen}
      onClose={onClose}
      title="Import Quick Apps"
      description="From a file, a folder, or a Git repository. Imported recipes are untrusted until you approve them (§88)."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button disabled={busy !== null || !source.trim()} onClick={() => run('import', async () => { await runCommand({ type: 'import_quick_app', source: source.trim() }); setSource(''); onDone() })}>Import</Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Field label="File, folder or Git URL">
          <Input value={source} onChange={(e) => setSource(e.target.value)} placeholder="C:\recipes\my-app.yaml  or  https://github.com/team/quick-apps.git" />
        </Field>
        <div className="flex gap-2">
          <Button variant="secondary" size="sm" onClick={async () => { const p = await open({ multiple: false, filters: [{ name: 'YAML', extensions: ['yaml', 'yml'] }] }); if (p && !Array.isArray(p)) setSource(p) }}>
            <FolderSearch /> Choose file
          </Button>
          <Button variant="secondary" size="sm" onClick={async () => { const p = await open({ directory: true }); if (p && !Array.isArray(p)) setSource(p) }}>
            <FolderSearch /> Choose folder
          </Button>
        </div>
      </div>
    </Dialog>
  )
}

// ------------------------------------------------------------------------------ wizard

type Phase = 'form' | 'review' | 'run'

export function Wizard({ id, onClose, onNavigate }: { id: string; onClose: () => void; onNavigate: (p: Page) => void }) {
  const [detail, setDetail] = useState<QuickEntryDetail | null>(null)
  const [provided, setProvided] = useState<Record<string, string>>({})
  const [plan, setPlan] = useState<QuickPlanResult | null>(null)
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [phase, setPhase] = useState<Phase>('form')
  const [approval, setApproval] = useState<'once' | 'source' | null>(null)
  const [allowElevated, setAllowElevated] = useState(false)
  const [runId, setRunId] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [starting, setStarting] = useState(false)

  useEffect(() => {
    runCommand({ type: 'get_quick_app', id }).then((r) => r.type === 'quick_app' && setDetail(r.detail)).catch((e) => setError(asDiagnostic(e)))
    runCommand({ type: 'list_runtime_catalog' }).then((r) => r.type === 'runtime_catalog' && setCatalog(r.entries))
  }, [id])

  // Re-plan (debounced) as answers change: this is what fills in templated defaults
  // ("{{ project_name }}.test"), validates, and builds the reviewable step list.
  useEffect(() => {
    if (phase !== 'form') return
    const t = setTimeout(() => {
      runCommand({ type: 'plan_quick_app', id, values: provided })
        .then((r) => r.type === 'quick_plan' && setPlan(r.result))
        .catch((e) => setError(asDiagnostic(e)))
    }, 250)
    return () => clearTimeout(t)
  }, [id, provided, phase])

  const app = detail?.app
  const resolved = plan?.values ?? {}
  const valueOf = (name: string) => provided[name] ?? resolved[name] ?? ''
  const shown = (app?.variables ?? []).filter((v) => !v.show_if || evalCondition(v.show_if, { ...resolved, ...provided }))
  const errorFor = (name: string) => plan?.errors.find((e) => e.field === name)?.message

  async function start() {
    setStarting(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'start_quick_app', id, values: provided, approval, allow_elevated: allowElevated })
      if (r.type === 'quick_run_started') {
        setRunId(r.run_id)
        setPhase('run')
      }
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setStarting(false)
    }
  }

  const untrusted = plan && !plan.trusted
  const hasElevated = plan?.plan?.steps.some((s) => s.elevated) ?? false
  const canStart = !!plan?.ok && (!untrusted || approval !== null) && (!hasElevated || allowElevated)

  return (
    <Dialog
      size="form"
      open
      onClose={phase === 'run' ? onClose : onClose}
      icon={app ? <TechTile id={app.id in BRANDED_APPS ? app.id : app.category} className="size-10 rounded-lg [&_svg]:size-5" /> : undefined}
      title={app ? `Create ${app.name}` : 'Loading…'}
      description={phase === 'form' ? app?.description : phase === 'review' ? 'Review exactly what will happen before anything runs.' : 'Creating your project…'}
      footer={
        phase === 'form' ? (
          <>
            <Button variant="ghost" onClick={onClose}>Cancel</Button>
            <Button disabled={!plan?.ok} onClick={() => setPhase('review')}>Review</Button>
          </>
        ) : phase === 'review' ? (
          <>
            <Button variant="ghost" onClick={() => setPhase('form')}>Back</Button>
            <Button disabled={!canStart || starting} onClick={start}>
              {starting ? <Loader2 className="animate-spin" /> : <Rocket />} Create app
            </Button>
          </>
        ) : null
      }
    >
      <div className="flex flex-col gap-5">
        <ErrorCard error={error} onDismiss={() => setError(null)} />

        {phase === 'form' && app && (
          <FormSection title="Details">
            <div className="grid gap-3 sm:grid-cols-2">
              {shown.map((v) => (
                <VariableField
                  key={v.name}
                  v={v}
                  value={valueOf(v.name)}
                  touched={provided[v.name] !== undefined}
                  error={errorFor(v.name)}
                  catalog={catalog}
                  onChange={(val) => setProvided((p) => ({ ...p, [v.name]: val }))}
                />
              ))}
            </div>
          </FormSection>
        )}

        {phase === 'review' && plan?.plan && (
          <Review plan={plan} approval={approval} setApproval={setApproval} allowElevated={allowElevated} setAllowElevated={setAllowElevated} />
        )}

        {phase === 'run' && runId && <RunProgress runId={runId} onClose={onClose} onNavigate={onNavigate} />}
      </div>
    </Dialog>
  )
}

function VariableField({
  v,
  value,
  touched,
  error,
  catalog,
  onChange,
}: {
  v: QuickVariable
  value: string
  touched: boolean
  error?: string
  catalog: CatalogEntry[]
  onChange: (val: string) => void
}) {
  const label = v.label || v.name
  const wide = ['directory', 'path', 'file'].includes(v.type)

  if (v.type === 'boolean') {
    return (
      <div className="pt-5">
        <Toggle checked={value === 'true'} onChange={(c) => onChange(c ? 'true' : 'false')} label={label} hint={v.help ?? undefined} />
      </div>
    )
  }
  if (v.type === 'select') {
    return (
      <Field label={label} error={error} hint={v.help ?? undefined}>
        <Select value={value} onChange={(e) => onChange(e.target.value)}>
          {v.options.map((o) => (
            <option key={String(o)} value={String(o)}>{String(o)}</option>
          ))}
        </Select>
      </Field>
    )
  }
  if (v.type === 'multiselect') {
    const chosen = value.split(',').map((s) => s.trim()).filter(Boolean)
    return (
      <Field label={label} error={error}>
        <div className="flex flex-wrap gap-3">
          {v.options.map((o) => (
            <Toggle
              key={String(o)}
              checked={chosen.includes(String(o))}
              onChange={(c) => onChange((c ? [...chosen, String(o)] : chosen.filter((x) => x !== String(o))).join(','))}
              label={String(o)}
            />
          ))}
        </div>
      </Field>
    )
  }
  if (v.type === 'runtime-version' || v.type === 'database-version') {
    const entries = catalog.filter((c) => c.id === v.runtime)
    const versions = Array.from(new Set(entries.map((c) => c.version.split('.').slice(0, 2).join('.'))))
    return (
      <Field label={label} error={error} hint={v.help ?? undefined}>
        <Select value={value} onChange={(e) => onChange(e.target.value)}>
          {versions.map((ver) => {
            const installed = entries.some((c) => c.installed && c.version.startsWith(`${ver}.`))
            return (
              <option key={ver} value={ver}>{ver}{installed ? ' (installed)' : ' (will be downloaded)'}</option>
            )
          })}
          {value && !versions.includes(value) && <option value={value}>{value}</option>}
        </Select>
      </Field>
    )
  }
  const isPicker = wide
  return (
    <div className={wide ? 'sm:col-span-2' : ''}>
      <Field label={label + (v.required ? ' *' : '')} error={error} hint={v.help ?? undefined}>
        <div className="flex gap-2">
          <Input
            type={v.type === 'password' || v.type === 'secret' ? 'password' : v.type === 'number' || v.type === 'port' ? 'number' : 'text'}
            value={touched ? value : ''}
            placeholder={value}
            onChange={(e) => onChange(e.target.value)}
          />
          {isPicker && (
            <Button
              variant="secondary"
              className="shrink-0"
              onClick={async () => {
                const p = await open({ directory: v.type !== 'file', multiple: false })
                if (p && !Array.isArray(p)) onChange(p)
              }}
            >
              <FolderSearch /> Browse
            </Button>
          )}
        </div>
      </Field>
    </div>
  )
}

const STAGE_LABEL: Record<string, string> = {
  requirements: 'Requirements',
  pre_create: 'Before creating',
  create: 'Create the project',
  files: 'Files and settings',
  post_create: 'After creating',
  pre_install: 'Before install',
  install: 'Install dependencies',
  post_install: 'After install',
  finalize: 'Domain, HTTPS and web server',
  pre_start: 'Before start',
  post_start: 'After start',
}

function Review({
  plan,
  approval,
  setApproval,
  allowElevated,
  setAllowElevated,
}: {
  plan: QuickPlanResult
  approval: 'once' | 'source' | null
  setApproval: (a: 'once' | 'source' | null) => void
  allowElevated: boolean
  setAllowElevated: (b: boolean) => void
}) {
  const p = plan.plan!
  const stages = Array.from(new Set(p.steps.map((s) => s.stage)))
  const hasElevated = p.steps.some((s) => s.elevated)
  return (
    <div className="flex flex-col gap-4">
      {!plan.trusted && (
        <Card className="border-warning/50 bg-warning/5">
          <CardHeader className="pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <AlertTriangle className="size-4 text-warning" /> This Quick App comes from an untrusted source ({plan.source})
            </CardTitle>
            <CardDescription>Read the commands below. It will run them on your computer.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            <Button size="sm" variant={approval === 'once' ? 'default' : 'secondary'} onClick={() => setApproval('once')}>Allow once</Button>
            <Button size="sm" variant={approval === 'source' ? 'default' : 'secondary'} onClick={() => setApproval('source')}>Trust this source</Button>
            {approval && <span className="self-center text-xs text-muted-foreground">Approved. Press Create app to run it.</span>}
          </CardContent>
        </Card>
      )}

      <div className="grid gap-4 md:grid-cols-2">
        <div>
          <h3 className="mb-2 text-sm font-medium">It will</h3>
          <ul className="flex flex-col gap-1 text-sm">
            {p.permissions.map((perm) => (
              <li key={perm.id} className="flex gap-2">
                <span className="text-muted-foreground">•</span>
                {perm.label}
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h3 className="mb-2 text-sm font-medium">Requirements</h3>
          <ul className="flex flex-col gap-1 text-sm">
            {plan.requirements.map((r) => (
              <li key={r.id} className="flex items-start gap-2">
                {r.status === 'installed' ? <CheckCircle2 className="mt-0.5 size-4 text-success" /> : r.status === 'installable' ? <Download className="mt-0.5 size-4 text-primary" /> : <XCircle className="mt-0.5 size-4 text-destructive" />}
                <span>
                  {r.label}{r.wanted ? ` ${r.wanted}` : ''}
                  <span className="block text-xs text-muted-foreground">
                    {r.status === 'installed' ? `installed (${r.detail})` : r.detail}
                  </span>
                </span>
              </li>
            ))}
          </ul>
          {plan.requirements.some((r) => r.status === 'unavailable') && (
            <p className="mt-2 text-xs text-destructive">Something required isn't available; the run will stop when it reaches that step.</p>
          )}
        </div>
      </div>

      {p.warnings.map((w) => (
        <p key={w} className="text-sm text-warning">{w}</p>
      ))}

      <div>
        <h3 className="mb-2 text-sm font-medium">Commands and steps</h3>
        <div className="flex flex-col gap-3 rounded-lg border border-border p-3">
          {stages.map((st) => (
            <div key={st}>
              <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{STAGE_LABEL[st] ?? st}</div>
              <ul className="mt-1 flex flex-col gap-1">
                {p.steps.filter((s) => s.stage === st).map((s, i) => (
                  <li key={i} className="text-sm">
                    {s.name}
                    {s.elevated && <Badge variant="warning" className="ml-2">administrator</Badge>}
                    {s.name !== s.display && <code className="block truncate rounded bg-muted px-2 py-0.5 text-xs">{s.display}</code>}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </div>

      {hasElevated && (
        <Toggle checked={allowElevated} onChange={setAllowElevated} label="I approve the steps that need administrator rights" hint="Windows will show its own approval prompt for each." />
      )}
    </div>
  )
}

function RunProgress({ runId, onClose, onNavigate }: { runId: string; onClose: () => void; onNavigate: (p: Page) => void }) {
  const [run, setRun] = useState<RunView | null>(null)
  const logRef = useRef<HTMLPreElement>(null)

  useEffect(() => {
    let alive = true
    const tick = async () => {
      try {
        const r = await runCommand({ type: 'get_quick_run', id: runId })
        if (alive && r.type === 'quick_run') setRun(r.run)
      } catch {
        /* retry on the next tick */
      }
    }
    void tick()
    const t = setInterval(tick, 700)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [runId])

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight })
  }, [run?.log.length])

  if (!run) return <Loader2 className="animate-spin" />
  const icon = (s: string) =>
    s === 'done' ? <CheckCircle2 className="size-4 text-success" /> : s === 'running' ? <Loader2 className="size-4 animate-spin text-primary" /> : s === 'failed' ? <XCircle className="size-4 text-destructive" /> : <span className="size-4 text-center text-muted-foreground">·</span>

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 md:grid-cols-2">
        <ul className="flex max-h-72 flex-col gap-1.5 overflow-y-auto rounded-lg border border-border p-3">
          {run.steps.map((s, i) => (
            <li key={i} className="flex items-center gap-2 text-sm">
              {icon(s.status)}
              <span className={s.status === 'pending' || s.status === 'skipped' ? 'text-muted-foreground' : ''}>{s.name}</span>
            </li>
          ))}
        </ul>
        <pre ref={logRef} className="max-h-72 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-lg border border-border bg-muted/40 p-3 font-mono text-xs">
          {run.log.slice(-400).join('\n')}
        </pre>
      </div>

      {run.state !== 'running' && (
        <Card className={run.state === 'succeeded' ? 'border-success/40' : 'border-destructive/40'}>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">{run.state === 'succeeded' ? 'Done' : run.state === 'cancelled' ? 'Cancelled' : 'It stopped with an error'}</CardTitle>
            {run.error && <CardDescription className="text-destructive">{run.error}</CardDescription>}
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {run.results.map((r, i) => (
              <div key={i} className="flex items-start gap-2 text-sm">
                {r.ok ? <CheckCircle2 className="mt-0.5 size-4 text-success" /> : <XCircle className="mt-0.5 size-4 text-destructive" />}
                <span>
                  {r.label}
                  {r.detail && <span className="text-muted-foreground"> · {r.detail}</span>}
                </span>
              </div>
            ))}
            {run.warnings.map((w) => (
              <div key={w} className="text-sm text-warning">{w}</div>
            ))}
          </CardContent>
        </Card>
      )}

      <div className="flex justify-end gap-2">
        {run.state === 'running' ? (
          <Button variant="outline" onClick={() => runCommand({ type: 'cancel_quick_run', id: runId })}>Cancel</Button>
        ) : (
          <>
            {run.project_id && (
              <Button variant="secondary" onClick={() => { onClose(); onNavigate('sites') }}>Go to Sites</Button>
            )}
            {run.open_url && run.state === 'succeeded' && (
              <Button onClick={() => runCommand({ type: 'open_url', url: run.open_url! })}>
                <ExternalLink /> Open {run.open_url}
              </Button>
            )}
            <Button variant="ghost" onClick={onClose}>Close</Button>
          </>
        )}
      </div>
    </div>
  )
}
