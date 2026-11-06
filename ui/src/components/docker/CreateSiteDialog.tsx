import { Globe } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { type Diagnostic, type DockerContainer, runCommand } from '@/core'
import { notify } from '@/lib/notify'

/** Default local TLD, as the Sites settings store it (same fallback as the core). */
async function defaultTld(): Promise<string> {
  try {
    const r = await runCommand({ type: 'get_setting', key: 'domains.default_tld' })
    if (r.type === 'setting' && typeof r.value === 'string' && r.value.trim()) return r.value.trim().replace(/^\./, '')
  } catch {
    // fall through
  }
  return 'local'
}

function slug(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '') || 'app'
}

/**
 * §166: give a container a local domain with HTTPS. The site is a normal proxy site to
 * the container's published port; OLS moves it if the container comes back on another port.
 */
export function CreateSiteDialog({
  container,
  onClose,
  onCreated,
}: {
  container: DockerContainer | null
  onClose: () => void
  onCreated: () => void
}) {
  const ports = (container?.ports ?? []).filter((p) => p.host_port !== null && p.protocol === 'tcp')
  const [port, setPort] = useState<number | null>(null)
  const [hostname, setHostname] = useState('')
  const [https, setHttps] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    if (!container) return
    let alive = true
    const first = container.ports.find((p) => p.host_port !== null && p.protocol === 'tcp')
    void defaultTld().then((tld) => {
      if (!alive) return
      setPort(first?.container_port ?? null)
      setHostname(`${slug(container.compose_project ?? container.compose_service ?? container.name)}.${tld}`)
      setHttps(true)
      setError(null)
    })
    return () => {
      alive = false
    }
  }, [container])

  async function create() {
    if (!container || port === null) return
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({
        type: 'docker_create_site',
        request: { container: container.name, container_port: port, hostname, https },
      })
      if (r.type !== 'domain') return
      const site = r.domain.hostname
      onCreated()
      onClose()
      // Same as any new site: apply the web config in the background.
      void runCommand({ type: 'apply_web', overwrite: [] })
        .then(() =>
          notify.success(`${site} now points at ${container.name}.`, {
            title: 'Site created',
            action: {
              label: 'Open',
              onClick: () => void runCommand({ type: 'open_url', url: `${https ? 'https' : 'http'}://${site}` }),
            },
          }),
        )
        .catch((err) =>
          notify.error(asDiagnostic(err).problem, { title: `${site} was created, but the web server was not updated` }),
        )
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open={!!container}
      onClose={onClose}
      size="form"
      icon={<Globe className="size-5 text-sky-500" />}
      title={`Create a site for ${container?.name ?? ''}`}
      description="The site forwards to the container. If the container comes back on another port, OLS moves the site with it."
      footer={
        <div className="flex w-full justify-end gap-2">
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" className="cursor-pointer" disabled={busy || port === null || !hostname.trim()} onClick={() => void create()}>
            {busy && <Spinner className="size-3.5" />} Create site
          </Button>
        </div>
      }
    >
      <div className="flex flex-col gap-4">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        {ports.length === 0 ? (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/5 p-3 text-sm">
            This container publishes no ports to this computer, so a site cannot reach it. Add a port in its compose file
            (for example <code>ports: [&quot;127.0.0.1:8080:80&quot;]</code>), run <b>Up</b>, then try again.
          </p>
        ) : (
          <label className="flex flex-col gap-1.5 text-sm">
            <span className="font-medium">Container port</span>
            <Select value={port ?? ''} onChange={(e) => setPort(Number(e.target.value))}>
              {ports.map((p) => (
                <option key={p.container_port} value={p.container_port}>
                  {p.container_port} (this computer: {p.host_port})
                </option>
              ))}
            </Select>
          </label>
        )}
        <label className="flex flex-col gap-1.5 text-sm">
          <span className="font-medium">Domain</span>
          <Input value={hostname} onChange={(e) => setHostname(e.target.value)} placeholder="shop.local" />
        </label>
        <div className="flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-sm">
          <span>
            <span className="font-medium">HTTPS</span>
            <span className="block text-xs text-muted-foreground">Uses OLS&apos;s trusted local certificate.</span>
          </span>
          <Switch checked={https} onChange={setHttps} label="HTTPS" />
        </div>
      </div>
    </Dialog>
  )
}
