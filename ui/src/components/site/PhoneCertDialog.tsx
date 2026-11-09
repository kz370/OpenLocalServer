import { Copy } from 'lucide-react'

import { PhoneShareIcon } from '@/components/PhoneShareIcon'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { FormSection, SwitchRow } from '@/components/ui/form'
import { type Diagnostic, type LanInfo, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'
import { confirmAction } from '@/lib/confirm'

/** An SVG string from the core as an image. Never injected as HTML. */
function Qr({ svg, label, small }: { svg: string; label: string; small?: boolean }) {
  return <img alt={label} className={`${small ? 'size-28' : 'size-44'} shrink-0 rounded-md bg-white p-1`} src={`data:image/svg+xml;base64,${btoa(svg)}`} />
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

/** One phone link: a QR code, the address and a copy button. */
function LinkCard({ title, hint, url, qr }: { title: string; hint: string; url: string; qr: string | null }) {
  return (
    <div className="flex min-w-0 flex-1 basis-72 items-center gap-3 rounded-md border border-border p-2.5">
      {qr && <Qr svg={qr} label={`QR code for ${url}`} small />}
      <div className="min-w-0 flex-1">
        <p className="text-xs font-medium">{title}</p>
        <p className="mt-0.5 break-all font-mono text-[11px]">{url}</p>
        <p className="mt-0.5 text-[11px] text-muted-foreground">{hint}</p>
        <Button size="sm" variant="ghost" className="-ml-2 mt-0.5 h-6 px-2 text-xs" onClick={() => void navigator.clipboard.writeText(url)}>
          <Copy className="size-3" /> Copy
        </Button>
      </div>
    </div>
  )
}

/** The phone section's frame: a titled form section in site settings, bare inside the sharing window. */
function PhoneWrap({ bare, children }: { bare?: boolean; children: React.ReactNode }) {
  return bare ? <div>{children}</div> : <FormSection title="Open on my phone" hint="Reach this site from a phone on the same Wi-Fi. No internet needed.">{children}</FormSection>
}

/** Site settings section: a LAN address and QR for the site, and the certificate helper. */
export function PhoneSection({ hostname, onChanged, bare }: { hostname: string; onChanged?: () => void; bare?: boolean }) {
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
    // Sharing opens the site to every device on the Wi-Fi, so both directions ask first.
    const ok = on
      ? await confirmAction(`Share ${hostname} on your network? Any device on the same Wi-Fi can open it while sharing is on.`, 'Share on my network', 'Share', 'amber')
      : await confirmAction(`Stop sharing ${hostname}? Phones on your Wi-Fi can no longer open it.`, 'Stop sharing', 'Stop sharing')
    if (!ok) return
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'set_site_lan', hostname, on })
      if (r.type === 'lan_info') setInfo(r.info)
      onChanged?.()
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(false)
    }
  }

  async function setHttp(http: boolean) {
    setBusy(true)
    setError(null)
    try {
      const r = await runCommand({ type: 'set_site_lan_http', hostname, http })
      if (r.type === 'lan_info') setInfo(r.info)
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
    <PhoneWrap bare={bare}>
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-col gap-2.5">
        <SwitchRow checked={info?.on ?? false} disabled={busy || !info} onChange={(v) => void toggle(v)} title="Share this site on my network" hint="Phones on the same Wi-Fi can open it. No internet needed." />
        {info?.on && (
          <SwitchRow
            checked={info.https ?? false}
            disabled={busy || !info.can_https}
            onChange={(v) => void setHttp(!v)}
            title="Use HTTPS"
            hint={
              info.can_https
                ? 'On: install the OLS certificate on the phone once. Off: plain HTTP, nothing to install.'
                : 'This site has no HTTPS, so the phone link is plain HTTP.'
            }
          />
        )}
        {info?.on && info.url && (
          <div className="flex flex-col gap-2.5">
            <div className="flex flex-wrap gap-3">
              {info.local_url && (
                <LinkCard
                  title={`By name: ${info.local_name}`}
                  hint="iPhone, Mac, Windows, newer Android."
                  url={info.local_url}
                  qr={info.local_qr_svg ?? null}
                />
              )}
              <LinkCard
                title={info.local_url ? 'By address (if the name does not open)' : 'By address'}
                hint="Works on every phone."
                url={info.url}
                qr={info.qr_svg}
              />
            </div>
            {!info.local_url && (
              <p className="rounded-md border border-border bg-muted/30 p-2 text-xs text-muted-foreground">
                Phones open a site by name only for <code>.local</code> domains (like <code>shop.local</code>). <code>{hostname}</code> is opened by address and port.
              </p>
            )}
            {info.https && (
              <Button size="sm" variant="secondary" className="h-7 w-fit px-2 text-xs" onClick={() => setCertOpen(true)}>
                <PhoneShareIcon className="size-3.5" /> Install certificate on phone…
              </Button>
            )}
          </div>
        )}
        {info?.warnings.map((w) => (
          <p key={w} className="text-xs text-warning">
            {w}
          </p>
        ))}
        {info?.on && (
          <p className="text-[11px] text-muted-foreground">
            Same Wi-Fi as this computer. Apps that redirect to their own domain (WordPress Site URL, Laravel APP_URL) will leave the phone link.
          </p>
        )}
      </div>
    </PhoneWrap>
    <PhoneCertDialog open={certOpen} onClose={() => setCertOpen(false)} />
    </>
  )
}

/**
 * The dedicated sharing window: one site's phone links, QR codes, HTTPS switch and certificate
 * helper. Opened from the Sites page (row button, menu, status link); the same controls also
 * live in the site's settings, Domains tab.
 */
export function PhoneShareDialog({ hostname, onClose, onChanged }: { hostname: string | null; onClose: () => void; onChanged?: () => void }) {
  return (
    <Dialog open={hostname !== null} onClose={onClose} title="Share on my network" description={hostname ?? undefined} size="default">
      {hostname && <PhoneSection key={hostname} hostname={hostname} onChanged={onChanged} bare />}
    </Dialog>
  )
}
