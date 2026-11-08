import { AlertTriangle, Container } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Select } from '@/components/ui/select'
import { type Diagnostic, type DockerizePlan, type Project, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'
import { notify } from '@/lib/notify'

/**
 * 2.11.0: Dockerize a project. Shows what OLS detected, the fixed "starting point, not
 * guaranteed" notice and checklist (both come from the core, never typed here), and the exact
 * files before anything is written. Nothing is overwritten.
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
  /** The compose file that was written, and whether to start it. Without it, the dialog starts it itself. */
  onCreated?: (file: string, start: boolean) => void
}) {
  const [projects, setProjects] = useState<Project[]>([])
  const [id, setId] = useState('')
  const [plan, setPlan] = useState<DockerizePlan | null>(null)
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
    setId(projectId ?? '')
    void runCommand({ type: 'list_projects' })
      .then((r) => alive && r.type === 'projects' && setProjects(r.projects))
      .catch((err) => alive && setError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [open, projectId])

  useEffect(() => {
    if (!open || !id) return
    let alive = true
    setPlan(null)
    setPlanError(null)
    void runCommand({ type: 'plan_dockerize', project_id: id })
      .then((r) => alive && r.type === 'dockerize_plan' && setPlan(r.plan))
      .catch((err) => alive && setPlanError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [open, id])

  const composeExists = !!plan && plan.existing.some((f) => f !== 'Dockerfile')

  async function create(start: boolean) {
    if (!plan) return
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'create_dockerize', project_id: id, overwrite_compose: replace })
      if (r.type !== 'dockerize_plan') return
      notify.success("Files created. Review them before running — OLS can't guarantee they work for your project.")
      if (onCreated) onCreated(r.plan.compose_path, start)
      else if (start) {
        await runCommand({ type: 'docker_compose_action', path: r.plan.compose_path, action: 'up' })
        notify.success('Starting the stack. Follow it on the Processes page.')
      }
      onClose()
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  const blocked = busy || !plan || (composeExists && !replace)

  return (
    <Dialog
      open={open}
      onClose={onClose}
      size="wide"
      icon={<Container className="size-5 text-sky-500" />}
      title="Dockerize a project"
      description="Make a Dockerfile and a compose file from what OLS finds in the project. Nothing is overwritten."
      footer={
        <div className="flex w-full items-center justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" variant="outline" className="cursor-pointer" disabled={blocked} onClick={() => void create(false)}>
            Create files
          </Button>
          <Button size="sm" className="cursor-pointer" disabled={blocked} onClick={() => void create(true)}>
            {busy && <Spinner className="size-3.5" />} Create and start
          </Button>
        </div>
      }
    >
      <div className="@container flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
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
        <ErrorCard error={planError} onDismiss={() => setPlanError(null)} />
        {id && !plan && !planError && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" /> Looking at the project…
          </p>
        )}
        {plan && (
          <>
            <div className="flex flex-col gap-1 text-sm">
              <span className="font-medium">What OLS found</span>
              <ul className="list-disc pl-5 text-muted-foreground">
                {plan.detected.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            </div>
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
                  <CodeEditor value={plan.dockerfile} language="text" readOnly height="260px" />
                </div>
              )}
              <div className="flex min-w-0 flex-col gap-1">
                <span className="text-sm font-medium">compose.yaml (host port {plan.port})</span>
                <CodeEditor value={plan.compose} language="yaml" readOnly height="260px" />
              </div>
            </div>
          </>
        )}
      </div>
    </Dialog>
  )
}
