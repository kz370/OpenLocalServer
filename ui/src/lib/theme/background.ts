/**
 * Per-mode app background resolution.
 *
 * The background is stored separately for light and dark mode. A gradient saved for one
 * mode is adapted (same hue, mode-appropriate lightness) for the other mode until the user
 * sets that mode explicitly, so a dark gradient never ends up behind light glass cards.
 */
import { hslToHex, parseColor, relativeLuminance, rgbToHsl } from './color.ts'
import type { ThemeBackground, ThemeBackgroundOverrides } from './types.ts'

export type ResolvedMode = 'light' | 'dark'

type GradientBackground = Extract<ThemeBackground, { type: 'gradient' }>

/** A gradient whose average luminance is below this reads as a dark background. */
const DARK_MAX_LUMINANCE = 0.2
/** A gradient whose average luminance is at or above this reads as a light background. */
const LIGHT_MIN_LUMINANCE = 0.45

/** Default glass card opacity when the background suits the mode. */
const GLASS_ALPHA = { light: 0.6, dark: 0.45 } as const
/** Card opacity ceiling for the worst mismatch (black behind light mode / white behind dark mode). */
const GLASS_ALPHA_MAX = { light: 0.9, dark: 0.85 } as const

export function otherMode(mode: ResolvedMode): ResolvedMode {
  return mode === 'dark' ? 'light' : 'dark'
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v))
}

/** Average WCAG relative luminance of a gradient's two stops; null for solid or unparseable. */
export function gradientLuminance(bg: ThemeBackground): number | null {
  if (bg.type !== 'gradient') return null
  const from = parseColor(bg.from)
  const to = parseColor(bg.to)
  if (!from || !to) return null
  return (relativeLuminance(from) + relativeLuminance(to)) / 2
}

/** True when the background reads correctly behind this mode's text and glass. Solid always suits. */
export function backgroundSuitsMode(bg: ThemeBackground, mode: ResolvedMode): boolean {
  const lum = gradientLuminance(bg)
  if (lum === null) return true
  return mode === 'light' ? lum >= LIGHT_MIN_LUMINANCE : lum <= DARK_MAX_LUMINANCE
}

/**
 * Re-tone one color for a mode, keeping its hue: pastel for light, deep for dark.
 * Darker sources map to lighter results in light mode, so the gradient direction still reads.
 */
export function adaptColorForMode(color: string, mode: ResolvedMode): string {
  const c = parseColor(color)
  if (!c) return color
  const { h, s, l } = rgbToHsl(c)
  if (mode === 'light') {
    return hslToHex(h, Math.min(s, 0.75), clamp(0.95 - l * 0.25, 0.86, 0.97))
  }
  return hslToHex(h, Math.min(s, 0.7), clamp(0.05 + (1 - l) * 0.15, 0.04, 0.16))
}

/** Return the gradient unchanged when it suits the mode, otherwise its re-toned counterpart. */
export function adaptGradientForMode(bg: GradientBackground, mode: ResolvedMode): GradientBackground {
  if (backgroundSuitsMode(bg, mode)) return bg
  return {
    type: 'gradient',
    from: adaptColorForMode(bg.from, mode),
    to: adaptColorForMode(bg.to, mode),
    angle: bg.angle,
  }
}

/**
 * Pick the background for a mode:
 * 1. the user's explicit choice for that mode;
 * 2. else the other mode's gradient, adapted to this mode;
 * 3. else the preset default.
 */
export function resolveBackgroundForMode(
  presetBackground: ThemeBackground,
  overrides: ThemeBackgroundOverrides | undefined,
  mode: ResolvedMode,
): ThemeBackground {
  const explicit = overrides?.[mode]
  if (explicit) return explicit
  const other = overrides?.[otherMode(mode)]
  if (other?.type === 'gradient') return adaptGradientForMode(other, mode)
  return presetBackground
}

/** True when this mode shows a gradient adapted from the other mode rather than one set for it. */
export function isBackgroundInherited(
  overrides: ThemeBackgroundOverrides | undefined,
  mode: ResolvedMode,
): boolean {
  return !overrides?.[mode] && overrides?.[otherMode(mode)]?.type === 'gradient'
}

/**
 * Glass card opacity for a background. Matched backgrounds keep the light frosted look;
 * a mismatched one (e.g. a dark gradient set explicitly for light mode) gets thicker glass,
 * scaled by how far off it is, so text stays readable while blur is kept.
 */
export function glassAlphaFor(bg: ThemeBackground, mode: ResolvedMode): number {
  const lum = gradientLuminance(bg)
  if (lum === null || backgroundSuitsMode(bg, mode)) return GLASS_ALPHA[mode]
  const miss =
    mode === 'light'
      ? (LIGHT_MIN_LUMINANCE - lum) / LIGHT_MIN_LUMINANCE
      : (lum - DARK_MAX_LUMINANCE) / (1 - DARK_MAX_LUMINANCE)
  const alpha = GLASS_ALPHA[mode] + clamp(miss, 0, 1) * (GLASS_ALPHA_MAX[mode] - GLASS_ALPHA[mode])
  return Math.round(alpha * 100) / 100
}
