import { open } from '@tauri-apps/plugin-dialog'
import {
  autoUpdate,
  flip,
  FloatingPortal,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
} from '@floating-ui/react'
import { Check, ChevronDown, Image as ImageIcon, Search, Trash2, Upload, X } from 'lucide-react'
import { useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { IconCategoryFilter } from '@/components/IconCategoryFilter'
import { SiteLogo } from '@/components/SiteLogo'
import { filterFaIcons, buildIconSpec, parseIconSpec } from '@/components/faIcons'
import { projectTypeTechIcon } from '@/components/techBrands'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { runCommand } from '@/core'

export interface SiteLogoPanelProps {
  hostname: string | null
  projectId?: string | null
  folder: string
  pickerFolder?: string
  displayName: string
  projectType?: string | null
  initialIcon?: string | null
  initialImage?: string | null
  isProxy?: boolean
  fallback?: 'globe' | 'folder'
  onUpdated?: () => Promise<void> | void
}

/**
 * Site logo & icon management panel.
 * Can be rendered on its own page inside Site Settings (SiteDialog)
 * or inside a standalone dialog (SiteLogoDialog).
 */
export function SiteLogoPanel({
  hostname,
  projectId = null,
  folder,
  pickerFolder,
  displayName,
  projectType = null,
  initialIcon = null,
  initialImage = null,
  isProxy = false,
  fallback = 'globe',
  onUpdated,
}: SiteLogoPanelProps) {
  const librarySelectionStorageKey = `ols:site-icon-library:${projectId ? `project:${projectId}` : hostname ? `site:${hostname}` : `folder:${folder}`}`
  const [activeTab, setActiveTab] = useState<'photo' | 'library' | 'fa'>('photo')
  const [currentIcon, setCurrentIcon] = useState<string | null>(initialIcon)
  const [currentImage, setCurrentImage] = useState<string | null>(initialImage)
  const [isCustomImage, setIsCustomImage] = useState(Boolean(isProxy && initialImage))

  // Parsed Font Awesome state
  const initialParsed = parseIconSpec(initialIcon)
  const [activeFaName, setActiveFaName] = useState<string>(initialParsed.name)
  const [customColor, setCustomColor] = useState<string>(initialParsed.color ?? '')

  const [search, setSearch] = useState('')
  const [librarySearch, setLibrarySearch] = useState('')
  const [selectedLibraryCategory, setSelectedLibraryCategory] = useState('__all__')
  const [selectedLibraryIcon, setSelectedLibraryIcon] = useState<string | null>(() => {
    try {
      return localStorage.getItem(librarySelectionStorageKey)
    } catch {
      return null
    }
  })
  const [libraryIcons, setLibraryIcons] = useState<{ name: string; category: string; data_url: string }[]>([])
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
    setIsCustomImage(Boolean(isProxy && initialImage))
  }, [initialImage, isProxy])

  useEffect(() => {
    try {
      setSelectedLibraryIcon(localStorage.getItem(librarySelectionStorageKey))
    } catch {
      setSelectedLibraryIcon(null)
    }
  }, [librarySelectionStorageKey])

  const rememberLibraryIcon = (key: string | null) => {
    setSelectedLibraryIcon(key)
    try {
      if (key) localStorage.setItem(librarySelectionStorageKey, key)
      else localStorage.removeItem(librarySelectionStorageKey)
    } catch {
      // Selection still works for this panel session when storage is unavailable.
    }
  }

  useEffect(() => {
    runCommand({ type: 'list_site_icon_library' })
      .then((res) => {
        if (res.type === 'site_icon_library') {
          setLibraryIcons(res.icons)
          setSelectedLibraryCategory('__all__')
        }
      })
      .catch(() => setLibraryIcons([]))
  }, [])

  // Distinguish an uploaded image from a detected favicon so the remove action only
  // targets a file OLS owns. Proxy icons are always manually uploaded.
  useEffect(() => {
    if (folder && !isProxy) {
      const sep = folder.includes('/') ? '/' : '\\'
      const iconPath = folder.endsWith(sep) ? `${folder}ols_site_icon.webp` : `${folder}${sep}ols_site_icon.webp`
      runCommand({ type: 'read_image_file', path: iconPath })
        .then((res) => {
          if (res.type === 'image_file' && res.data_url) {
            setCurrentImage(res.data_url)
            setIsCustomImage(true)
          }
        })
        .catch(() => {
          setIsCustomImage(false)
        })
    }
  }, [folder, isProxy])

  const flashSaved = () => {
    setSavedNotice(true)
    setTimeout(() => setSavedNotice(false), 2200)
  }

  const saveImageDataUrl = async (sourceDataUrl: string) => {
    const img = new Image()
    img.src = sourceDataUrl
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('Could not decode this icon. Choose a valid image file.'))
    })

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
    await runCommand({
      type: 'set_site_icon_image',
      hostname: hostname ?? undefined,
      folder: folder || undefined,
      webp_base64: webpDataUrl,
    })
    setCurrentImage(webpDataUrl)
    setIsCustomImage(true)
    flashSaved()
  }

  const handlePickPhoto = async () => {
    setError(null)
    try {
      const selected = await open({
        multiple: false,
        defaultPath: pickerFolder || folder || undefined,
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

      await saveImageDataUrl(res.data_url)
      setCurrentIcon(null)
      setActiveFaName('')
      setCustomColor('')
      rememberLibraryIcon(null)
      if (hostname) {
        await runCommand({ type: 'set_domain_icon', hostname, icon: null })
      } else if (projectId) {
        await runCommand({ type: 'update_project', id: projectId, clear_icon: true })
      }
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleSelectLibraryIcon = async (item: { name: string; category: string; data_url: string }) => {
    setError(null)
    setBusy(true)
    try {
      await saveImageDataUrl(item.data_url)
      setCurrentIcon(null)
      setActiveFaName('')
      setCustomColor('')
      rememberLibraryIcon(`${item.category}/${item.name}`)
      if (hostname) {
        await runCommand({ type: 'set_domain_icon', hostname, icon: null })
      } else if (projectId) {
        await runCommand({ type: 'update_project', id: projectId, clear_icon: true })
      }
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleClearCurrentIcon = async () => {
    setError(null)
    setCurrentImage(null)
    setIsCustomImage(false)
    setCurrentIcon(null)
    setActiveFaName('')
    setCustomColor('')
    rememberLibraryIcon(null)
    flashSaved()
    setBusy(true)
    try {
      if (isCustomImage) {
        await runCommand({
          type: 'remove_site_icon_image',
          hostname: hostname ?? undefined,
          folder: folder || undefined,
        })
      }
      if (hostname) {
        await runCommand({ type: 'set_domain_icon', hostname, icon: null })
      } else if (projectId) {
        await runCommand({ type: 'update_project', id: projectId, clear_icon: true })
      }
      void onUpdated?.()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleSelectFaIcon = async (iconId: string, colorToApply?: string) => {
    if (!hostname && !projectId) {
      setError('Font Awesome icons require a site domain or registered project.')
      return
    }
    const color = colorToApply !== undefined ? colorToApply : customColor
    const iconSpec = buildIconSpec(iconId, color)

    // Immediate optimistic update (zero glitch/layout shift)
    setCurrentIcon(iconSpec)
    setActiveFaName(iconId)
    setCurrentImage(null)
    setIsCustomImage(false)
    rememberLibraryIcon(null)
    flashSaved()

    try {
      if (isCustomImage) {
        await runCommand({
          type: 'remove_site_icon_image',
          hostname: hostname ?? undefined,
          folder: folder || undefined,
        })
      }
      if (hostname) {
        await runCommand({
          type: 'set_domain_icon',
          hostname,
          icon: iconSpec,
        })
      } else if (projectId) {
        await runCommand({
          type: 'update_project',
          id: projectId,
          icon: iconSpec,
        })
      }
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
    await handleClearCurrentIcon()
  }

  // Deferred so typing stays instant while the grid re-filters a few hundred icons.
  const deferredSearch = useDeferredValue(search)
  const filteredIcons = useMemo(() => filterFaIcons(category, deferredSearch), [category, deferredSearch])
  const libraryCategories = useMemo(() => [...new Set(libraryIcons.map((item) => item.category))].sort((a, b) => {
    if (a === '') return -1
    if (b === '') return 1
    return a.localeCompare(b)
  }), [libraryIcons])
  const filteredLibraryIcons = libraryIcons.filter((item) => {
    if (selectedLibraryCategory !== '__all__' && item.category !== selectedLibraryCategory) return false
    if (!librarySearch) return true
    const term = librarySearch.toLowerCase()
    return item.name.toLowerCase().includes(term) || (item.category || 'root icons').toLowerCase().includes(term)
  })

  return (
    <div className="flex flex-col gap-4">
      {error && <ErrorCard error={asDiagnostic(error)} onDismiss={() => setError(null)} overlay />}

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
            {currentImage && isCustomImage ? (
              <Badge variant="outline" className="border-emerald-500/40 bg-emerald-500/10 text-emerald-500 text-[10px]">
                {isProxy ? 'Proxy Site Icon (proxy-site-images)' : 'Custom Photo (ols_site_icon.webp)'}
              </Badge>
            ) : currentImage ? (
              <Badge variant="outline" className="border-sky-500/40 bg-sky-500/10 text-sky-500 text-[10px]">
                Detected Site Favicon
              </Badge>
            ) : currentIcon ? (
              <Badge variant="outline" className="border-sky-500/40 bg-sky-500/10 text-sky-500 text-[10px]">
                Font Awesome ({parseIconSpec(currentIcon).name})
              </Badge>
            ) : projectTypeTechIcon(projectType) ? (
              <Badge variant="outline" className="border-sky-500/40 bg-sky-500/10 text-sky-500 text-[10px]">
                {projectType?.split(':').at(-1)} Project Icon
              </Badge>
            ) : fallback === 'folder' ? (
              <Badge variant="outline" className="border-amber-500/40 bg-amber-500/10 text-amber-500 dark:text-amber-400 text-[10px]">
                Default Folder Icon
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
              projectType={projectType}
              fallback={fallback}
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

      {/* Tabs: upload, user library, and Font Awesome */}
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
          {isProxy ? 'Site Icon Image (proxy-site-images)' : 'Photo from PC (ols_site_icon.webp)'}
        </button>
        <button
          type="button"
          className={`cursor-pointer px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px ${
            activeTab === 'library'
              ? 'border-primary text-primary'
              : 'border-transparent text-muted-foreground hover:text-foreground'
          }`}
          onClick={() => setActiveTab('library')}
        >
          My Icons ({libraryIcons.length})
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

      {(currentImage || currentIcon) && (
        <div className="flex justify-end">
          <Button
            size="sm"
            variant="outline"
            className="text-destructive hover:bg-destructive/10"
            onClick={() => void handleClearCurrentIcon()}
            disabled={busy}
          >
            <Trash2 className="size-3.5" />
            Clear selected icon
          </Button>
        </div>
      )}

      {/* Tab 1: Photo from PC */}
      {activeTab === 'photo' && (
        <div className="flex flex-col gap-4 py-2">
          <div className="rounded-md border border-dashed border-border p-4 text-center">
            <div className="mx-auto flex size-12 items-center justify-center rounded-full bg-muted/60 text-muted-foreground">
              <ImageIcon className="size-6" />
            </div>
            <h3 className="mt-2 text-sm font-medium">Pick a photo from your PC</h3>
            <p className="mt-1 text-xs text-muted-foreground">
              PNG, JPG, WEBP, SVG, GIF, or ICO. The image is converted to WebP format and saved{' '}
              {isProxy ? (
                <>
                  into OpenLocalServer's data folder as{' '}
                  <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">
                    data/proxy-site-images/{hostname ? hostname.replace(/[^a-zA-Z0-9-]/g, '_') : 'domain'}.webp
                  </code>
                  . Each proxy site maintains its own unique icon.
                </>
              ) : (
                <>
                  directly into the project folder as{' '}
                  <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground">ols_site_icon.webp</code>.
                </>
              )}
            </p>
            <div className="mt-4 flex flex-wrap items-center justify-center gap-2">
              <Button size="sm" onClick={() => void handlePickPhoto()} disabled={busy}>
                <Upload className="size-3.5" />
                {busy ? 'Processing…' : 'Choose photo from PC…'}
              </Button>
            </div>
          </div>

          <div className="text-xs text-muted-foreground space-y-1">
            <p>
              <strong>Destination path:</strong>{' '}
              <span className="font-mono text-[11px]">
                {isProxy
                  ? `data\\proxy-site-images\\${hostname ? hostname.replace(/[^a-zA-Z0-9-]/g, '_') : 'domain'}.webp`
                  : folder
                    ? `${folder}\\ols_site_icon.webp`
                    : 'Project root'}
              </span>
            </p>
            <p>
              If the file is deleted or not provided, the site automatically falls back to the configured Font Awesome icon or the default internet globe icon.
            </p>
          </div>
        </div>
      )}

      {activeTab === 'library' && (
        <div className="flex flex-col gap-3 py-2">
          <p className="text-xs text-muted-foreground">
            Add icons under <code>data/icons</code> or a category folder. Name an icon <code>laravel_fallback.ico</code> to use it automatically for Laravel sites when no site icon is available.
          </p>
          <div className="flex items-center gap-2">
            <Search className="size-4 text-muted-foreground" />
            <Input value={librarySearch} onChange={(e) => setLibrarySearch(e.target.value)} placeholder="Search this folder…" />
          </div>
          <IconFolderTabs
            categories={libraryCategories}
            icons={libraryIcons}
            selected={selectedLibraryCategory === '__all__' ? '__all__' : `folder:${selectedLibraryCategory}`}
            onSelect={(id) => setSelectedLibraryCategory(id === '__all__' ? '__all__' : id.slice('folder:'.length))}
          />
          {filteredLibraryIcons.length ? (
            <div className="grid max-h-64 grid-cols-4 gap-1.5 overflow-y-auto sm:grid-cols-5 md:grid-cols-6">
              {filteredLibraryIcons.map((item) => {
                const isSelected = selectedLibraryIcon === `${item.category}/${item.name}`
                return (
                  <button
                    key={`${item.category}/${item.name}`}
                    type="button"
                    disabled={busy}
                    aria-pressed={isSelected}
                    onClick={() => void handleSelectLibraryIcon(item)}
                    className={`flex min-w-0 flex-col items-center gap-1.5 rounded-md border p-2 text-center transition-all disabled:opacity-50 ${
                      isSelected
                        ? 'border-primary bg-primary/10 text-primary shadow-xs'
                        : 'border-border/60 bg-muted/20 text-foreground hover:border-border hover:bg-muted/60'
                    }`}
                    title={`Use ${item.name} from ${item.category}`}
                  >
                    <img src={item.data_url} alt="" className="size-7 rounded object-contain" />
                    <span className="w-full truncate text-xs">{item.name}</span>
                    <span className={`w-full truncate text-[10px] ${isSelected ? 'text-primary/80' : 'text-muted-foreground'}`}>{item.category}</span>
                  </button>
                )
              })}
            </div>
          ) : libraryIcons.length ? (
            <div className="rounded-md border border-dashed p-5 text-center text-xs text-muted-foreground">
              No icons in this folder match the search.
            </div>
          ) : (
            <div className="rounded-md border border-dashed p-5 text-center text-xs text-muted-foreground">
              <p>No custom icons found.</p>
              <p className="mt-1">Add image files to <code>data/icons</code> or a category folder such as <code>data/icons/Web</code>.</p>
            </div>
          )}
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

          {/* Category list (wide) or dropdown (narrow) beside a scrolling icon grid */}
          <IconCategoryFilter value={category} onChange={setCategory} search={deferredSearch} heightClassName="h-72">
            {/* Auto-fill columns follow the grid pane's own width, not the window's. */}
            <div className="grid grid-cols-[repeat(auto-fill,minmax(5.5rem,1fr))] gap-2">
              {filteredIcons.map((item) => {
                const isSelected = activeFaName === item.id
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => void handleSelectFaIcon(item.id)}
                    disabled={busy}
                    // content-visibility lets the browser skip layout and paint for tiles
                    // scrolled out of view, so a long grid scrolls as smoothly as a short one.
                    className={`group flex cursor-pointer flex-col items-center gap-1.5 rounded-md border p-2 text-center transition-all [content-visibility:auto] [contain-intrinsic-size:auto_64px] ${
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
                  No offline icons found matching &ldquo;{deferredSearch}&rdquo;.
                </div>
              )}
            </div>
          </IconCategoryFilter>
        </div>
      )}
    </div>
  )
}

interface IconFolderTab {
  id: string
  label: string
  count: number
}

function IconFolderTabs({
  categories,
  icons,
  selected,
  onSelect,
}: {
  categories: string[]
  icons: { category: string }[]
  selected: string
  onSelect: (id: string) => void
}) {
  const tabs = useMemo<IconFolderTab[]>(() => [
    { id: '__all__', label: 'All', count: icons.length },
    ...categories.map((category) => ({
      id: `folder:${category}`,
      label: category === '' ? 'Root icons' : category,
      count: icons.filter((icon) => icon.category === category).length,
    })),
  ], [categories, icons])
  const measureRef = useRef<HTMLDivElement>(null)
  const chipsRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)
  const [availableWidth, setAvailableWidth] = useState(0)
  const [measuredWidths, setMeasuredWidths] = useState<Record<string, number>>({})
  const [moreWidth, setMoreWidth] = useState(84)
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')

  useLayoutEffect(() => {
    if (!measureRef.current) return
    const widths: Record<string, number> = {}
    measureRef.current.querySelectorAll<HTMLElement>('[data-folder-tab]').forEach((element) => {
      const id = element.dataset.folderTab
      const width = element.getBoundingClientRect().width
      if (id === '__more__') setMoreWidth(width)
      else if (id) widths[id] = width
    })
    setMeasuredWidths(widths)
  }, [tabs])

  useEffect(() => {
    const element = chipsRef.current
    if (!element || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0) setAvailableWidth(entry.contentRect.width)
    })
    observer.observe(element)
    setAvailableWidth(element.clientWidth)
    return () => observer.disconnect()
  }, [])

  const { visibleTabs, overflowTabs } = useMemo(() => {
    const allTab = tabs[0]
    const otherTabs = tabs.slice(1)
    const totalWidth = tabs.reduce((total, tab) => total + (measuredWidths[tab.id] ?? 80) + 6, -6)
    if (!availableWidth || totalWidth <= availableWidth) {
      return { visibleTabs: tabs, overflowTabs: [] as IconFolderTab[] }
    }
    const maxWidth = Math.max(availableWidth - moreWidth - 6, 76)
    const visible = [allTab]
    const overflow: IconFolderTab[] = []
    let usedWidth = measuredWidths[allTab.id] ?? 50
    for (const tab of otherTabs) {
      const width = measuredWidths[tab.id] ?? 80
      if (usedWidth + 6 + width <= maxWidth) {
        visible.push(tab)
        usedWidth += 6 + width
      } else {
        overflow.push(tab)
      }
    }
    if (selected !== '__all__') {
      const selectedIndex = overflow.findIndex((tab) => tab.id === selected)
      if (selectedIndex >= 0) {
        const selectedTab = overflow[selectedIndex]
        if (visible.length > 1) {
          const displacedIndex = visible.length - 1
          overflow[selectedIndex] = visible[displacedIndex]
          visible[displacedIndex] = selectedTab
        } else {
          visible.push(selectedTab)
          overflow.splice(selectedIndex, 1)
        }
      }
    }
    return { visibleTabs: visible, overflowTabs: overflow }
  }, [tabs, measuredWidths, availableWidth, moreWidth, selected])

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: (nextOpen) => {
      setOpen(nextOpen)
      if (!nextOpen) setQuery('')
    },
    placement: 'bottom-start',
    middleware: [offset(4), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
  })
  const dismiss = useDismiss(context)
  const { getReferenceProps, getFloatingProps } = useInteractions([dismiss])

  useEffect(() => {
    if (open) requestAnimationFrame(() => searchRef.current?.focus())
  }, [open])

  const filteredOverflow = overflowTabs.filter((tab) => tab.label.toLowerCase().includes(query.trim().toLowerCase()))

  return (
    <>
      <div
        ref={measureRef}
        aria-hidden="true"
        className="pointer-events-none absolute left-[-9999px] top-[-9999px] flex items-center gap-1.5 whitespace-nowrap opacity-0"
      >
        {tabs.map((tab) => (
          <button key={tab.id} type="button" data-folder-tab={tab.id} className="flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium">
            <span>{tab.label}</span><span className="text-[10px]">{tab.count}</span>
          </button>
        ))}
        <button type="button" data-folder-tab="__more__" className="flex shrink-0 items-center gap-1 rounded-md px-2.5 py-1.5 text-xs font-medium">
          <span>+99 more</span><ChevronDown className="size-3.5" />
        </button>
      </div>
      <div ref={chipsRef} className="flex min-w-0 items-center gap-1.5 overflow-hidden" aria-label="Icon folders">
        {visibleTabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            aria-pressed={selected === tab.id}
            onClick={() => onSelect(tab.id)}
            className={`flex shrink-0 cursor-pointer items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs font-medium transition-colors ${
              selected === tab.id
                ? 'bg-primary font-semibold text-[var(--on-accent,var(--primary-foreground))] shadow-xs'
                : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground'
            }`}
            title={`${tab.label} (${tab.count})`}
          >
            <span>{tab.label}</span>
            <span className="text-[10px] opacity-80">{tab.count}</span>
          </button>
        ))}
        {overflowTabs.length > 0 && (
          <button
            ref={refs.setReference}
            {...getReferenceProps()}
            type="button"
            onClick={() => setOpen((wasOpen) => !wasOpen)}
            aria-expanded={open}
            aria-haspopup="dialog"
            className={`flex shrink-0 cursor-pointer items-center gap-1 rounded-md border border-border/80 bg-background px-2.5 py-1.5 text-xs font-medium text-foreground hover:bg-muted/70 ${open ? 'border-primary/50 bg-muted' : ''}`}
            title={`${overflowTabs.length} more icon folders`}
          >
            <span>+{overflowTabs.length} more</span>
            <ChevronDown className={`size-3.5 transition-transform duration-150 ${open ? 'rotate-180' : ''}`} />
          </button>
        )}
      </div>
      {open && (
        <FloatingPortal>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            {...getFloatingProps()}
            className="z-50 w-64 rounded-lg border border-border bg-popover p-1.5 text-popover-foreground shadow-lg outline-none animate-in fade-in-0 zoom-in-95"
            aria-label="More icon folders"
          >
            <div className="relative mb-1.5 px-0.5 pt-0.5">
              <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                ref={searchRef}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search folders..."
                className="h-8 pl-8 pr-7 text-xs"
                aria-label="Search icon folders"
              />
              {query && (
                <button type="button" onClick={() => setQuery('')} className="absolute right-2 top-1/2 flex size-4 -translate-y-1/2 cursor-pointer items-center justify-center text-muted-foreground hover:text-foreground" aria-label="Clear folder search">
                  <X className="size-3" />
                </button>
              )}
            </div>
            <div className="flex max-h-56 flex-col gap-0.5 overflow-y-auto pr-0.5">
              {filteredOverflow.length ? filteredOverflow.map((tab) => (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => { onSelect(tab.id); setOpen(false) }}
                  className={`flex w-full cursor-pointer items-center justify-between rounded-md px-2 py-1.5 text-left text-xs font-medium transition-colors ${
                    selected === tab.id ? 'bg-primary/10 font-semibold text-primary' : 'text-foreground hover:bg-muted/70'
                  }`}
                >
                  <span className="truncate">{tab.label}</span>
                  <span className="ml-2 shrink-0 text-[10px] text-muted-foreground">{tab.count}</span>
                </button>
              )) : (
                <div className="py-4 text-center text-xs text-muted-foreground">No matching folders</div>
              )}
            </div>
          </div>
        </FloatingPortal>
      )}
    </>
  )
}
