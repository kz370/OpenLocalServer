import { Bug, Check, Copy, PackageCheck, Play, RefreshCw, Send, Trash2, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { ProjectTerminal } from '@/components/Terminal'
import { EnvEditor } from '@/components/EnvEditor'
import { XdebugDialog } from '@/components/XdebugDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Field, Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import {
  type ComposerInfo,
  type CoreCommand,
  type MailCheck,
  type MailEnvPlan,
  type PackageManagerInfo,
  type ProjectDetail,
  type VenvInfo,
  type XdebugReport,
  runCommand,
} from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmThen } from '@/lib/confirm'

type ToolTab = 'terminal' | 'env' | 'mail' | 'composer' | 'node' | 'python' | 'xdebug'

/**
 * Composer, Node package managers, Python venv and Xdebug for one project (§13–17).
 * `start` runs a command that returns a process and shows its live output in the page.
 * `refreshKey` changes when that process finishes, so the panels re-read the project files.
 */
export function ProjectTools({ detail, start, refreshKey }: { detail: ProjectDetail; start: (cmd: CoreCommand) => Promise<void>; refreshKey: number }) {
  const id = detail.project.id
  const markers = detail.detection.markers
  const [tab, setTab] = useState<ToolTab>(markers.includes('composer.json') ? 'composer' : markers.includes('package.json') ? 'node' : 'composer')
  const [composer, setComposer] = useState<ComposerInfo | null>(null)
  const [managers, setManagers] = useState<PackageManagerInfo | null>(null)
  const [venv, setVenv] = useState<VenvInfo | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const [c, m, v] = await Promise.all([
      runCommand({ type: 'get_composer_info', project_id: id }).catch(() => null),
      runCommand({ type: 'get_package_managers', project_id: id }).catch(() => null),
      runCommand({ type: 'get_venv', project_id: id }).catch(() => null),
    ])
    if (c?.type === 'composer') setComposer(c.info)
    if (m?.type === 'package_managers') setManagers(m.info)
    if (v?.type === 'venv') setVenv(v.info)
  }, [id])

  useEffect(() => {
    void load()
  }, [load, refreshKey])

  const go = (key: string, cmd: CoreCommand) => run(key, () => start(cmd))

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Tools</CardTitle>
        <CardDescription>Run Composer, pnpm / yarn, Python environments and Xdebug for this project.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <Tabs
          tabs={[
            { id: 'terminal', label: 'Terminal' },
            { id: 'env', label: '.env' },
            { id: 'mail', label: 'Mail' },
            { id: 'composer', label: 'Composer', badge: composer?.packages.length || undefined },
            { id: 'node', label: 'Node' },
            { id: 'python', label: 'Python' },
            { id: 'xdebug', label: 'Xdebug' },
          ]}
          value={tab}
          onChange={setTab}
        />

        {tab === 'terminal' && <ProjectTerminal projectId={id} />}
        {tab === 'env' && <EnvEditor projectId={id} />}
        {tab === 'mail' && <MailPanel projectId={id} />}
        {tab === 'composer' && <ComposerPanel projectId={id} info={composer} busy={busy} go={go} />}
        {tab === 'node' && <NodePanel projectId={id} info={managers} busy={busy} go={go} />}
        {tab === 'python' && <PythonPanel projectId={id} info={venv} busy={busy} go={go} />}
        {tab === 'xdebug' && <XdebugPanel detail={detail} />}
      </CardContent>
    </Card>
  )
}

type Go = (key: string, cmd: CoreCommand) => Promise<unknown>

