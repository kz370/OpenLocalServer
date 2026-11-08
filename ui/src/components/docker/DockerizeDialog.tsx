import { AlertTriangle, Container, KeyRound } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Select } from '@/components/ui/select'
import { type Diagnostic, type DockerizeMode, type DockerizePlan, type Project, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'
import { notify } from '@/lib/notify'

/**
 * Dockerize a project. Shows what OLS detected, the services it would add (tick or untick
 * them), the fixed "starting point, not guaranteed" notice and checklist (both come from the
 * core, never typed here), and the exact files before anything is written. Nothing is
 * overwritten. 2.11.0: the project is copied into the image; Development mode syncs edits with
 * `docker compose watch`, Production builds a lean non-root image.
 */
export function DockerizeDialog({
  open,
  projectId,
  onClose,
  onCreated,
}: {
  open: boolean
  /** Preselects a project (right-click menus). */
  projectId?: string | null
  onClose: () => void
  /** The compose file that was written, whether to start it, and whether to start it with `watch`. Without it, the dialog starts it itself. */
  onCreated?: (file: string, start: boolean, watch: boolean) => void
}) {
  const [projects, setProjects] = useState<Project[]>([])
  const [id, setId] = useState('')
  const [mode, setMode] = useState<DockerizeMode>('dev')
  /** The services the user picked; `null` = what OLS detected. */
  const [choice, setChoice] = useState<string[] | null>(null)
  const [plan, setPlan] = useState<DockerizePlan | null>(null)
  const [loading, setLoading] = useState(false)
  const [planError, setPlanError] = useState<Diagnostic | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [busy, setBusy] = useState(false)
  const [replace, setReplace] = useState(false)

  useEffect(() => {
    if (!open) return
    let alive = true
    setPlan(null)
    setPlanError(null)
    setError(null)
    setReplace(false)
    setMode('dev')
    setChoice(null)
    setId(projectId ?? '')
    void runCommand({ type: 'list_projects' })
      .then((r) => alive && r.type === 'projects' && setProjects(r.projects))
      .catch((err) => alive && setError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [open, projectId])

  // A different project starts from what OLS detects again.
  useEffect(() => {
    setChoice(null)
    setPlan(null)
  }, [id])

  useEffect(() => {
    if (!open || !id) return
    let alive = true
    setLoading(true)
    setPlanError(null)
    void runCommand({ type: 'plan_dockerize', project_id: id, options: { mode, services: choice } })
      .then((r) => alive && r.type === 'dockerize_plan' && setPlan(r.plan))
      .catch((err) => alive && (setPlan(null), setPlanError(asDiagnostic(err))))
      .finally(() => alive && setLoading(false))
    return () => {
      alive = false
    }
  }, [open, id, mode, choice])

  const composeExists = !!plan && plan.existing.some((f) => f !== 'Dockerfile')

  function toggle(serviceId: string, on: boolean) {
    if (!plan) return
    const now = plan.services.filter((s) => s.selected).map((s) => s.id)
    setChoice(on ? [...now, serviceId] : now.filter((s) => s !== serviceId))
  }

  async function create(start: boolean, watch: boolean) {
    if (!plan) return
    setBusy(true)
    setError(null)
    try {
      const services = plan.services.filter((s) => s.selected).map((s) => s.id)
      const r = await runCommand({
        type: 'create_dockerize',
        project_id: id,
        overwrite_compose: replace,
        options: { mode, services },
      })
      if (r.type !== 'dockerize_plan') return
      notify.success("Files created. Review them before running — OLS can't guarantee they work for your project.")
      if (onCreated) onCreated(r.plan.compose_path, start, watch)
      else if (start) {
        await runCommand({ type: 'docker_compose_action', path: r.plan.compose_path, action: watch ? 'watch' : 'up' })
        notify.success('Starting the stack. Follow it on the Processes page.')
      }
      onClose()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const blocked = busy || loading || !plan || (composeExists && !replace)

  return (
    <Dialog
      open={open}
      onClose={onClose}
      size="wide"
      icon={<Container className="size-5 text-sky-500" />}
      title="Dockerize a project"
      description="Make a Dockerfile, a compose file and a .dockerignore from what OLS finds in the project. The project is copied into the image. Nothing is overwritten."
      footer={
        <div className="flex w-full items-center justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" variant="outline" className="cursor-pointer" disabled={blocked} onClick={() => void create(false, false)}>
            Create files
          </Button>
          <Button
            size="sm"
            variant={plan?.watch ? 'outline' : 'default'}
            className="cursor-pointer"
            disabled={blocked}
            onClick={() => void create(true, false)}
          >
            {busy && !plan?.watch && <Spinner className="size-3.5" />} Create and start
          </Button>
          {plan?.watch && (
            <Button
              size="sm"
              className="cursor-pointer"
              disabled={blocked}
              title="Starts the stack and keeps it in sync with your edits (docker compose watch). Needs Docker Compose 2.22 or newer."
              onClick={() => void create(true, true)}
            >
              {busy && <Spinner className="size-3.5" />} Create and watch
            </Button>
          )}
        </div>
      }
    >
      <div className="@container flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="grid grid-cols-1 gap-3 @xl:grid-cols-[1fr_auto]">
          <label className="flex min-w-0 flex-col gap-1.5 text-sm">
            <span className="font-medium">Project</span>
            <Select value={id} onChange={(e) => setId(e.target.value)} placeholder="Choose a project…" className="min-w-0">
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} — {p.path}
                </option>
              ))}
            </Select>
          </label>
          <div className="flex flex-col gap-1.5 text-sm">
            <span className="font-medium">Mode</span>
            <div className="flex gap-1" role="radiogroup" aria-label="Mode">
              <Button
                size="sm"
                variant={mode === 'dev' ? 'default' : 'outline'}
                className="cursor-pointer"
                role="radio"
                aria-checked={mode === 'dev'}
                title="Syncs your edits into the container (docker compose watch) and runs the dev server."
                onClick={() => {
                  setMode('dev')
                  setChoice(null)
                }}
              >
                Development
              </Button>
              <Button
                size="sm"
                variant={mode === 'prod' ? 'default' : 'outline'}
                className="cursor-pointer"
                role="radio"
                aria-checked={mode === 'prod'}
                title="Multi-stage build, no dev packages, runs as a non-root user."
                onClick={() => {
                  setMode('prod')
                  setChoice(null)
                }}
              >
                Production
              </Button>
            </div>
          </div>
        </div>
        <ErrorCard error={planError} onDismiss={() => setPlanError(null)} />
        {id && !plan && !planError && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" /> Looking at the project…
          </p>
        )}
        {plan && (
          <>
            <div className="flex flex-col gap-1 text-sm">
              <span className="flex items-center gap-2 font-medium">
                What OLS found {loading && <Spinner className="size-3.5" />}
              </span>
              <ul className="list-disc pl-5 text-muted-foreground">
                {plan.detected.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            </div>
            <div className="flex flex-col gap-1.5 text-sm">
              <span className="font-medium">Services</span>
              <p className="text-xs text-muted-foreground">
                Real images, pinned to a version. Tick what your project needs; OLS fills in the connection settings for the app.
              </p>
              <ul className="flex flex-col gap-1">
                {plan.services.map((s) => (
                  <li key={s.id} className="flex items-start gap-2">
                    <Checkbox checked={s.selected} onChange={(v) => toggle(s.id, v)} label={s.name} className="mt-0.5" />
                    <div className="min-w-0">
                      <span className="font-medium">{s.name}</span>{' '}
                      <span className="font-mono text-xs text-muted-foreground">{s.image}</span>
                      {s.detected && <span className="ml-2 text-xs text-emerald-600 dark:text-emerald-400">found: {s.reason}</span>}
                      {!s.detected && s.optional && <span className="ml-2 text-xs text-muted-foreground">optional</span>}
                    </div>
                  </li>
                ))}
              </ul>
              {plan.secrets.length > 0 && (
                <p className="flex items-start gap-2 text-xs text-muted-foreground">
                  <KeyRound className="mt-0.5 size-3.5 shrink-0" />
                  {plan.secrets.length} password{plan.secrets.length === 1 ? '' : 's'} will be generated and kept in the Windows credential store. They are never written to the files.
                </p>
              )}
            </div>
            {plan.workers.length > 0 && (
              <p className="text-sm">
                <span className="font-medium">Extra processes from the same image: </span>
                {plan.workers.join(', ')}
              </p>
            )}
            <div className="flex gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
              <p>{plan.disclaimer}</p>
            </div>
            <div className="flex flex-col gap-1 text-sm">
              <span className="font-medium">OLS cannot see these — check them yourself</span>
              <ul className="list-disc pl-5 text-muted-foreground">
                {plan.checklist.map((c) => (
                  <li key={c}>{c}</li>
                ))}
              </ul>
            </div>
            {plan.php_extensions.length > 0 && (
              <p className="text-sm">
                <span className="font-medium">PHP extensions that will be installed: </span>
                {plan.php_extensions.join(', ')}
              </p>
            )}
            {plan.unknown_extensions.length > 0 && (
              <p className="text-sm">
                <span className="font-medium">Needs manual setup: </span>
                {plan.unknown_extensions.join(', ')}
              </p>
            )}
            {plan.notes.length > 0 && (
              <ul className="list-disc pl-5 text-sm text-muted-foreground">
                {plan.notes.map((n) => (
                  <li key={n}>{n}</li>
                ))}
              </ul>
            )}
            {plan.existing.length > 0 && (
              <p className="text-sm text-muted-foreground">
                Kept as they are: {plan.existing.filter((f) => f === 'Dockerfile').join(', ') || 'nothing'}.
              </p>
            )}
            {composeExists && (
              <label className="flex items-center gap-2 text-sm">
                <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} />
                Replace the existing compose file (the old one is backed up)
              </label>
            )}
            <div className="grid grid-cols-1 gap-3 @3xl:grid-cols-2">
              {plan.dockerfile && (
                <div className="flex min-w-0 flex-col gap-1">
                  <span className="text-sm font-medium">Dockerfile</span>
                  <CodeEditor value={plan.dockerfile} language="text" readOnly height="300px" />
                </div>
              )}
              <div className="flex min-w-0 flex-col gap-1">
                <span className="text-sm font-medium">compose.yaml (host port {plan.port})</span>
                <CodeEditor value={plan.compose} language="yaml" readOnly height="300px" />
              </div>
            </div>
            {plan.dockerignore && (
              <div className="flex min-w-0 flex-col gap-1">
                <span className="text-sm font-medium">.dockerignore</span>
                <CodeEditor value={plan.dockerignore} language="text" readOnly height="140px" />
              </div>
            )}
          </>
        )}
      </div>
    </Dialog>
  )
}
