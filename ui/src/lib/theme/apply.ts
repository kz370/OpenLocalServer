import { adjustAccentForMode, hexToRgba, parseColor, pickReadableForeground } from './color.ts'
import type { ThemeDefinition, ThemeOptions, ThemeOverrides, ThemeTokens } from './types.ts'

/**
 * Combines preset tokens with user overrides for the given mode.
 */
export function computeEffectiveTokens(
  preset: ThemeDefinition,
  resolvedMode: 'light' | 'dark',
  overrides: ThemeOverrides,
): ThemeTokens {
  const base = { ...preset[resolvedMode] }

  if (overrides.accent) {
    const accent = adjustAccentForMode(overrides.accent, resolvedMode)
    base.primary = accent
    base.primaryForeground = pickReadableForeground(accent, '#FFFFFF', '#04120C')
    base.ring = hexToRgba(accent, resolvedMode === 'dark' ? 0.45 : 0.4)
    base.sidebarAccentForeground = accent
  }

  return base
}

/**
 * Combines preset options with user overrides.
 */
export function computeEffectiveOptions(
  preset: ThemeDefinition,
  overrides: ThemeOverrides,
): ThemeOptions {
  return {
    sidebarIcons: overrides.sidebarIcons ?? preset.options.sidebarIcons,
    background: overrides.background ?? preset.options.background,
    radius: preset.options.radius,
  }
}

/**
 * Maps camelCase ThemeTokens keys to CSS custom properties.
 */
const TOKEN_CSS_MAP: Record<keyof ThemeTokens, string> = {
  background: '--background',
  foreground: '--foreground',
  card: '--card',
  cardForeground: '--card-foreground',
  popover: '--popover',
  popoverForeground: '--popover-foreground',
  primary: '--primary',
  primaryForeground: '--primary-foreground',
  secondary: '--secondary',
  secondaryForeground: '--secondary-foreground',
  muted: '--muted',
  mutedForeground: '--muted-foreground',
  accent: '--accent',
  accentForeground: '--accent-foreground',
  destructive: '--destructive',
  destructiveForeground: '--destructive-foreground',
  success: '--success',
  successForeground: '--success-foreground',
  warning: '--warning',
  info: '--info',
  border: '--border',
  input: '--input',
  ring: '--ring',
  sidebar: '--sidebar',
  sidebarForeground: '--sidebar-foreground',
  sidebarBorder: '--sidebar-border',
  sidebarAccent: '--sidebar-accent',
  sidebarAccentForeground: '--sidebar-accent-foreground',
}

/**
 * Injects computed theme tokens and options into document root.
 */
export function applyThemeToDom(
  tokens: ThemeTokens,
  options: ThemeOptions,
  resolvedMode: 'light' | 'dark',
): void {
  if (typeof document === 'undefined') return

  const root = document.documentElement

  // 1. Toggle dark class and sync colorScheme
  root.classList.toggle('dark', resolvedMode === 'dark')
  root.style.colorScheme = resolvedMode

  // 2. Set tokens
  for (const [key, cssVar] of Object.entries(TOKEN_CSS_MAP) as [keyof ThemeTokens, string][]) {
    const val = tokens[key]
    if (val) {
      root.style.setProperty(cssVar, val)
    }
  }

  // 3. Set radius
  if (options.radius) {
    root.style.setProperty('--radius', options.radius)
  }

  // 4. Background options
  if (options.background.type === 'gradient') {
    const { from, to, angle } = options.background
    root.style.setProperty('--gradient-from', from)
    root.style.setProperty('--gradient-to', to)
    root.style.setProperty('--gradient-angle', `${angle}deg`)
    root.style.setProperty('--app-background', `linear-gradient(${angle}deg, ${from}, ${to})`)
    root.dataset.bgType = 'gradient'

    // Subtle frosted acrylic card surface when gradient mode is active
    const cardColor = parseColor(tokens.card)
    if (cardColor && tokens.card !== '#000000') {
      const glassAlpha = resolvedMode === 'dark' ? 0.45 : 0.6
      root.style.setProperty('--card', `rgba(${cardColor.r}, ${cardColor.g}, ${cardColor.b}, ${glassAlpha})`)
    }
  } else {
    root.style.removeProperty('--gradient-from')
    root.style.removeProperty('--gradient-to')
    root.style.removeProperty('--gradient-angle')
    root.style.setProperty('--app-background', 'var(--background)')
    root.dataset.bgType = 'solid'
  }

  // 5. Sidebar icon style marker
  root.dataset.sidebarIcons = options.sidebarIcons

  // 6. Solid, 100% opaque dialog and modal surface
  const bgRgb = parseColor(tokens.background) ?? { r: 255, g: 255, b: 255, a: 1 }
  const cardRgb = parseColor(tokens.card) ?? bgRgb
  const solidR = Math.round(cardRgb.r * cardRgb.a + bgRgb.r * (1 - cardRgb.a))
  const solidG = Math.round(cardRgb.g * cardRgb.a + bgRgb.g * (1 - cardRgb.a))
  const solidB = Math.round(cardRgb.b * cardRgb.a + bgRgb.b * (1 - cardRgb.a))
  const solidDialog = `rgb(${solidR}, ${solidG}, ${solidB})`

  const isDark = resolvedMode === 'dark'
  const delta = isDark ? 8 : -6
  const innerR = Math.min(255, Math.max(0, solidR + delta))
  const innerG = Math.min(255, Math.max(0, solidG + delta))
  const innerB = Math.min(255, Math.max(0, solidB + (isDark ? delta + 4 : delta)))
  const solidInnerCard = `rgb(${innerR}, ${innerG}, ${innerB})`

  root.style.setProperty('--dialog-surface', solidDialog)
  root.style.setProperty('--dialog-card', solidInnerCard)
}
