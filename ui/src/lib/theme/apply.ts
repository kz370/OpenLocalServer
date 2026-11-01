import { adjustAccentForMode, hexToRgba, pickReadableForeground } from './color.ts'
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
  } else {
    root.style.removeProperty('--gradient-from')
    root.style.removeProperty('--gradient-to')
    root.style.removeProperty('--gradient-angle')
    root.style.setProperty('--app-background', 'var(--background)')
    root.dataset.bgType = 'solid'
  }

  // 5. Sidebar icon style marker
  root.dataset.sidebarIcons = options.sidebarIcons
}
