import { Copy, Link2, ShieldCheck } from 'lucide-react'
import { useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { type ConnectInfo, type Diagnostic, runCommand } from '@/core'
import { notify } from '@/lib/notify'

function copy(text: string, what: string) {
  void navigator.clipboard.writeText(text).then(() => notify.success(`${what} copied.`, { title: 'Copied' }))
}

/**
 * §166 "Connect to OLS": lets a stack's containers reach OLS sites by name, trust OLS's
 * HTTPS certificate, and use OLS's own services. The compose file is never changed: OLS
 * adds a generated file when it runs the stack. Keyed by path by the caller.
 */
export function ConnectDialog({ path, onClose, onChanged }: { path: string; onClose: () => void; onChanged: () => void }) {
  const [info, setInfo] = useState<ConnectInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    let alive = true
    void runCommand({ type: 'get_docker_compose_connect', path })
      .then((r) => alive && r.type === 'docker_compose_connect' && setInfo(r.info))
      .catch((err) => alive && setError(asDiagnostic(err)))
    return () => {
      alive = false
    }
  }, [path])

  async function toggle(enabled: boolean) {
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'set_docker_compose_connect', path, enabled })
      if (r.type === 'docker_compose_connect') {
        setInfo(r.info)
        onChanged()
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      size="wide"
      icon={<Link2 className="size-5 text-emerald-500" />}
      title="Connect to OLS"
      description="Let this stack's containers reach your OLS sites and services. Your compose file is not changed."
    >
      <div className="@container flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {!info ? (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner className="size-4" /> Reading the stack…
          </p>
        ) : (
          <>
            <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-3">
              <span className="text-sm">
                <span className="block font-medium">{info.enabled ? 'Connected' : 'Not connected'}</span>
                <span className="block text-xs text-muted-foreground">
                  Takes effect the next time OLS starts or restarts the stack (Up or Restart).
                </span>
              </span>
              <Switch checked={info.enabled} disabled={busy} onChange={(v) => void toggle(v)} label="Connect to OLS" />
            </div>

            <div className="grid grid-cols-1 gap-3 @2xl:grid-cols-2">
              <div className="flex flex-col gap-2 rounded-md border p-3 text-sm">
                <span className="font-medium">Your sites, by name</span>
                <span className="text-xs text-muted-foreground">
                  Containers can open these addresses, and they reach OLS on this computer.
                </span>
                <div className="flex max-h-28 flex-wrap gap-1 overflow-y-auto">
                  {info.hostnames.length === 0 ? (
                    <span className="text-xs text-muted-foreground">No sites yet.</span>
                  ) : (
                    info.hostnames.map((h) => (
                      <Badge key={h} variant="outline" className="font-mono">
                        {h}
                      </Badge>
                    ))
                  )}
                </div>
              </div>
              <div className="flex flex-col gap-2 rounded-md border p-3 text-sm">
                <span className="flex items-center gap-1.5 font-medium">
                  <ShieldCheck className="size-4 text-emerald-500" /> HTTPS certificate
                </span>
                {info.ca ? (
                  <span className="text-xs text-muted-foreground">
                    OLS&apos;s certificate (never its private key) is mounted read-only at{' '}
                    <code className="break-all">{info.ca_path_in_container}</code>. Node.js trusts it automatically
                    (<code>NODE_EXTRA_CA_CERTS</code>). For PHP, Python or curl, run <code>update-ca-certificates</code> in
                    your Dockerfile, or point the tool at <code>$OLS_CA_CERT</code>.
                  </span>
                ) : (
                  <span className="text-xs text-muted-foreground">
                    OLS has no local certificate yet. It is created the first time a site uses HTTPS.
                  </span>
                )}
              </div>
            </div>

            <div className="flex flex-col gap-2 text-sm">
              <span className="font-medium">OLS services from inside a container</span>
              {info.services.length === 0 ? (
                <span className="text-xs text-muted-foreground">No OLS services are installed.</span>
              ) : (
                <div className="flex flex-col divide-y rounded-md border">
                  {info.services.map((s) => (
                    <div key={s.id} className="flex flex-wrap items-center justify-between gap-2 px-3 py-2">
                      <span className="flex items-center gap-2">
                        <span className={s.running ? 'size-2 rounded-full bg-emerald-500' : 'size-2 rounded-full bg-muted-foreground/40'} />
                        <span className="font-medium">{s.name}</span>
                        {!s.running && <span className="text-xs text-muted-foreground">(stopped)</span>}
                      </span>
                      <span className="flex min-w-0 items-center gap-1">
                        <code className="truncate text-xs" title={s.connection ?? s.address ?? ''}>
                          {s.connection ?? s.address ?? 'no port'}
                        </code>
                        {(s.connection ?? s.address) && (
                          <Button
                            size="sm"
                            variant="ghost"
                            className="size-7 shrink-0 cursor-pointer p-0"
                            title="Copy"
                            onClick={() => copy((s.connection ?? s.address)!, s.name)}
                          >
                            <Copy className="size-3.5" />
                          </Button>
                        )}
                      </span>
                    </div>
                  ))}
                </div>
              )}
              <span className="text-xs text-muted-foreground">
                Database users must be allowed to connect from containers; if a login is refused, create a database user with a
                password on the Databases page.
              </span>
            </div>

            <details className="text-sm">
              <summary className="cursor-pointer font-medium">What OLS adds when it runs this stack</summary>
              <div className="mt-2">
                <CodeEditor value={info.override_yaml} language="yaml" readOnly height="200px" />
              </div>
            </details>
          </>
        )}
      </div>
    </Dialog>
  )
}