/** §63, §66: point the project's .env at Mailpit after showing the change, check the mail path, send a test. */
function MailPanel({ projectId }: { projectId: string }) {
  const [plan, setPlan] = useState<MailEnvPlan | null>(null)
  const [checks, setChecks] = useState<MailCheck[]>([])
  const [to, setTo] = useState('dev@example.test')
  const [message, setMessage] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const [p, c] = await Promise.all([
      runCommand({ type: 'mailpit_env_plan', project_id: projectId, file: '.env' }).catch(() => null),
      runCommand({ type: 'mail_diagnostics', project_id: projectId }),
    ])
    if (p?.type === 'mail_env_plan') setPlan(p.plan)
    if (c.type === 'mail_checks') setChecks(c.checks)
  }, [projectId])

  useEffect(() => {
    load().catch((e) => setError(e))
  }, [load, setError])

  const changed = plan?.changes.filter((c) => c.changed) ?? []

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-medium">Checklist</h3>
          <Button size="sm" variant="ghost" onClick={() => run('check', load)}>
            <RefreshCw className="size-3.5" /> Check again
          </Button>
        </div>
        {checks.map((c) => (
          <div key={c.id} className="flex items-start gap-2 rounded-lg border border-border px-3 py-1.5 text-sm">
            {c.ok ? <Check className="mt-0.5 size-4 text-success" /> : <X className="mt-0.5 size-4 text-destructive" />}
            <div>
              <div>{c.label}</div>
              <div className="text-xs text-muted-foreground">{c.detail}</div>
              {!c.ok && c.fix && <div className="text-xs text-warning">{c.fix}</div>}
            </div>
          </div>
        ))}
      </div>

      {plan && (
        <div className="flex flex-col gap-2">
          <h3 className="text-sm font-medium">Point .env at Mailpit</h3>
          {plan.note && <p className="text-sm text-muted-foreground">{plan.note}</p>}
          {plan.changes.length > 0 && (
            <div className="rounded-lg border border-border">
              {plan.changes.map((c) => (
                <div key={c.key} className="flex items-center justify-between gap-3 border-b border-border px-3 py-1.5 font-mono text-xs last:border-b-0">
                  <span>{c.key}</span>
                  <span className={c.changed ? '' : 'text-muted-foreground'}>
                    {c.changed ? (
                      <>
                        <span className="text-destructive line-through">{c.current ?? '(not set)'}</span> → <span className="text-success">{c.new}</span>
                      </>
                    ) : (
                      c.new
                    )}
                  </span>
                </div>
              ))}
            </div>
          )}
          {plan.changes.length > 0 && (
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                disabled={plan.up_to_date || busy !== null}
                onClick={() =>
                  run('apply', async () => {
                    const r = await runCommand({ type: 'apply_mailpit_env', project_id: projectId, file: '.env' })
                    if (r.type === 'mail_env_plan') setPlan(r.plan)
                    setMessage('.env updated. The previous version is kept in the .env editor’s history.')
                    await load()
                  })
                }
              >
                {plan.up_to_date ? 'Already pointing at Mailpit' : `Apply ${changed.length} change${changed.length === 1 ? '' : 's'}`}
              </Button>
              <span className="text-xs text-muted-foreground">Only the lines above are touched.</span>
            </div>
          )}
        </div>
      )}

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Send a test message</h3>
        <div className="flex items-center gap-2">
          <Input className="w-64" value={to} onChange={(e) => setTo(e.target.value)} placeholder="dev@example.test" />
          <Button
            size="sm"
            variant="secondary"
            disabled={!to || busy !== null}
            onClick={() =>
              run('send', async () => {
                const r = await runCommand({ type: 'send_test_mail', to })
                if (r.type === 'text') setMessage(r.text)
              })
            }
          >
            <Send className="size-3.5" /> Send
          </Button>
        </div>
      </div>
      {message && <p className="text-sm text-success">{message}</p>}
    </div>
  )
}

