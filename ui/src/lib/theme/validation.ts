import { BUILTIN_PRESETS, DEFAULT_THEME_ID } from './presets.ts'
import type {
  ThemeBackground,
  ThemeBackgroundOverrides,
  ThemeMode,
  ThemeOverrides,
  ThemeSettings,
  SidebarIconStyle,
} from './types.ts'
import { parseColor } from './color.ts'
import { backgroundSuitsMode } from './background.ts'

export const DEFAULT_THEME_SETTINGS: ThemeSettings = {
  mode: 'system',
  presetId: DEFAULT_THEME_ID,
  overrides: {},
}

const VALID_MODES: readonly ThemeMode[] = ['system', 'light', 'dark']
const VALID_ICON_STYLES: readonly SidebarIconStyle[] = ['monochrome', 'colorful']

/**
 * Validate and sanitize theme settings.
 * If anything is missing, corrupted, or invalid, gracefully fall back to defaults.
 * Never throws an error.
 */
export function validateThemeSettings(raw: unknown): ThemeSettings {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_THEME_SETTINGS }
  }

  const candidate = raw as Record<string, unknown>

  // 1. Mode
  const mode: ThemeMode =
    typeof candidate.mode === 'string' && VALID_MODES.includes(candidate.mode as ThemeMode)
      ? (candidate.mode as ThemeMode)
      : 'system'

  // 2. Preset ID
  const presetId: string =
    typeof candidate.presetId === 'string' && BUILTIN_PRESETS.some((p) => p.id === candidate.presetId)
      ? candidate.presetId
      : DEFAULT_THEME_ID

  // 3. Overrides
  const overrides: ThemeOverrides = {}
  if (candidate.overrides && typeof candidate.overrides === 'object') {
    const rawOverrides = candidate.overrides as Record<string, unknown>

    // Accent override
    if (typeof rawOverrides.accent === 'string' && parseColor(rawOverrides.accent)) {
      overrides.accent = rawOverrides.accent
    }

    // Sidebar icon style override
    if (
      typeof rawOverrides.sidebarIcons === 'string' &&
      VALID_ICON_STYLES.includes(rawOverrides.sidebarIcons as SidebarIconStyle)
    ) {
      overrides.sidebarIcons = rawOverrides.sidebarIcons as SidebarIconStyle
    }

    // Background override
    const background = parseBackgroundOverrides(rawOverrides.background)
    if (background) overrides.background = background
  }

  return { mode, presetId, overrides }
}

function parseBackground(raw: unknown): ThemeBackground | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const bg = raw as Record<string, unknown>
  if (bg.type === 'solid') return { type: 'solid' }
  if (bg.type === 'gradient') {
    const from = typeof bg.from === 'string' && parseColor(bg.from) ? bg.from : '#050B16'
    const to = typeof bg.to === 'string' && parseColor(bg.to) ? bg.to : '#0B1428'
    const angle = typeof bg.angle === 'number' && Number.isFinite(bg.angle) ? bg.angle : 135
    return { type: 'gradient', from, to, angle }
  }
  return undefined
}

/**
 * Accepts the per-mode shape `{ light?, dark? }` and migrates the legacy single
 * background (one value shared by both modes): a solid applies to both modes, a
 * gradient is kept for the mode it was designed for (by luminance) and the other
 * mode inherits an adapted copy.
 */
function parseBackgroundOverrides(raw: unknown): ThemeBackgroundOverrides | undefined {
  if (!raw || typeof raw !== 'object') return undefined
  const candidate = raw as Record<string, unknown>

  if ('type' in candidate) {
    const legacy = parseBackground(candidate)
    if (!legacy) return undefined
    if (legacy.type === 'solid') return { light: legacy, dark: legacy }
    return backgroundSuitsMode(legacy, 'light') ? { light: legacy } : { dark: legacy }
  }

  const result: ThemeBackgroundOverrides = {}
  const light = parseBackground(candidate.light)
  const dark = parseBackground(candidate.dark)
  if (light) result.light = light
  if (dark) result.dark = dark
  return light || dark ? result : undefined
}
