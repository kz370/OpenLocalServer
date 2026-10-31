import { BUILTIN_PRESETS, DEFAULT_THEME_ID } from './presets.ts'
import type { ThemeMode, ThemeOverrides, ThemeSettings, SidebarIconStyle } from './types.ts'
import { parseColor } from './color.ts'

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
    if (rawOverrides.background && typeof rawOverrides.background === 'object') {
      const bg = rawOverrides.background as Record<string, unknown>
      if (bg.type === 'solid') {
        overrides.background = { type: 'solid' }
      } else if (bg.type === 'gradient') {
        const from = typeof bg.from === 'string' && parseColor(bg.from) ? bg.from : '#050B16'
        const to = typeof bg.to === 'string' && parseColor(bg.to) ? bg.to : '#0B1428'
        const angle = typeof bg.angle === 'number' && Number.isFinite(bg.angle) ? bg.angle : 135
        overrides.background = { type: 'gradient', from, to, angle }
      }
    }
  }

  return { mode, presetId, overrides }
}
