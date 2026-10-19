import { open } from '@tauri-apps/plugin-dialog'
import { Check, Image as ImageIcon, Search, Trash2, Upload } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { FA_ICONS, type FaIconDef, SiteLogo } from '@/components/SiteLogo'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { type DomainSummary, type Project, runCommand } from '@/core'

export interface SiteLogoTarget {
  key: string
  site: DomainSummary | null
  project: Project | null
}

export function SiteLogoDialog({
  target,
  onClose,
  onUpdated,
}: {
  target: SiteLogoTarget | null
  onClose: () => void
  onUpdated: () => Promise<void> | void
}) {
  const [activeTab, setActiveTab] = useState<'photo' | 'fa'>('photo')
  const [search, setSearch] = useState('')
  const [category, setCategory] = useState<string>('all')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)

  if (!target) return null

  const { site: d, project: p } = target
  const hostname = d?.hostname ?? null
  const folder = d?.folder ?? p?.path ?? ''
  const displayName = d?.hostname ?? p?.name ?? 'Site'
  const currentIcon = d?.icon ?? null
  const currentImage = d?.icon_image ?? p?.icon_image ?? null

  const handlePickPhoto = async () => {
    setError(null)
    setSuccess(null)
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: 'Image Files',
            extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg', 'gif', 'bmp', 'ico'],
          },
        ],
      })

      if (!selected || typeof selected !== 'string') return

      setBusy(true)

      // Read file from disk via backend CoreCommand
      const res = await runCommand({
        type: 'read_image_file',
        path: selected,
      })

      if (res.type !== 'image_file' || !res.data_url) {
        throw new Error('Could not read image file.')
      }

      // Load image into an offscreen canvas and convert to standard WebP
      const img = new Image()
      img.crossOrigin = 'anonymous'
      img.src = res.data_url
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve()
        img.onerror = () => reject(new Error('Failed to decode image data.'))
      })

      // Resize if larger than 512px while maintaining aspect ratio
      const maxDim = 512
      let w = img.naturalWidth || img.width
      let h = img.naturalHeight || img.height
      if (w > maxDim || h > maxDim) {
        if (w > h) {
          h = Math.round((h * maxDim) / w)
          w = maxDim
        } else {
          w = Math.round((w * maxDim) / h)
          h = maxDim
        }
      }

      const canvas = document.createElement('canvas')
      canvas.width = Math.max(w, 16)
      canvas.height = Math.max(h, 16)
      const ctx = canvas.getContext('2d')
      if (!ctx) throw new Error('Could not create canvas context.')
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height)

      const webpDataUrl = canvas.toDataURL('image/webp', 0.92)

      // Send to backend to save into `<folder>/ols_site_icon.webp`
      await runCommand({
        type: 'set_site_icon_image',
        hostname: hostname ?? undefined,
        folder: folder || undefined,
        webp_base64: webpDataUrl,
      })

      setSuccess('Saved as ols_site_icon.webp in the project folder.')
      await onUpdated()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleRemovePhoto = async () => {
    setError(null)
    setSuccess(null)
    setBusy(true)
    try {
      await runCommand({
        type: 'remove_site_icon_image',
        hostname: hostname ?? undefined,
        folder: folder || undefined,
      })
      setSuccess('Removed ols_site_icon.webp from the project folder.')
      await onUpdated()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleSelectFaIcon = async (iconId: string) => {
    if (!hostname) {
      setError('Font Awesome icons require a site domain.')
      return
    }
    setError(null)
    setSuccess(null)
    setBusy(true)
    try {
      await runCommand({
        type: 'set_domain_icon',
        hostname,
        icon: iconId,
      })
      await onUpdated()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleClearFaIcon = async () => {
    if (!hostname) return
    setError(null)
    setSuccess(null)
    setBusy(true)
    try {
      await runCommand({
        type: 'set_domain_icon',
        hostname,
        icon: null,
      })
      await onUpdated()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const filteredIcons = FA_ICONS.filter((item: FaIconDef) => {
    if (category !== 'all' && item.category !== category) return false
    if (!search) return true
    const term = search.toLowerCase()
    return item.id.includes(term) || item.name.toLowerCase().includes(term)
  })

  return (
    <Dialog
      open
      onClose={onClose}
      title="Site Logo & Icon"
      description={`Configure the logo or icon for ${displayName}`}
      size="form"
      footer={
        <Button size="sm" variant="secondary" onClick={onClose}>
          Done
        </Button>
      }
    >
      <div className="flex flex-col gap-4">
        {error && <ErrorCard error={asDiagnostic(error)} onDismiss={() => setError(null)} />}
        {success && (
          <div className="flex items-center gap-2 rounded-md bg-emerald-500/10 px-3 py-2 text-xs text-emerald-500">
            <Check className="size-3.5" />
            <span>{success}</span>
          </div>
        )}

        {/* Live Preview Box */}
        <div className="flex flex-col gap-2 rounded-lg border bg-muted/20 p-3">
          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span>Sites Page Table Preview</span>
            {currentImage ? (
              <Badge variant="outline" className="border-emerald-500/40 bg-emerald-500/10 text-emerald-500 text-[10px]">
                Photo (ols_site_icon.webp)
              </Badge>
            ) : currentIcon ? (
              <Badge variant="outline" className="border-sky-500/40 bg-sky-500/10 text-sky-500 text-[10px]">
                Font Awesome ({currentIcon})
              </Badge>
            ) : (
              <Badge variant="outline" className="border-primary/40 bg-primary/10 text-primary text-[10px]">
                Default Internet Icon
              </Badge>
            )}
          </div>
          <div className="flex items-center rounded-md border bg-background/80 px-3 py-2">
            <div className="flex size-7 shrink-0 items-center justify-center">
              <SiteLogo icon={currentIcon} iconImage={currentImage} hostname={displayName} className="size-5" />
            </div>
            <div className="mx-3.5 h-8 w-px bg-border/80 shrink-0" aria-hidden="true" />
            <div className="flex flex-col">
              <span className="font-medium text-sm leading-tight">{displayName}</span>
              <span className="text-xs text-muted-foreground truncate">{folder || 'project folder'}</span>
            </div>
          </div>
        </div>

        {/* Tabs: Photo from PC vs Font Awesome */}
        <div className="flex border-b">
          <button
            type="button"
            className={`cursor-pointer px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
              activeTab === 'photo'
                ? 'border-primary text-primary'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            onClick={() => setActiveTab('photo')}
          >
            Photo from PC (ols_site_icon.webp)
          </button>
          <button
            type="button"
            className={`cursor-pointer px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
              activeTab === 'fa'
                ? 'border-primary text-primary'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            onClick={() => setActiveTab('fa')}
          >
            Font Awesome Icons (Offline)
          </button>
        </div>

        {/* Tab 1: Photo from PC */}
        {activeTab === 'photo' && (
          <div className="flex flex-col gap-4 py-2">
            <div className="rounded-md border border-dashed border-border p-4 text-center">
              <div className="mx-auto flex size-12 items-center justify-center rounded-full bg-muted/60 text-muted-foreground">
                <ImageIcon className="size-6" />
              </div>
              <h3 className="mt-2 text-sm font-medium">Pick a photo from your PC</h3>
              <p className="mt-1 text-xs text-muted-foreground">
                PNG, JPG, WEBP, SVG, GIF, or ICO. The image is converted into WebP format and saved into the project folder as{' '}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">ols_site_icon.webp</code>.
              </p>
              <div className="mt-4 flex flex-wrap items-center justify-center gap-2">
                <Button size="sm" onClick={() => void handlePickPhoto()} disabled={busy}>
                  <Upload className="size-3.5" />
                  {busy ? 'Processing…' : 'Choose photo from PC…'}
                </Button>
                {currentImage && (
                  <Button
                    size="sm"
                    variant="outline"
                    className="text-destructive hover:bg-destructive/10"
                    onClick={() => void handleRemovePhoto()}
                    disabled={busy}
                  >
                    <Trash2 className="size-3.5" />
                    Remove photo file
                  </Button>
                )}
              </div>
            </div>

            <div className="text-xs text-muted-foreground">
              <p>
                <strong>Destination:</strong>{' '}
                <span className="font-mono text-[11px]">{folder ? `${folder}\\ols_site_icon.webp` : 'Project root'}</span>
              </p>
              <p className="mt-1">
                If the file is deleted from the folder or not provided, the site automatically falls back to the configured Font Awesome icon or the default internet icon.
              </p>
            </div>
          </div>
        )}

        {/* Tab 2: Font Awesome Icons (Offline) */}
        {activeTab === 'fa' && (
          <div className="flex flex-col gap-3 py-1">
            <div className="flex flex-wrap items-center gap-2">
              <div className="relative min-w-40 flex-1">
                <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder="Search offline icons (e.g. wordpress, server, shop, rocket)…"
                  className="h-8 pl-8 text-xs"
                />
              </div>
              {currentIcon && (
                <Button size="sm" variant="ghost" className="h-8 text-xs" onClick={() => void handleClearFaIcon()} disabled={busy}>
                  Clear Font Awesome icon
                </Button>
              )}
            </div>

            {/* Category Filter Chips */}
            <div className="flex flex-wrap gap-1.5 text-xs">
              {[
                { id: 'all', label: 'All' },
                { id: 'brands', label: 'Brands & CMS' },
                { id: 'web', label: 'Web' },
                { id: 'tech', label: 'Tech & Dev' },
                { id: 'commerce', label: 'Commerce' },
                { id: 'media', label: 'Media' },
                { id: 'general', label: 'General' },
              ].map((c) => (
                <button
                  key={c.id}
                  type="button"
                  className={`cursor-pointer rounded-full px-2.5 py-0.5 text-[11px] font-medium transition ${
                    category === c.id
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
                  }`}
                  onClick={() => setCategory(c.id)}
                >
                  {c.label}
                </button>
              ))}
            </div>

            {/* Icons Grid */}
            <div className="grid max-h-56 grid-cols-4 gap-2 overflow-y-auto rounded-md border p-2 sm:grid-cols-6">
              {filteredIcons.map((item) => {
                const selected = currentIcon === item.id
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => void handleSelectFaIcon(item.id)}
                    className={`group flex cursor-pointer flex-col items-center justify-center rounded-lg border p-2 transition hover:bg-muted/70 ${
                      selected ? 'border-primary bg-primary/10 text-primary shadow-xs' : 'border-border/60 bg-background'
                    }`}
                    title={item.name}
                  >
                    <div className="flex size-6 items-center justify-center">
                      <SiteLogo icon={item.id} className="size-5" />
                    </div>
                    <span className="mt-1.5 w-full truncate text-center text-[10px] text-muted-foreground group-hover:text-foreground">
                      {item.name}
                    </span>
                  </button>
                )
              })}
              {filteredIcons.length === 0 && (
                <div className="col-span-full py-6 text-center text-xs text-muted-foreground">
                  No matching Font Awesome icons found.
                </div>
              )}
            </div>

            {currentImage && (
              <p className="text-[11px] text-muted-foreground">
                <em>Note:</em> A custom photo (<code className="font-mono">ols_site_icon.webp</code>) is currently present in the project folder and takes display priority over Font Awesome icons.
              </p>
            )}
          </div>
        )}
      </div>
    </Dialog>
  )
}
