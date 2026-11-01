import { Check, Laptop, Moon, RotateCcw, Sparkles, Sun } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  BUILTIN_PRESETS,
  CURATED_ACCENTS,
  CURATED_GRADIENTS,
  getContrastWarning,
  useTheme,
  type SidebarIconStyle,
  type ThemeMode,
} from '@/lib/theme'
import { cn } from '@/lib/utils'
import { DashboardIcon, QuickAppsIcon, SitesIcon } from '@/components/layout/SidebarIcons'

const HEX_RE = /^#[0-9a-fA-F]{6}$/

/**
 * Color picker + hex input with local draft state. Drags update the draft instantly
 * and are committed at most once per animation frame, so the rest of the app is not
 * re-rendered on every raw input event.
 */
function ColorField({
  value,
  fallback,
  onCommit,
}: {
  value: string
  fallback: string
  onCommit: (hex: string) => void
}) {
  const [draft, setDraft] = useState(value)
  const raf = useRef<number | null>(null)
  const pending = useRef<string | null>(null)
  const commitRef = useRef(onCommit)
  commitRef.current = onCommit

  // Follow external changes (presets, reset) when not mid-edit.
  useEffect(() => {
    if (pending.current === null) setDraft(value)
  }, [value])

  useEffect(
    () => () => {
      if (raf.current !== null) cancelAnimationFrame(raf.current)
    },
    [],
  )

  const update = (next: string) => {
    setDraft(next)
    if (!HEX_RE.test(next)) return
    pending.current = next
    if (raf.current !== null) return
    raf.current = requestAnimationFrame(() => {
      raf.current = null
      const hex = pending.current
      pending.current = null
      if (hex) commitRef.current(hex)
    })
  }

  const pickerValue = HEX_RE.test(draft) ? draft : HEX_RE.test(value) ? value.slice(0, 7) : fallback

  return (
    <div className="flex items-center gap-2 rounded-lg border border-border bg-card px-2 py-1">
      <input
        type="color"
        value={pickerValue}
        onChange={(e) => update(e.target.value)}
        className="size-6 cursor-pointer rounded border-0 bg-transparent p-0"
      />
      <Input
        value={draft}
        onChange={(e) => update(e.target.value)}
        maxLength={7}
        placeholder={fallback}
        className="h-6 w-24 border-0 bg-transparent p-0 font-mono text-xs uppercase shadow-none focus-visible:ring-0"
      />
    </div>
  )
}

