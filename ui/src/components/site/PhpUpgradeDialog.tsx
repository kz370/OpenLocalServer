import { ShieldCheck } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { TaskOutputDialog } from '@/components/docker/TaskOutputDialog'
import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type Diagnostic, type Domain, type PhpUpgradeReport, type UpgradeItem, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { asDiagnostic } from '@/lib/diagnostic'
import { notify } from '@/lib/notify'

/** Changes a PHP site's version through the same commands the site settings use. */
async function setPhpVersion(hostname: string, version: string | null) {
  const got = await runCommand({ type: 'get_domain', hostname })
  if (got.type !== 'domain') return
  const domain: Domain = { ...got.domain, kind: { type: 'php', version } }
  await runCommand({ type: 'update_domain', domain })
  await runCommand({ type: 'apply_web', overwrite: [] })
}

/**
 * §171 PHP upgrade check: runs the check as a background task, then shows what would break
 * (blockers, warnings, notes) and offers to switch the site, with an Undo toast to go back.
 */
export function PhpUpgradeDialog({
  hostname,
  from,
  target,
  onClose,
  onSwitched,
}: {
  hostname: string
  /** The version the site has now (null = it follows the default). */
  from: string | null
  target: string
  onClose: () => void
  onSwitched: () => void
}) {
  const [taskId, setTaskId] = useState<string | null>(null)
  const [running, setRunning] = useState(true)
  const [report, setReport] = useState<PhpUpgradeReport | null>(null)
  const [root, setRoot] = useState('')
  const [error, setError] = useState<Diagnostic | null>(null)
  const [switching, setSwitching] = useState(false)
  const started = useRef(false)

  useEffect(() => {
    if (started.current) return
    started.current = true
    void (async () => {
      try {
        const d = await runCommand({ type: 'get_domain', hostname })
        if (d.type === 'domain') setRoot(d.domain.root)
        const r = await runCommand({ type: 'check_php_upgrade', hostname, target })
        if (r.type === 'task_started') setTaskId(r.id)
      } catch (e) {
        setError(asDiagnostic(e))
        setRunning(false)
      }
    })()
  }, [hostname, target])

  async function finished(task: { state: string }) {
    setRunning(false)
    setTaskId(null)
    if (task.state !== 'succeeded') return
    try {
      const r = await runCommand({ type: 'get_php_upgrade_report', hostname })
      if (r.type === 'php_upgrade_report') setReport(r.report)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }

  async function switchNow(force: boolean) {
    if (!report) return
    if (force && !(await confirmAction(`This site has problems with PHP ${target}. Switch anyway? You can switch back right after.`, 'Switch anyway?', 'Switch anyway'))) return
    setSwitching(true)
    try {
      await setPhpVersion(hostname, target)
      onSwitched()
      notify.success(`${hostname} now uses PHP ${target}.`, {
        action: {
          label: `Switch back to ${from ?? 'the default'}`,
          isUndo: true,
          onClick: async () => {
            await setPhpVersion(hostname, from)
            onSwitched()
            notify.info(`${hostname} is back on ${from ? `PHP ${from}` : 'the default PHP'}.`)
          },
        },
      })
      onClose()
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setSwitching(false)
    }
  }

  const count = (level: string) => report?.items.filter((i) => i.level === level).length ?? 0
  const openFile = (it: UpgradeItem) => {
    if (it.file && root) void runCommand({ type: 'open_in_editor', path: `${root}\\${it.file.replace(/\//g, '\\')}` })
  }

  return (
    <>
      <Dialog
        open={!running}
        onClose={onClose}
        size="wide"
        icon={<ShieldCheck className="size-5 text-emerald-500" />}
        title={`${hostname} on PHP ${target}`}
        description={report ? `${count('blocker')} blockers, ${count('warning')} warnings, ${report.files_checked.toLocaleString()} files checked${report.truncated ? ' (stopped at the limit)' : ''}` : 'The check did not produce a report.'}
        footer={
          <div className="flex w-full flex-wrap justify-end gap-2">
            <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
              Close
            </Button>
            {report && (
              <Button size="sm" disabled={switching} variant={report.can_switch ? 'default' : 'secondary'} className="cursor-pointer" onClick={() => void switchNow(!report.can_switch)}>
                {report.can_switch ? `Switch to ${target}` : 'Switch anyway'}
              </Button>
            )}
          </div>
        }
      >
        <div className="flex flex-col gap-3 text-sm">
          <ErrorCard error={error} onDismiss={() => setError(null)} />
          {report && report.items.length === 0 && <p className="text-muted-foreground">Nothing found. This site should run on PHP {target}.</p>}
          <div className="flex max-h-96 flex-col gap-1 overflow-y-auto">
            {report?.items.map((it, i) => (
              <div key={i} className="flex items-start gap-2 rounded-md border px-3 py-1.5">
                <Badge variant={it.level === 'blocker' ? 'destructive' : 'secondary'}>{it.level}</Badge>
                <div className="min-w-0 flex-1">
                  <p className="break-words">{it.title}</p>
                  {it.detail && <p className="text-xs break-words text-muted-foreground">{it.detail}</p>}
                  {it.file && (
                    <button type="button" className="cursor-pointer font-mono text-xs text-primary hover:underline" onClick={() => openFile(it)}>
                      {it.file}
                      {it.line ? `:${it.line}` : ''}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">OLS only reads your files. For a deeper check, add phpcompatibility/php-compatibility to the project as a dev dependency.</p>
        </div>
      </Dialog>
      <TaskOutputDialog taskId={taskId} onClose={onClose} onFinished={(t) => void finished(t)} />
    </>
  )
}
