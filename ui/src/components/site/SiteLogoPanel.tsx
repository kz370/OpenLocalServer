import { open } from '@tauri-apps/plugin-dialog'
import { Check, Image as ImageIcon, Search, Trash2, Upload } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import {
  FA_ICONS,
  type FaIconDef,
  SiteLogo,
  buildIconSpec,
  parseIconSpec,
} from '@/components/SiteLogo'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { runCommand } from '@/core'

export interface SiteLogoPanelProps {
  hostname: string | null
  folder: string
  displayName: string
  initialIcon?: string | null
  initialImage?: string | null
  onUpdated?: () => Promise<void> | void
}

/**
 * Site logo & icon management panel.
 * Can be rendered on its own page inside Site Settings (SiteDialog)
 * or inside a standalone dialog (SiteLogoDialog).
 */
export function SiteLogoPanel({
  hostname,
  folder,
  displayName,
  initialIcon = null,
  initialImage = null,
  onUpdated,
}: SiteLogoPanelProps) {
  const [activeTab, setActiveTab] = useState<'photo' | 'fa'>('photo')
  const [currentIcon, setCurrentIcon] = useState<string | null>(initialIcon)
  const [currentImage, setCurrentImage] = useState<string | null>(initialImage)

  // Parsed Font Awesome state
  const initialParsed = parseIconSpec(initialIcon)
  const [activeFaName, setActiveFaName] = useState<string>(initialParsed.name)
  const [customColor, setCustomColor] = useState<string>(initialParsed.color ?? '')

  const [search, setSearch] = useState('')
  const [category, setCategory] = useState<string>('all')
  const [busy, setBusy] = useState(false)
  const [savedNotice, setSavedNotice] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Keep state in sync with incoming initial props
  useEffect(() => {
    setCurrentIcon(initialIcon)
    const p = parseIconSpec(initialIcon)
    setActiveFaName(p.name)
    if (p.color !== undefined) {
      setCustomColor(p.color)
    }
  }, [initialIcon])

  useEffect(() => {
    setCurrentImage(initialImage)
  }, [initialImage])

  // If no initial image is passed but a folder is known, check for existing ols_site_icon.webp
  useEffect(() => {
    if (!currentImage && folder) {
      const sep = folder.includes('/') ? '/' : '\\'
      const iconPath = folder.endsWith(sep) ? `${folder}ols_site_icon.webp` : `${folder}${sep}ols_site_icon.webp`
      runCommand({ type: 'read_image_file', path: iconPath })
        .then((res) => {
          if (res.type === 'image_file' && res.data_url) {
            setCurrentImage(res.data_url)
          }
        })
        .catch(() => {
          // File does not exist or cannot be read yet
        })
    }
  }, [folder])

  const flashSaved = () => {
    setSavedNotice(true)
    setTimeout(() => setSavedNotice(false), 2200)
  }

  const handlePickPhoto = async () => {
    setError(null)
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

      setCurrentImage(webpDataUrl)
      flashSaved()
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleRemovePhoto = async () => {
    setError(null)
    setBusy(true)
    try {
      await runCommand({
        type: 'remove_site_icon_image',
        hostname: hostname ?? undefined,
        folder: folder || undefined,
      })
      setCurrentImage(null)
      flashSaved()
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleSelectFaIcon = async (iconId: string, colorToApply?: string) => {
    if (!hostname) {
      setError('Font Awesome icons require a site domain.')
      return
    }
    const color = colorToApply !== undefined ? colorToApply : customColor
    const iconSpec = buildIconSpec(iconId, color)

    // Immediate optimistic update (zero glitch/layout shift)
    setCurrentIcon(iconSpec)
    setActiveFaName(iconId)
    flashSaved()

    try {
      await runCommand({
        type: 'set_domain_icon',
        hostname,
        icon: iconSpec,
      })
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const handleApplyColor = (newColor: string) => {
    setCustomColor(newColor)
    if (activeFaName) {
      void handleSelectFaIcon(activeFaName, newColor)
    }
  }

  const handleClearFaIcon = async () => {
    if (!hostname) return
    setError(null)

    // Optimistic update
    setCurrentIcon(null)
    setActiveFaName('')
    setCustomColor('')
    flashSaved()

    try {
      await runCommand({
        type: 'set_domain_icon',
        hostname,
        icon: null,
      })
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const filteredIcons = FA_ICONS.filter((item: FaIconDef) => {
    if (category !== 'all' && item.category !== category) return false
    if (!search) return true
    const term = search.toLowerCase()
    return item.id.includes(term) || item.name.toLowerCase().includes(term)
  })

  return (
    <div className="flex flex-col gap-4">
      {error && <ErrorCard error={asDiagnostic(error)} onDismiss={() => setError(null)} />}

      {/* Live Preview Box with zero layout shifting */}
      <div className="flex flex-col gap-2 rounded-lg border bg-muted/20 p-3">
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span className="font-medium">Sites Page Table Row Preview</span>
          <div className="flex items-center gap-2">
            {savedNotice && (
              <span className="flex items-center gap-1 text-[11px] font-medium text-emerald-500 animate-in fade-in duration-200">
                <Check className="size-3" /> Saved
              </span>
            )}
            {currentImage ? (
              <Badge variant="outline" className="border-emerald-500/40 bg-emerald-500/10 text-emerald-500 text-[10px]">
                Custom Photo (ols_site_icon.webp)
              </Badge>
            ) : currentIcon ? (
              <Badge variant="outline" className="border-sky-500/40 bg-sky-500/10 text-sky-500 text-[10px]">
                Font Awesome ({parseIconSpec(currentIcon).name})
              </Badge>
            ) : (
              <Badge variant="outline" className="border-[#00b4d8]/40 bg-[#00b4d8]/10 text-[#00b4d8] dark:text-[#06b6d4] text-[10px]">
                Default Internet Icon
              </Badge>
            )}
          </div>
        </div>
        <div className="flex items-center rounded-md border bg-background/80 px-3 py-2.5">
          <div className="flex size-8 shrink-0 items-center justify-center">
            <SiteLogo
              icon={currentIcon}
              iconImage={currentImage}
              hostname={displayName}
              className={currentImage ? 'size-8' : 'size-6'}
            />
          </div>
          <div className="mx-3.5 h-8 w-px bg-border/80 shrink-0" aria-hidden="true" />
          <div className="flex flex-col min-w-0 flex-1">
            <span className="font-medium text-sm leading-tight truncate">{displayName}</span>
            <span className="text-xs text-muted-foreground truncate">{folder || 'project folder'}</span>
          </div>
        </div>
      </div>

      {/* Tabs: Photo from PC vs Font Awesome */}
      <div className="flex border-b border-border">
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
              PNG, JPG, WEBP, SVG, GIF, or ICO. The image is converted to WebP format and saved directly into the project folder as{' '}
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

          <div className="text-xs text-muted-foreground space-y-1">
            <p>
              <strong>Destination path:</strong>{' '}
              <span className="font-mono text-[11px]">{folder ? `${folder}\\ols_site_icon.webp` : 'Project root'}</span>
            </p>
            <p>
              If the file is deleted from the folder or not provided, the site automatically falls back to the configured Font Awesome icon or the default internet globe icon.
            </p>
          </div>
        </div>
      )}

      {/* Tab 2: Font Awesome Icons (Offline) with Color Picker */}
      {activeTab === 'fa' && (
        <div className="flex flex-col gap-3 py-1">
          {/* Custom Color Bar (Name, Hex, RGBA) */}
          <div className="flex flex-col gap-2 rounded-md border border-border/70 bg-muted/20 p-2.5">
            <div className="flex items-center justify-between text-xs">
              <span className="font-medium text-foreground">Icon Color</span>
              <span className="text-[11px] text-muted-foreground">Name (e.g. crimson), Hex (#00b4d8), or RGBA</span>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {/* HTML5 color picker swatch button */}
              <div className="relative flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md border border-border bg-background shadow-xs overflow-hidden">
                <input
                  type="color"
                  value={customColor.startsWith('#') && customColor.length === 7 ? customColor : '#00b4d8'}
                  onChange={(e) => handleApplyColor(e.target.value)}
                  className="absolute inset-0 size-full cursor-pointer opacity-0"
                  title="Pick hex color with system color picker"
                />
                <div
                  className="size-5 rounded-full border border-black/10 shadow-xs"
                  style={{ backgroundColor: customColor || '#00b4d8' }}
                />
              </div>

              {/* Text input for CSS color names, hex, rgba */}
              <div className="min-w-36 flex-1">
                <Input
                  value={customColor}
                  onChange={(e) => {
                    const val = e.target.value
                    setCustomColor(val)
                    if (activeFaName) {
                      const spec = buildIconSpec(activeFaName, val)
                      setCurrentIcon(spec)
                    }
                  }}
                  onBlur={() => {
                    if (activeFaName) void handleApplyColor(customColor)
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && activeFaName) void handleApplyColor(customColor)
                  }}
                  placeholder="e.g. #00b4d8, crimson, rgba(56, 189, 248, 0.9)"
                  className="h-8 text-xs font-mono"
                />
              </div>

              {/* Reset to brand/default color button */}
              {customColor && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-8 text-xs px-2 text-muted-foreground hover:text-foreground"
                  onClick={() => handleApplyColor('')}
                >
                  Reset Color
                </Button>
              )}
            </div>

            {/* Quick color preset chips */}
            <div className="flex flex-wrap items-center gap-1.5 pt-1">
              <span className="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mr-1">Presets:</span>
              {[
                { label: 'Cyan', color: '#00b4d8' },
                { label: 'Teal', color: '#14b8a6' },
                { label: 'Emerald', color: '#10b981' },
                { label: 'Sky', color: '#0ea5e9' },
                { label: 'Blue', color: '#3b82f6' },
                { label: 'Indigo', color: '#6366f1' },
                { label: 'Purple', color: '#a855f7' },
                { label: 'Rose', color: '#f43f5e' },
                { label: 'Orange', color: '#f97316' },
                { label: 'Gold', color: 'gold' },
                { label: 'Crimson', color: 'crimson' },
                { label: 'Coral', color: 'coral' },
                { label: 'Translucent Cyan', color: 'rgba(0, 180, 216, 0.85)' },
              ].map((preset) => (
                <button
                  key={preset.label}
                  type="button"
                  onClick={() => handleApplyColor(preset.color)}
                  className="group flex cursor-pointer items-center gap-1 rounded-full border border-border/60 bg-background/60 px-2 py-0.5 text-[10px] text-muted-foreground transition-colors hover:border-border hover:bg-muted hover:text-foreground"
                  title={`Set color to ${preset.label} (${preset.color})`}
                >
                  <span
                    className="size-2 rounded-full border border-black/20 shrink-0"
                    style={{ backgroundColor: preset.color }}
                  />
                  <span>{preset.label}</span>
                </button>
              ))}
            </div>
          </div>

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

          {/* Icons Grid with zero layout jumping */}
          <div className="grid max-h-60 grid-cols-3 gap-2 overflow-y-auto rounded-md border border-border p-2 sm:grid-cols-4 md:grid-cols-6">
            {filteredIcons.map((item) => {
              const isSelected = activeFaName === item.id
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => void handleSelectFaIcon(item.id)}
                  disabled={busy}
                  className={`group flex cursor-pointer flex-col items-center gap-1.5 rounded-md border p-2 text-center transition-all ${
                    isSelected
                      ? 'border-primary bg-primary/10 text-primary shadow-xs'
                      : 'border-border/60 bg-muted/20 hover:border-border hover:bg-muted/60 text-foreground'
                  }`}
                  title={item.name}
                >
                  <SiteLogo
                    icon={isSelected ? currentIcon : item.id}
                    className="size-6 transition-transform group-hover:scale-110"
                  />
                  <span className="w-full truncate text-[11px] leading-tight">{item.name}</span>
                </button>
              )
            })}
            {filteredIcons.length === 0 && (
              <div className="col-span-full py-6 text-center text-xs text-muted-foreground">
                No offline icons found matching &ldquo;{search}&rdquo;.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
