import { useEffect, useState } from 'react'
import { QrCode as QrIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { runCommand } from '@/core'

/** An SVG string from the core as an image. Never injected as HTML. */
export function Qr({ svg, label, className = 'size-52' }: { svg: string; label: string; className?: string }) {
  return <img alt={label} className={`${className} shrink-0 rounded-md bg-white p-1.5`} src={`data:image/svg+xml;base64,${btoa(svg)}`} />
}

/** A QR code for any text (asked from the core), centered. Shows nothing while it loads or if it fails. */
export function QrFor({ text, className }: { text: string; className?: string }) {
  const [svg, setSvg] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    setSvg(null)
    void runCommand({ type: 'make_qr', text })
      .then((r) => {
        if (!cancelled && r.type === 'qr' && r.svg) setSvg(r.svg)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [text])
  if (!svg) return null
  return (
    <div className="flex justify-center">
      <Qr svg={svg} label={`QR code for ${text}`} className={className} />
    </div>
  )
}

/** Button that toggles a tunnel address QR code (the parent renders <QrFor> when active). */
export function QrButton({ active, onClick }: { active: boolean; onClick: () => void }) {
  return (
    <Button type="button" size="icon" variant={active ? 'secondary' : 'ghost'} className="size-7 shrink-0" title={active ? 'Hide the QR code' : 'Show a QR code for this address'} aria-pressed={active} onClick={onClick}>
      <QrIcon className="size-3.5" />
    </Button>
  )
}