export function AppearanceSettings() {
  const {
    settings,
    mode,
    resolvedTheme,
    preset,
    tokens,
    options,
    setMode,
    setPresetId,
    setOverrides,
    resetToDefault,
  } = useTheme()

  const [resetConfirm, setResetConfirm] = useState(false)

  const activeAccent = settings.overrides.accent ?? preset[resolvedTheme].primary
  const contrastInfo = getContrastWarning(activeAccent, resolvedTheme === 'dark')

  // Background state
  const isGradient = options.background.type === 'gradient'
  const gradFrom =
    options.background.type === 'gradient' ? options.background.from : '#050B16'
  const gradTo =
    options.background.type === 'gradient' ? options.background.to : '#0B1428'
  const gradAngle =
    options.background.type === 'gradient' ? options.background.angle : 135

  const handleReset = () => {
    resetToDefault()
    setResetConfirm(true)
    setTimeout(() => setResetConfirm(false), 2500)
  }

  return (
    <div className="flex flex-col gap-6">
      {/* ── 1. Color Mode ──────────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-base font-semibold">Mode</CardTitle>
              <CardDescription>
                Choose how OpenLocalServer adjusts its interface lighting.
              </CardDescription>
            </div>
            <div className="flex items-center gap-1.5">
              {mode === 'system' && (
                <Badge variant="secondary" className="text-xs font-normal">
                  System: {resolvedTheme}
                </Badge>
              )}
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-3 gap-3">
            {(
              [
                { id: 'system', label: 'System', icon: Laptop, hint: 'Follows OS mode' },
                { id: 'light', label: 'Light', icon: Sun, hint: 'Clean & daylight' },
                { id: 'dark', label: 'Dark', icon: Moon, hint: 'High-focus dark' },
              ] as const
            ).map((m) => {
              const active = mode === m.id
              const Icon = m.icon
              return (
                <button
                  key={m.id}
                  onClick={() => setMode(m.id as ThemeMode)}
                  className={cn(
                    'relative flex flex-col items-center justify-center gap-2 rounded-xl border p-4 text-center transition-all',
                    active
                      ? 'border-primary bg-primary/10 text-primary shadow-xs'
                      : 'border-border bg-card/60 text-muted-foreground hover:border-primary/40 hover:bg-card hover:text-foreground',
                  )}
                >
                  {active && (
                    <span className="absolute right-2.5 top-2.5 flex size-4 items-center justify-center rounded-full bg-primary text-[10px] text-primary-foreground">
                      <Check className="size-2.5 stroke-[3]" />
                    </span>
                  )}
                  <Icon className="size-5" />
                  <div className="flex flex-col">
                    <span className="text-sm font-medium">{m.label}</span>
                    <span className="text-[11px] text-muted-foreground">{m.hint}</span>
                  </div>
                </button>
              )
            })}
          </div>
        </CardContent>
      </Card>

      {/* ── 2. Theme Presets ───────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base font-semibold">Presets</CardTitle>
          <CardDescription>
            Select a curated visual foundation. Presets provide harmonious tokens for both light and dark.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {BUILTIN_PRESETS.map((p) => {
              const active = settings.presetId === p.id
              return (
                <button
                  key={p.id}
                  onClick={() => setPresetId(p.id)}
                  className={cn(
                    'group relative flex flex-col gap-3 rounded-xl border p-3 text-left transition-all',
                    active
                      ? 'border-primary bg-primary/5 shadow-md ring-1 ring-primary/40'
                      : 'border-border bg-card/60 hover:border-primary/40 hover:bg-card',
                  )}
                >
                  {/* Miniature Light/Dark split preview container */}
                  <div className="relative flex h-24 w-full overflow-hidden rounded-lg border border-border/80 shadow-xs">
                    {/* Light preview half */}
                    <div
                      className="relative flex w-1/2 flex-col justify-between p-2"
                      style={{ backgroundColor: p.light.background }}
                    >
                      <div className="flex items-center gap-1">
                        <span
                          className="size-2 rounded-full"
                          style={{ backgroundColor: p.light.primary }}
                        />
                        <span
                          className="h-1.5 w-6 rounded-full"
                          style={{ backgroundColor: p.light.foreground, opacity: 0.8 }}
                        />
                      </div>
                      <div
                        className="rounded-md border p-1.5"
                        style={{
                          backgroundColor: p.light.card,
                          borderColor: p.light.border,
                        }}
                      >
                        <div
                          className="h-2 w-8 rounded-full"
                          style={{ backgroundColor: p.light.primary }}
                        />
                      </div>
                    </div>

                    {/* Dark preview half */}
                    <div
                      className="relative flex w-1/2 flex-col justify-between border-l border-border/60 p-2"
                      style={{ backgroundColor: p.dark.background }}
                    >
                      <div className="flex items-center gap-1">
                        <span
                          className="size-2 rounded-full"
                          style={{ backgroundColor: p.dark.primary }}
                        />
                        <span
                          className="h-1.5 w-6 rounded-full"
                          style={{ backgroundColor: p.dark.foreground, opacity: 0.8 }}
                        />
                      </div>
                      <div
                        className="rounded-md border p-1.5"
                        style={{
                          backgroundColor: p.dark.card,
                          borderColor: p.dark.border,
                        }}
                      >
                        <div
                          className="h-2 w-8 rounded-full"
                          style={{ backgroundColor: p.dark.primary }}
                        />
                      </div>
                    </div>

                    {/* Active check indicator */}
                    {active && (
                      <span className="absolute right-2 top-2 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
                        <Check className="size-3 stroke-[3]" />
                      </span>
                    )}
                  </div>

                  <div className="flex items-center justify-between">
                    <span className="text-sm font-semibold">{p.name}</span>
                    {active && (
                      <span className="text-[11px] font-medium text-primary">Active</span>
                    )}
                  </div>
                </button>
              )
            })}
          </div>
        </CardContent>
      </Card>

      {/* ── 3. Accent Color ────────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-base font-semibold">Accent Color</CardTitle>
              <CardDescription>
                Drives buttons, focus rings, and active navigation indicators.
              </CardDescription>
            </div>
            {settings.overrides.accent && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-xs text-muted-foreground"
                onClick={() => {
                  setOverrides({ accent: undefined })
                }}
              >
                Reset to preset accent
              </Button>
            )}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {/* Swatches */}
          <div className="flex flex-wrap items-center gap-3">
            {CURATED_ACCENTS.map((swatch) => {
              const colorVal = resolvedTheme === 'dark' ? swatch.darkValue : swatch.value
              const isSelected = activeAccent.toLowerCase() === colorVal.toLowerCase()
              return (
                <button
                  key={swatch.name}
                  onClick={() => {
                    setOverrides({ accent: colorVal })
                  }}
                  title={swatch.name}
                  className={cn(
                    'group relative flex size-9 items-center justify-center rounded-full border-2 transition-transform hover:scale-110',
                    isSelected
                      ? 'border-foreground shadow-sm scale-105'
                      : 'border-transparent hover:border-border',
                  )}
                  style={{ backgroundColor: colorVal }}
                >
                  {isSelected && (
                    <Check
                      className="size-4 stroke-[3]"
                      style={{ color: tokens.primaryForeground }}
                    />
                  )}
                </button>
              )
            })}

            {/* Custom Color Input */}
            <ColorField
              value={activeAccent}
              fallback="#059669"
              onCommit={(hex) => setOverrides({ accent: hex })}
            />
          </div>
          {/* Contrast warning if poor */}
          {contrastInfo.isLow && (
            <div className="flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 p-2.5 text-xs text-warning">
              <span className="font-semibold">Notice:</span>
              <span>{contrastInfo.message}</span>
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── 4. Sidebar Icon Style ──────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base font-semibold">Sidebar Icon Style</CardTitle>
          <CardDescription>
            Choose between colorful illustrations or streamlined monochrome icons.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            {(
              [
                {
                  id: 'colorful',
                  label: 'Colorful',
                  desc: 'Vibrant, hand-crafted illustration colors per navigation item.',
                },
                {
                  id: 'monochrome',
                  label: 'Monochrome',
                  desc: 'Clean, minimalist outline icons styled with currentColor.',
                },
              ] as const
            ).map((style) => {
              const active = options.sidebarIcons === style.id
              const isMono = style.id === 'monochrome'
              return (
                <button
                  key={style.id}
                  onClick={() => setOverrides({ sidebarIcons: style.id as SidebarIconStyle })}
                  className={cn(
                    'relative flex flex-col gap-2 rounded-xl border p-4 text-left transition-all',
                    active
                      ? 'border-primary bg-primary/5 shadow-xs ring-1 ring-primary/40'
                      : 'border-border bg-card/60 hover:border-primary/40 hover:bg-card',
                  )}
                >
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <div className="flex items-center gap-1.5 rounded-lg border border-border/80 bg-background/60 p-1.5">
                        <DashboardIcon className="size-4" monochrome={isMono} />
                        <SitesIcon className="size-4" monochrome={isMono} />
                        <QuickAppsIcon className="size-4" monochrome={isMono} />
                      </div>
                      <span className="text-sm font-semibold">{style.label}</span>
                    </div>
                    {active && (
                      <span className="flex size-4 items-center justify-center rounded-full bg-primary text-[10px] text-primary-foreground">
                        <Check className="size-2.5 stroke-[3]" />
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-muted-foreground">{style.desc}</p>
                </button>
              )
            })}
          </div>
        </CardContent>
      </Card>

      {/* ── 5. App Background ──────────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-base font-semibold">App Background</CardTitle>
              <CardDescription>
                Choose a clean solid theme background or an ambient linear gradient.
              </CardDescription>
            </div>
            {isGradient && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-xs text-muted-foreground"
                onClick={() => setOverrides({ background: { type: 'solid' } })}
              >
                Reset to solid
              </Button>
            )}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex gap-2">
            <Button
              variant={!isGradient ? 'default' : 'outline'}
              size="sm"
              onClick={() => setOverrides({ background: { type: 'solid' } })}
            >
              Solid
            </Button>
            <Button
              variant={isGradient ? 'default' : 'outline'}
              size="sm"
              onClick={() =>
                setOverrides({
                  background: {
                    type: 'gradient',
                    from: gradFrom,
                    to: gradTo,
                    angle: gradAngle,
                  },
                })
              }
            >
              Gradient
            </Button>
          </div>

          {/* Gradient controls */}
          {isGradient && (
            <div className="flex flex-col gap-4 rounded-xl border border-border bg-card/40 p-4">
              <div>
                <span className="text-xs font-medium text-muted-foreground">
                  Curated Gradients
                </span>
                <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
                  {CURATED_GRADIENTS.map((cg) => (
                    <button
                      key={cg.name}
                      onClick={() =>
                        setOverrides({
                          background: {
                            type: 'gradient',
                            from: cg.from,
                            to: cg.to,
                            angle: cg.angle,
                          },
                        })
                      }
                      className="flex items-center gap-2 rounded-lg border border-border p-2 text-left hover:border-primary/50"
                    >
                      <span
                        className="size-6 shrink-0 rounded-md border border-border/80 shadow-xs"
                        style={{
                          background: `linear-gradient(${cg.angle}deg, ${cg.from}, ${cg.to})`,
                        }}
                      />
                      <span className="truncate text-xs font-medium">{cg.name}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
                {/* From Color */}
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium text-muted-foreground">Color 1</label>
                  <ColorField
                    value={gradFrom}
                    fallback="#050B16"
                    onCommit={(hex) =>
                      setOverrides({
                        background: { type: 'gradient', from: hex, to: gradTo, angle: gradAngle },
                      })
                    }
                  />
                </div>

                {/* To Color */}
                <div className="flex flex-col gap-1.5">
                  <label className="text-xs font-medium text-muted-foreground">Color 2</label>
                  <ColorField
                    value={gradTo}
                    fallback="#0B1428"
                    onCommit={(hex) =>
                      setOverrides({
                        background: { type: 'gradient', from: gradFrom, to: hex, angle: gradAngle },
                      })
                    }
                  />
                </div>
                {/* Angle slider */}
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs text-muted-foreground">
                    <label className="font-medium">Angle</label>
                    <span className="font-mono">{gradAngle}°</span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="360"
                    step="5"
                    value={gradAngle}
                    onChange={(e) =>
                      setOverrides({
                        background: {
                          type: 'gradient',
                          from: gradFrom,
                          to: gradTo,
                          angle: parseInt(e.target.value, 10),
                        },
                      })
                    }
                    className="h-8 cursor-pointer accent-primary"
                  />
                </div>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── 6. Live Preview Strip ──────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-primary" />
            <CardTitle className="text-base font-semibold">Live Preview Strip</CardTitle>
          </div>
          <CardDescription>
            Interactive components dynamically rendered with your active theme tokens.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap items-center gap-6 rounded-xl border border-border/70 bg-card/60 p-4 shadow-xs">
            {/* Nav item sample */}
            <div className="flex flex-col gap-1">
              <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                Sidebar Item
              </span>
              <div className="relative flex items-center gap-2.5 rounded-lg border border-primary/70 bg-primary/10 px-3 py-1.5 text-xs font-semibold text-foreground shadow-xs">
                <span className="absolute left-0 top-1 bottom-1 w-1 rounded-r-full bg-primary" />
                <DashboardIcon className="size-4" />
                <span>Dashboard</span>
              </div>
            </div>

            {/* Button sample */}
            <div className="flex flex-col gap-1">
              <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                Primary Button
              </span>
              <Button size="sm" className="h-8">
                Confirm Action
              </Button>
            </div>

            {/* Badge sample */}
            <div className="flex flex-col gap-1">
              <span className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                Badge
              </span>
              <div className="flex items-center gap-2">
                <Badge variant="default">Active</Badge>
                <Badge variant="secondary">Secondary</Badge>
              </div>
            </div>

            {/* Mini Card sample */}
            <div className="flex min-w-44 flex-1 flex-col gap-1 rounded-lg border border-border bg-card p-2.5 text-xs">
              <span className="font-semibold text-foreground">Sample Card Panel</span>
              <span className="text-muted-foreground">Cards remain solid & high contrast.</span>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* ── 7. Reset Button ────────────────────────────────────────────── */}
      <div className="flex items-center justify-between border-t border-border pt-4">
        <div>
          {resetConfirm && (
            <span className="text-xs font-medium text-emerald-600 dark:text-emerald-400">
              Appearance reset to defaults.
            </span>
          )}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={handleReset}
          className="gap-2 text-muted-foreground hover:text-foreground"
        >
          <RotateCcw className="size-3.5" />
          Reset to default
        </Button>
      </div>
    </div>
  )
}
