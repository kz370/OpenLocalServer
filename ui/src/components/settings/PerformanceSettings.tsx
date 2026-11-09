import { AlertTriangle, Copy, ExternalLink, ShieldOff } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type DefenderState, type DevDriveInfo, type Diagnostic, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { asDiagnostic } from '@/lib/diagnostic'

interface Status {
  state: DefenderState
  excluded: string[]
  suggestions: string[]
  devDrive: DevDriveInfo[]
  otherAv: string[]
}

const DEV_DRIVE_DOCS = 'https://learn.microsoft.com/windows/dev-drive/'

/**
 * §173 Settings → Performance: optional Windows Defender exclusions for the folders OLS
 * manages, and a Dev Drive hint. Nothing is excluded without a click and a confirmation, and
 * OLS only lists and removes what it added itself.
 */
export function PerformanceSettings() {
  const [status, setStatus] = useState<Status | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const take = (r: Awaited<ReturnType<typeof runCommand>>) => {
    if (r.type === 'defender_status') setStatus({ state: r.state, excluded: r.excluded_by_ols, suggestions: r.suggestions, devDrive: r.dev_drive, otherAv: r.other_antivirus ?? [] })
  }

  const load = useCallback(async () => {
    try {
      take(await runCommand({ type: 'get_defender_status' }))
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  async function exclude(path: string) {
    const first = (status?.excluded.length ?? 0) === 0
    if (first) {
      const ok = await confirmAction(
        `Windows Defender will stop scanning ${path} in real time, so a malicious package downloaded there would not be caught. Only do this for folders with code you trust. Windows will ask for administrator permission if the OLS helper service is not installed.`,
        'Exclude from Defender?',
        'Exclude',
        'amber',
      )
      if (!ok) return
    }
    setBusy(path)
    setError(null)
    try {
      take(await runCommand({ type: 'add_defender_exclusion', path }))
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(null)
    }
  }

  async function removeExclusion(path: string) {
    setBusy(path)
    setError(null)
    try {
      take(await runCommand({ type: 'remove_defender_exclusion', path }))
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(null)
    }
  }

  const isExcluded = (p: string) => status?.excluded.some((e) => e.toLowerCase() === p.toLowerCase()) ?? false

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Faster installs on Windows</CardTitle>
        <CardDescription>Your antivirus scans every file that npm and Composer write. Excluding your project folders can make installs much faster.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {status === null ? (
          <p className="flex items-center gap-2 text-muted-foreground">
            <Spinner className="size-4" /> Asking Windows Defender…
          </p>
        ) : status.otherAv.length > 0 && !(status.state.available && status.state.realtime_on) ? (
          <OtherAntivirus folders={status.suggestions} />
        ) : !status.state.available ? (
          <p className="text-muted-foreground">Defender is not active or is managed by your organization. Nothing to do here.</p>
        ) : (
          <>
            <p>
              Real-time protection is <strong>{status.state.realtime_on ? 'on' : 'off'}</strong>.
            </p>
            <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs">
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
              <p>
                <strong>Security trade-off.</strong> Defender does not scan excluded folders in real time. Malware downloaded into an excluded folder (for example a malicious npm package) is not caught there. Only exclude folders with code you trust. OLS never excludes anything without asking, and lists and removes only what it added.
              </p>
            </div>
            <div className="flex flex-col divide-y rounded-md border">
              {status.suggestions.length === 0 && <p className="p-3 text-muted-foreground">No folders to suggest yet.</p>}
              {status.suggestions.map((p) => (
                <div key={p} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                  <span className="min-w-0 break-all font-mono text-xs">{p}</span>
                  {isExcluded(p) ? (
                    <span className="flex items-center gap-2">
                      <Badge variant="secondary">excluded</Badge>
                      <Button size="sm" variant="ghost" className="cursor-pointer" disabled={busy !== null} onClick={() => void removeExclusion(p)}>
                        Remove exclusion
                      </Button>
                    </span>
                  ) : (
                    <Button size="sm" variant="secondary" className="cursor-pointer" disabled={busy !== null || !status.state.realtime_on} onClick={() => void exclude(p)}>
                      {busy === p ? <Spinner /> : <ShieldOff />} Exclude
                    </Button>
                  )}
                </div>
              ))}
            </div>
            {status.devDrive.length > 0 && (
              <div className="flex flex-col gap-1">
                <p className="font-medium">Dev Drive</p>
                {status.devDrive.map((d) => (
                  <p key={d.path} className="text-xs text-muted-foreground">
                    {d.path} is {d.file_system || 'unknown'}.{' '}
                    {d.is_dev_drive
                      ? 'This is a ReFS drive, which is a good place for projects.'
                      : 'A Dev Drive (ReFS) is a Windows volume tuned for developer workloads and scanned in a lighter way. OLS never creates or formats drives for you.'}
                  </p>
                ))}
                <button
                  type="button"
                  className="inline-flex w-fit cursor-pointer items-center gap-1 text-xs text-primary hover:underline"
                  onClick={() => runCommand({ type: 'open_url', url: DEV_DRIVE_DOCS }).catch((e) => setError(asDiagnostic(e)))}
                >
                  Learn about Dev Drive <ExternalLink className="size-3" />
                </button>
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}

/**
 * Another antivirus is doing the scanning instead of Defender. OLS cannot change its settings,
 * so it lists the folders for the user to add to that app's exclusions. Product names are not
 * shown on purpose.
 */
function OtherAntivirus({ folders }: { folders: string[] }) {
  return (
    <div className="flex flex-col gap-3">
      <p>
        Your PC uses another antivirus app instead of Windows Defender. OLS cannot change its settings. To make installs faster, open your antivirus app and add the folders below to its exclusions (sometimes called Exceptions or Allow list).
      </p>
      <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs">
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
        <p>
          <strong>Security trade-off.</strong> Excluded folders are not scanned in real time, so malware downloaded into them (for example a malicious npm package) is not caught there. Only exclude folders with code you trust.
        </p>
      </div>
      <div className="flex flex-col divide-y rounded-md border">
        {folders.length === 0 && <p className="p-3 text-muted-foreground">No folders to suggest yet.</p>}
        {folders.map((p) => (
          <div key={p} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
            <span className="min-w-0 break-all font-mono text-xs">{p}</span>
            <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => void navigator.clipboard.writeText(p)}>
              <Copy /> Copy
            </Button>
          </div>
        ))}
      </div>
      {folders.length > 1 && (
        <Button size="sm" variant="secondary" className="w-fit cursor-pointer" onClick={() => void navigator.clipboard.writeText(folders.join('\n'))}>
          <Copy /> Copy all folders
        </Button>
      )}
    </div>
  )
}
