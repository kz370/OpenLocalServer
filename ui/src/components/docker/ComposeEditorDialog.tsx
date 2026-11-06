import { AlertTriangle, CheckCircle2, CircleX, FileCode2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type ComposeCheck, type Diagnostic, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'

/**
 * §166: edit a compose file. "Check" runs OLS's own rules and then Docker's parser
 * (`docker compose config`) on the unsaved text; "Save" runs the same rules and keeps a
 * backup of the old file in OLS's data folder.
 */
export function ComposeEditorDialog({
  path,
  onClose,
  onSaved,
}: {
  path: string | null
  onClose: () => void
  onSaved: () => void
}) {
  const [text, setText] = useState('')
  const [saved, setSaved] = useState('')
  // Remounted per file (the caller keys it by path), so it starts loading.
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<'check' | 'save' | null>(null)
  const [check, setCheck] = useState<ComposeCheck | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    if (!path) return
    let alive = true
    void runCommand({ type: 'read_docker_compose_file', path })
      .then((r) => {
        if (alive && r.type === 'docker_compose_file') {
          setText(r.content)
          setSaved(r.content)
          setCheck(null)
          setError(null)
        }
      })
      .catch((err) => alive && setError(asDiagnostic(err)))
      .finally(() => alive && setLoading(false))
    return () => {
      alive = false
    }
  }, [path])

  const dirty = text !== saved

  async function close() {
    if (dirty && !(await confirmAction('Your changes to the compose file are not saved.', 'Discard changes?', 'Discard'))) return
    onClose()
  }

  async function runCheck() {
    if (!path) return
    setBusy('check')
    setError(null)
    try {
      const r = await runCommand({ type: 'validate_docker_compose', path, content: text })
      if (r.type === 'docker_compose_check') setCheck(r.check)
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(null)
    }
  }

  async function save() {
    if (!path) return
    setBusy('save')
    setError(null)
    try {
      await runCommand({ type: 'save_docker_compose_file', path, content: text })
      setSaved(text)
      notify.success('The previous version was backed up.', { title: 'Compose file saved' })
      onSaved()
    } catch (err) {
      setError(asDiagnostic(err))
      // Show what is wrong right under the editor.
      void runCheck()
    } finally {
      setBusy(null)
    }
  }

  return (
    <Dialog
      open={!!path}
      onClose={() => void close()}
      size="wide"
      icon={<FileCode2 className="size-5 text-sky-500" />}
      title="Edit compose file"
      description={path ?? undefined}
      footer={
        <div className="flex w-full items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">{dirty ? 'Unsaved changes' : 'Saved'}</span>
          <span className="flex gap-2">
            <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => void close()}>
              Close
            </Button>
            <Button size="sm" variant="outline" className="cursor-pointer" disabled={!!busy || loading} onClick={() => void runCheck()}>
              {busy === 'check' && <Spinner className="size-3.5" />} Check
            </Button>
            <Button size="sm" className="cursor-pointer" disabled={!!busy || loading || !dirty} onClick={() => void save()}>
              {busy === 'save' && <Spinner className="size-3.5" />} Save
            </Button>
          </span>
        </div>
      }
    >
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {loading ? (
        <div className="flex h-96 items-center justify-center text-sm text-muted-foreground">
          <Spinner className="mr-2 size-4" /> Opening…
        </div>
      ) : (
        <CodeEditor
          value={text}
          onChange={(v) => {
            setText(v)
            setCheck(null)
          }}
          language="yaml"
          height="420px"
        />
      )}
      {check && (
        <div className="mt-3 flex flex-col gap-1.5 text-sm">
          {check.ok && check.warnings.length === 0 && (
            <p className="flex items-center gap-2 text-emerald-600 dark:text-emerald-400">
              <CheckCircle2 className="size-4" /> Looks good{check.docker_checked ? ', and Docker accepts it.' : '. (Docker was not checked: the docker command was not found.)'}
            </p>
          )}
          {check.errors.map((e) => (
            <p key={e} className="flex items-start gap-2 text-destructive">
              <CircleX className="mt-0.5 size-4 shrink-0" /> <span className="whitespace-pre-wrap">{e}</span>
            </p>
          ))}
          {check.warnings.map((w) => (
            <p key={w} className="flex items-start gap-2 text-amber-600 dark:text-amber-400">
              <AlertTriangle className="mt-0.5 size-4 shrink-0" /> <span>{w}</span>
            </p>
          ))}
        </div>
      )}
    </Dialog>
  )
}
