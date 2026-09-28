import { Download } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input, NumberInput } from '@/components/ui/input'
import { type XdebugReport, type XdebugSettings, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'

const MODES: { id: string; label: string; hint: string }[] = [
  { id: 'debug', label: 'Step debugging', hint: 'Breakpoints in your IDE' },
  { id: 'develop', label: 'Develop', hint: 'Better var_dump, stack traces and error pages' },
  { id: 'coverage', label: 'Code coverage', hint: 'For test coverage reports' },
  { id: 'profile', label: 'Profiling', hint: 'Writes cachegrind files you open in QCachegrind / PhpStorm' },
  { id: 'trace', label: 'Tracing', hint: 'Writes a trace of every function call' },
  { id: 'gcstats', label: 'GC stats', hint: 'Garbage collector statistics' },
]

/** Xdebug for one PHP version (§13): turn it on, and set how it talks to the IDE. */
export function XdebugDialog({ version, onClose }: { version: string | null; onClose: () => void }) {
  const [report, setReport] = useState<XdebugReport | null>(null)
  const [draft, setDraft] = useState<XdebugSettings | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  // Mounted with key={version}, so state starts fresh for each version.
  useEffect(() => {
    if (!version) return
    void runCommand({ type: 'get_xdebug', version }).then((res) => {
      if (res.type === 'xdebug') {
        setReport(res.report)
        setDraft(res.report.settings)
      }
    })
  }, [version])

  if (!version) return null

  const apply = (res: Awaited<ReturnType<typeof runCommand>>) => {
    if (res.type === 'xdebug') {
      setReport(res.report)
      setDraft(res.report.settings)
    }
  }

  const setModes = (id: string, on: boolean) => {
    if (!draft) return
    const rest = draft.modes.filter((m) => m !== id && m !== 'off')
    setDraft({ ...draft, modes: on ? [...rest, id] : rest.length ? rest : ['off'] })
  }

  return (
    <Dialog
      open
      layer="top"
      onClose={onClose}
      title={`Xdebug for PHP ${version}`}
      description="Settings apply to every site on this PHP version. The PHP workers restart when you save."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
          <Button
            disabled={busy !== null || !draft || !report?.installed}
            onClick={() =>
              run('save', async () => {
                apply(await runCommand({ type: 'set_xdebug', version, settings: draft! }))
                setNotice('Saved. PHP was restarted with the new settings.')
              })
            }
          >
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {notice && <p className="text-sm text-success">{notice}</p>}

        {report && !report.installed && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border p-3 text-sm">
            <span>Xdebug is not installed for this PHP version.</span>
            <Button
              size="sm"
              disabled={busy !== null}
              onClick={() =>
                run('install', async () => {
                  await runCommand({ type: 'install_php_extension', version, name: 'xdebug' })
                  apply(await runCommand({ type: 'get_xdebug', version }))
                  setNotice('Xdebug downloaded and enabled.')
                })
              }
            >
              <Download /> {busy === 'install' ? 'Downloading…' : 'Download Xdebug'}
            </Button>
          </div>
        )}

        {report?.installed && (
          <div className="flex items-center justify-between gap-3">
            <Toggle
              checked={report.enabled}
              disabled={busy !== null}
              onChange={(enabled) =>
                run('toggle', async () => {
                  await runCommand({ type: 'set_php_extension', version, name: 'xdebug', enabled })
                  apply(await runCommand({ type: 'get_xdebug', version }))
                })
              }
              label="Xdebug is loaded"
              hint="Off means PHP runs at full speed with no Xdebug at all."
            />
            <Badge variant={report.enabled ? 'success' : 'secondary'}>{report.enabled ? 'on' : 'off'}</Badge>
          </div>
        )}

        {draft && report?.installed && (
          <>
            <div className="flex flex-col gap-2">
              <span className="text-xs font-medium text-muted-foreground">Modes</span>
              <div className="grid gap-2 sm:grid-cols-2">
                {MODES.map((m) => (
                  <Toggle key={m.id} checked={draft.modes.includes(m.id)} onChange={(on) => setModes(m.id, on)} label={m.label} hint={m.hint} />
                ))}
              </div>
            </div>

            <Field label="Start a session" hint="“On request” connects to the IDE only when the request has ?XDEBUG_TRIGGER=1 (or the browser extension's cookie), so the site stays fast.">
              <Select value={draft.start_with_request} onChange={(e) => setDraft({ ...draft, start_with_request: e.target.value as XdebugSettings['start_with_request'] })}>
                <option value="trigger">On request (trigger) — recommended</option>
                <option value="yes">Every request</option>
                <option value="default">Xdebug default for the mode</option>
                <option value="no">Never (settings only)</option>
              </Select>
            </Field>

            <div className="grid gap-3 sm:grid-cols-3">
              <Field label="IDE host">
                <Input value={draft.client_host} onChange={(e) => setDraft({ ...draft, client_host: e.target.value })} />
              </Field>
              <Field label="IDE port" hint="9003 is Xdebug 3's default">
                <NumberInput
                  label="IDE port"
                  min={1}
                  max={65535}
                  value={draft.client_port}
                  onChange={(n) => setDraft({ ...draft, client_port: n ?? 9003 })}
                />
              </Field>
              <Field label="IDE key">
                <Input value={draft.idekey} onChange={(e) => setDraft({ ...draft, idekey: e.target.value })} />
              </Field>
            </div>
          </>
        )}
      </div>
    </Dialog>
  )
}