function ComposerPanel({ projectId, info, busy, go }: { projectId: string; info: ComposerInfo | null; busy: string | null; go: Go }) {
  const [pkg, setPkg] = useState('')
  const composer = (action: string, target: string | null = null): CoreCommand => ({ type: 'run_composer', project_id: projectId, action, target })
  if (!info) return <p className="text-sm text-muted-foreground">Reading composer.json…</p>
  if (!info.has_composer_json) return <p className="text-sm text-muted-foreground">This project has no composer.json.</p>

  const simple: [string, string][] = [
    ['install', 'Install'],
    ['update', 'Update all'],
    ['dump_autoload', 'Dump autoload'],
    ['outdated', 'Outdated'],
    ['validate', 'Validate'],
    ['audit', 'Audit'],
    ['diagnose', 'Diagnose'],
    ['clear_cache', 'Clear cache'],
  ]

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-1.5">
        {info.vendor_installed ? <Badge variant="success">vendor installed</Badge> : <Badge variant="warning">not installed</Badge>}
        {info.has_lock ? <Badge variant="secondary">composer.lock</Badge> : <Badge variant="outline">no lock file</Badge>}
        {info.name && <span className="text-xs text-muted-foreground">{info.name}</span>}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {simple.map(([action, label]) => (
          <Button key={action} size="sm" variant={action === 'install' ? 'default' : 'secondary'} disabled={busy !== null} onClick={() => go(action, composer(action))}>
            {label}
          </Button>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Input className="h-8 max-w-64" value={pkg} onChange={(e) => setPkg(e.target.value)} placeholder="vendor/package or vendor/package:^1.0" />
        <Button size="sm" disabled={busy !== null || !pkg.trim()} onClick={() => go('require', composer('require', pkg.trim())).then(() => setPkg(''))}>
          Require
        </Button>
        <Button size="sm" variant="secondary" disabled={busy !== null || !pkg.trim()} onClick={() => go('require_dev', composer('require_dev', pkg.trim())).then(() => setPkg(''))}>
          Require (dev)
        </Button>
      </div>
      {info.scripts.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-muted-foreground">Scripts:</span>
          {info.scripts.map((s) => (
            <Button key={s} size="sm" variant="outline" className="h-7" disabled={busy !== null} onClick={() => go(`script-${s}`, composer('run_script', s))}>
              <Play className="size-3" /> {s}
            </Button>
          ))}
        </div>
      )}
      {info.packages.length > 0 && (
        <div className="max-h-64 overflow-y-auto rounded-md border border-border">
          {info.packages.map((p) => (
            <div key={`${p.dev}-${p.name}`} className="group flex items-center gap-2 border-b border-border px-3 py-1.5 text-sm last:border-b-0">
              <span className="min-w-0 flex-1 truncate font-medium">{p.name}</span>
              {p.dev && <Badge variant="secondary">dev</Badge>}
              <span className="font-mono text-xs text-muted-foreground">{p.constraint}</span>
              <span className="w-24 truncate text-right font-mono text-xs">{p.locked ?? '—'}</span>
              <Button size="icon" variant="ghost" className="size-6" title="Update this package" disabled={busy !== null} onClick={() => go('update', composer('update', p.name))}>
                <RefreshCw className="size-3" />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="size-6"
                title="Remove this package"
                disabled={busy !== null}
                onClick={() => confirmThen(`Remove ${p.name} from composer.json?`, () => go('remove', composer('remove', p.name)))}
              >
                <Trash2 className="size-3" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function NodePanel({ projectId, info, busy, go }: { projectId: string; info: PackageManagerInfo | null; busy: string | null; go: Go }) {
  if (!info) return <p className="text-sm text-muted-foreground">Checking Node tools…</p>
  const pm = info.detected && ['npm', 'pnpm', 'yarn'].includes(info.detected) ? info.detected : 'npm'
  const ready = { npm: info.npm, pnpm: info.pnpm, yarn: info.yarn }
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm">
        {info.detected ? (
          <>
            This project uses <b>{info.detected}</b>
            {info.pinned_version && <> {info.pinned_version}</>}
            <span className="text-muted-foreground"> (from {info.detected_from})</span>
          </>
        ) : (
          <span className="text-muted-foreground">No package manager is set in this project; npm is the default.</span>
        )}
      </p>
      {info.detected === 'bun' && <p className="text-xs text-warning">Bun is not managed by OpenLocalServer. Install it yourself and run it from a terminal.</p>}
      <div className="flex flex-col gap-2">
        {(['npm', 'pnpm', 'yarn'] as const).map((m) => (
          <div key={m} className="flex items-center justify-between rounded-md border border-border px-3 py-2 text-sm">
            <span className="font-medium">{m}</span>
            {ready[m] ? (
              <div className="flex items-center gap-2">
                <Badge variant="success">ready</Badge>
                <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => go(`${m}-install`, { type: 'run_command_line', line: `${m} install`, cwd: null, project_id: projectId })}>
                  <Play /> {m} install
                </Button>
              </div>
            ) : m === 'npm' ? (
              <Badge variant="destructive">Node is not installed</Badge>
            ) : (
              <div className="flex items-center gap-2">
                <Badge variant="secondary">not switched on</Badge>
                <Button size="sm" disabled={busy !== null || !info.corepack} title={info.corepack ? '' : 'This Node has no corepack'} onClick={() => go(`enable-${m}`, { type: 'enable_package_manager', project_id: projectId, manager: m })}>
                  <PackageCheck /> Enable with corepack
                </Button>
              </div>
            )}
          </div>
        ))}
      </div>
      {!info.corepack && <p className="text-xs text-muted-foreground">This Node.js has no corepack (Node 25 and newer dropped it). Use the bundled Node 24, or install pnpm with npm.</p>}
      {pm !== 'npm' && !ready[pm as 'pnpm' | 'yarn'] && <p className="text-xs text-warning">The project uses {pm}, which is not switched on yet.</p>}
    </div>
  )
}

function PythonPanel({ projectId, info, busy, go }: { projectId: string; info: VenvInfo | null; busy: string | null; go: Go }) {
  const [what, setWhat] = useState('')
  if (!info) return <p className="text-sm text-muted-foreground">Looking for a virtual environment…</p>
  const choices = [...info.requirements, ...(info.has_pyproject ? ['pyproject'] : [])]
  const pick = what && choices.includes(what) ? what : (choices[0] ?? '')
  return (
    <div className="flex flex-col gap-3">
      {info.exists ? (
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <Badge variant={info.base_missing ? 'destructive' : 'success'}>{info.base_missing ? 'broken' : 'ready'}</Badge>
          <span className="font-mono text-xs">{info.dir_name}</span>
          {info.python_version && <span className="text-muted-foreground">Python {info.python_version}</span>}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">No virtual environment yet. Create one to keep this project's packages separate.</p>
      )}
      {info.base_missing && <p className="text-xs text-warning">The Python this environment was made from is gone ({info.base_home}). Recreate it.</p>}
      <div className="flex flex-wrap items-center gap-2">
        {!info.exists ? (
          <Button size="sm" disabled={busy !== null} onClick={() => go('venv-create', { type: 'create_venv', project_id: projectId, recreate: false })}>
            Create .venv
          </Button>
        ) : (
          <Button
            size="sm"
            variant="secondary"
            disabled={busy !== null}
            onClick={() => confirmThen(`Delete ${info.dir_name} and create it again? Installed packages are removed.`, () => go('venv-recreate', { type: 'create_venv', project_id: projectId, recreate: true }))}
          >
            <RefreshCw /> Recreate
          </Button>
        )}
      </div>
      {info.exists && choices.length > 0 && (
        <div className="flex flex-wrap items-end gap-2">
          <Field label="Install packages from">
            <Select className="w-64" value={pick} onChange={(e) => setWhat(e.target.value)}>
              {choices.map((c) => (
                <option key={c} value={c}>
                  {c === 'pyproject' ? 'pyproject.toml (editable install)' : c}
                </option>
              ))}
            </Select>
          </Field>
          <Button size="sm" disabled={busy !== null} onClick={() => go('venv-install', { type: 'install_venv_requirements', project_id: projectId, what: pick })}>
            pip install
          </Button>
        </div>
      )}
      {info.exists && (
        <p className="text-xs text-muted-foreground">
          Commands you run for this project (Quick Commands, the Commands page) use this environment automatically. To use it in your own terminal:{' '}
          <code className="rounded bg-muted px-1 py-0.5">{info.activate_command}</code>
        </p>
      )}
    </div>
  )
}

function XdebugPanel({ detail }: { detail: ProjectDetail }) {
  const php = detail.resolved.find((r) => r.id === 'php')?.installed_version ?? null
  const [report, setReport] = useState<XdebugReport | null>(null)
  const [ide, setIde] = useState('vscode')
  const [config, setConfig] = useState('')
  const [open, setOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  const projectId = detail.project.id

  const refresh = useCallback(() => {
    if (!php) return
    void runCommand({ type: 'get_xdebug', version: php }).then((r) => r.type === 'xdebug' && setReport(r.report))
  }, [php])
  useEffect(() => refresh(), [refresh])
  useEffect(() => {
    if (!php) return
    void runCommand({ type: 'xdebug_ide_config', project_id: projectId, ide, version: php }).then((r) => r.type === 'text' && setConfig(r.text))
  }, [php, ide, projectId, report])

  if (!php) return <p className="text-sm text-muted-foreground">This project has no PHP version resolved. Install PHP from the Runtimes page.</p>

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border px-3 py-2 text-sm">
        <div className="flex items-center gap-2">
          <Bug className="size-4 text-muted-foreground" />
          <span>PHP {php}</span>
          {report && <Badge variant={report.enabled ? 'success' : report.installed ? 'secondary' : 'outline'}>{report.enabled ? 'Xdebug on' : report.installed ? 'Xdebug off' : 'Xdebug not installed'}</Badge>}
        </div>
        <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>
          Xdebug settings…
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">Xdebug is set per PHP version, so it applies to every site running on PHP {php}. Debug one site at a time by opening it with ?XDEBUG_TRIGGER=1.</p>
      <div className="flex items-end gap-2">
        <Field label="IDE setup">
          <Select className="w-48" value={ide} onChange={(e) => setIde(e.target.value)}>
            <option value="vscode">VS Code</option>
            <option value="phpstorm">PhpStorm</option>
            <option value="other">Other IDE</option>
          </Select>
        </Field>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            void navigator.clipboard.writeText(config).then(() => {
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
            })
          }}
        >
          <Copy /> {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">{config}</pre>
      {open && (
        <XdebugDialog
          key={php}
          version={php}
          onClose={() => {
            setOpen(false)
            refresh()
          }}
        />
      )}
    </div>
  )
}
