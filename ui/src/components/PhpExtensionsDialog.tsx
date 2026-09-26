import { Download, Loader2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { type Diagnostic, type PhpExtensions, runCommand } from '@/core'

/** Turn a PHP version's extensions on/off and download new ones from PECL. */
export function PhpExtensionsDialog({ version, onClose }: { version: string | null; onClose: () => void }) {
  const [report, setReport] = useState<PhpExtensions | null>(null)
  const [pecl, setPecl] = useState<string[]>([])
  const [filter, setFilter] = useState('')
  const [installName, setInstallName] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  // Mounted with key={version}, so state starts fresh for each version.
  useEffect(() => {
    if (!version) return
    void runCommand({ type: 'list_php_extensions', version }).then((res) => {
      if (res.type === 'php_extensions') setReport(res.report)
    })
    // The PECL list is only a typing aid; offline just means no suggestions.
    void runCommand({ type: 'list_pecl_packages' })
      .then((res) => res.type === 'names' && setPecl(res.names))
      .catch(() => {})
  }, [version])

  async function act(key: string, fn: () => ReturnType<typeof runCommand>, done?: string) {
    setBusy(key)
    setError(null)
    setNotice(null)
    try {
      const res = await fn()
      if (res.type === 'php_extensions') setReport(res.report)
      if (done) setNotice(done)
    } catch (err) {
      const d = err as Diagnostic
      setError(d.cause || d.problem)
    } finally {
      setBusy(null)
    }
  }

  const toggle = (name: string, enabled: boolean) =>
    act(name, () => runCommand({ type: 'set_php_extension', version: version!, name, enabled }))

  const install = () => {
    const name = installName.trim().toLowerCase()
    if (!name) return
    void act('install', () => runCommand({ type: 'install_php_extension', version: version!, name }), `Installed and enabled ${name}.`).then(() =>
      setInstallName(''),
    )
  }

  const shown = useMemo(
    () => (report?.extensions ?? []).filter((e) => e.name.includes(filter.trim().toLowerCase())),
    [report, filter],
  )
  const enabledCount = report?.extensions.filter((e) => e.enabled).length ?? 0

  return (
    <Dialog
      open={version !== null}
      layer="top"
      onClose={onClose}
      title={`PHP ${version ?? ''} extensions`}
      description={
        report
          ? `${enabledCount} of ${report.extensions.length} enabled · ${report.thread_safe ? 'thread-safe' : 'non-thread-safe'} build. Changes restart this version's PHP workers.`
          : 'Loading…'
      }
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5 rounded-lg border border-border p-3">
          <span className="text-sm font-medium">Download from PECL</span>
          <div className="flex gap-2">
            <Input
              list="pecl-packages"
              value={installName}
              onChange={(e) => setInstallName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && install()}
              placeholder="e.g. redis, xdebug, apcu, imagick"
              className="h-8"
              disabled={busy !== null}
            />
            <datalist id="pecl-packages">
              {pecl.map((n) => (
                <option key={n} value={n} />
              ))}
            </datalist>
            <Button size="sm" onClick={install} disabled={busy !== null || !installName.trim()}>
              {busy === 'install' ? <Loader2 className="animate-spin" /> : <Download />} Install
            </Button>
          </div>
          <span className="text-xs text-muted-foreground">Picks the newest Windows build matching this PHP version.</span>
        </div>

        {notice && <p className="text-sm text-muted-foreground">{notice}</p>}
        {error && <p className="text-sm text-destructive">{error}</p>}

        <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter extensions" className="h-8" />
        <div className="grid grid-cols-2 gap-x-4 gap-y-2 sm:grid-cols-3">
          {shown.map((e) => (
            <div key={e.name} className="flex min-w-0 items-center justify-between gap-2 rounded-md py-0.5">
              <span className="flex min-w-0 items-center gap-1.5 truncate text-sm" title={e.name}>
                <span className="truncate">{e.name}</span>
                {e.downloaded && (
                  <Badge variant="secondary" className="h-4 shrink-0 px-1 text-[10px]">
                    PECL
                  </Badge>
                )}
              </span>
              <span className="flex shrink-0 items-center gap-1.5">
                <Switch
                  checked={e.enabled}
                  disabled={busy !== null}
                  onChange={(v) => void toggle(e.name, v)}
                  label={e.name}
                />
                <span className="flex size-3 items-center justify-center">
                  {busy === e.name && <Loader2 className="size-3 animate-spin text-muted-foreground" />}
                </span>
              </span>
            </div>
          ))}
        </div>
        {report && shown.length === 0 && <p className="text-sm text-muted-foreground">No extensions match.</p>}
      </div>
    </Dialog>
  )
}
