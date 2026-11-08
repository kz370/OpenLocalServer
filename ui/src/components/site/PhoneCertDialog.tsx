import { Copy, Smartphone } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { FormSection, SwitchRow } from '@/components/ui/form'
import { type Diagnostic, type LanInfo, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'

/** An SVG string from the core as an image. Never injected as HTML. */
function Qr({ svg, label }: { svg: string; label: string }) {
  return <img alt={label} className="size-44 rounded-md bg-white p-1" src={`data:image/svg+xml;base64,${btoa(svg)}`} />
}

function useCountdown(ms: number | null) {
  const [left, setLeft] = useState(ms ?? 0)
  useEffect(() => {
    if (ms === null) return
    const end = Date.now() + ms
    setLeft(ms)
    const id = window.setInterval(() => setLeft(Math.max(0, end - Date.now())), 1000)
    return () => window.clearInterval(id)
  }, [ms])
  const s = Math.ceil(left / 1000)
  return { left, text: `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` }
}

/**
 * §178 Install the OLS certificate on a phone: a QR code for a temporary page on the local
 * network, the install steps and how to remove it again. Closing the dialog stops the page.
 */
export function PhoneCertDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [share, setShare] = useState<{ url: string; qr_svg: string; expires_ms: number } | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const clock = useCountdown(share?.expires_ms ?? null)

  useEffect(() => {
    if (!open) return
    let cancelled = false
    setError(null)
    void runCommand({ type: 'start_ca_share' })
      .then((r) => {
        if (!cancelled && r.type === 'ca_share') setShare({ url: r.url, qr_svg: r.qr_svg, expires_ms: r.expires_ms })
      })
      .catch((e) => !cancelled && setError(asDiagnostic(e)))
    return () => {
      cancelled = true
      void runCommand({ type: 'stop_ca_share' }).catch(() => undefined)
      setShare(null)
    }
  }, [open])

  return (
    <Dialog open={open} onClose={onClose} title="Install the certificate on your phone" description="Do this once per phone, on the same Wi-Fi as this computer." size="form">
      <div className="flex flex-col gap-4 text-sm">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <div className="rounded-md border border-warning/40 bg-warning/10 p-3 text-xs">
          Installing OLS's certificate makes the phone trust any site certificate OLS creates. Keep your computer safe, and remove the certificate from the phone when you are done testing (steps below). OLS never shares the certificate's private key.
        </div>
        {!share && !error && (
          <p className="flex items-center gap-2 text-muted-foreground">
            <Spinner /> Starting the page…
          </p>
        )}
        {share && (
          <div className="flex flex-wrap items-center gap-4">
            <Qr svg={share.qr_svg} label="QR code for the certificate page" />
            <div className="min-w-0 flex-1">
              <p className="break-all font-mono text-xs">{share.url}</p>
              <p className="mt-1 text-xs text-muted-foreground">{clock.left > 0 ? `The page closes in ${clock.text}.` : 'The page has closed. Close this window and open it again to get a new link.'}</p>
              <Button size="sm" variant="ghost" className="mt-1 h-7 px-2 text-xs" onClick={() => void navigator.clipboard.writeText(share.url)}>
                <Copy className="size-3.5" /> Copy link
              </Button>
            </div>
          </div>
        )}
        <div>
          <h4 className="font-medium">Android</h4>
          <p className="text-xs text-muted-foreground">Steps may differ by phone. Scan the code, download <code>ols-ca.crt</code>, then Settings → Security → More security settings → Encryption &amp; credentials → Install a certificate → <b>CA certificate</b>.</p>
          <p className="text-xs text-muted-foreground">Remove: the same place → Trusted credentials → User → OpenLocalServer Local CA → Remove.</p>
        </div>
        <div>
          <h4 className="font-medium">iPhone / iPad</h4>
          <p className="text-xs text-muted-foreground">Open the link <b>in Safari</b>, tap the profile, then Settings → Profile Downloaded → Install (iOS says "Not verified"; that is expected). Then Settings → General → About → <b>Certificate Trust Settings</b> → turn on OpenLocalServer Local CA.</p>
          <p className="text-xs text-muted-foreground">Remove: Settings → General → VPN &amp; Device Management → the profile → Remove Profile.</p>
        </div>
      </div>
    </Dialog>
  )
}

/** Site settings section: a LAN address and QR for the site, and the certificate helper. */
export function PhoneSection({ hostname }: { hostname: string }) {
  const [info, setInfo] = useState<LanInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [certOpen, setCertOpen] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  const load = useCallback(async () => {
    try {
      const r = await runCommand({ type: 'get_lan_info', hostname })
      if (r.type === 'lan_info') setInfo(r.info)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }, [hostname])
  useEffect(() => {
    void load()
  }, [load])

  async function toggle(on: boolean) {
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'set_site_lan', hostname, on })
      if (r.type === 'lan_info') setInfo(r.info)
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
    <FormSection title="Open on my phone" hint="Reach this site from a phone on the same Wi-Fi, with working HTTPS.">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-col gap-3">
        <SwitchRow checked={info?.on ?? false} disabled={busy || !info} onChange={(v) => void toggle(v)} title="Share this site on my network" hint="Adds a second name for the site that your phone can look up." />
        {info?.on && info.url && (
          <div className="flex flex-wrap items-center gap-4">
            {info.qr_svg && <Qr svg={info.qr_svg} label="QR code for the site address" />}
            <div className="min-w-0 flex-1">
              <p className="break-all font-mono text-xs">{info.url}</p>
              <Button size="sm" variant="ghost" className="mt-1 h-7 px-2 text-xs" onClick={() => void navigator.clipboard.writeText(info.url ?? '')}>
                <Copy className="size-3.5" /> Copy address
              </Button>
              <Button size="sm" variant="secondary" className="mt-1 ml-2 h-7 px-2 text-xs" onClick={() => setCertOpen(true)}>
                <Smartphone className="size-3.5" /> Install certificate on phone…
              </Button>
            </div>
          </div>
        )}
        {info?.warnings.map((w) => (
          <p key={w} className="text-xs text-warning">
            {w}
          </p>
        ))}
        {info?.on && (
          <p className="text-xs text-muted-foreground">
            The address (the site's name plus your computer's address) is looked up through a public DNS service, sslip.io. Some routers block such answers ("DNS rebinding protection"). If the page does not open, allow sslip.io in the router or Pi-hole, or use a Tunnel instead.
          </p>
        )}
      </div>
    </FormSection>
    <PhoneCertDialog open={certOpen} onClose={() => setCertOpen(false)} />
    </>
  )
}
