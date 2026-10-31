import { createContext, useContext, useEffect, useMemo, useState } from 'react'
import { emit } from '@tauri-apps/api/event'

import { runCommand } from '@/core'
import { applyThemeToDom, computeEffectiveOptions, computeEffectiveTokens } from './apply'
import { BUILTIN_PRESETS, DEFAULT_THEME_ID } from './presets'
import type { ThemeDefinition, ThemeMode, ThemeOptions, ThemeOverrides, ThemeSettings, ThemeTokens } from './types'
import { DEFAULT_THEME_SETTINGS, validateThemeSettings } from './validation'

export interface ThemeContextValue {
  settings: ThemeSettings
  mode: ThemeMode
  resolvedTheme: 'light' | 'dark'
  preset: ThemeDefinition
  tokens: ThemeTokens
  options: ThemeOptions
  setMode: (mode: ThemeMode) => void
  setPresetId: (presetId: string) => void
  setOverrides: (overrides: Partial<ThemeOverrides>) => void
  resetToDefault: () => void
  // Legacy aliases
  theme: ThemeMode
  setTheme: (theme: ThemeMode) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export const STORAGE_KEY_APPEARANCE = 'ols-appearance'
export const STORAGE_KEY_THEME_LEGACY = 'ols-theme'

function getSystemTheme(): 'light' | 'dark' {
  if (typeof window === 'undefined') return 'dark'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function loadInitialSettings(): ThemeSettings {
  if (typeof window === 'undefined') return DEFAULT_THEME_SETTINGS
  try {
    const rawAppearance = localStorage.getItem(STORAGE_KEY_APPEARANCE)
    if (rawAppearance) {
      const parsed = JSON.parse(rawAppearance)
      return validateThemeSettings(parsed)
    }
    // Fallback to legacy ols-theme key if appearance settings not found
    const legacyTheme = localStorage.getItem(STORAGE_KEY_THEME_LEGACY)
    if (legacyTheme === 'light' || legacyTheme === 'dark' || legacyTheme === 'system') {
      return { ...DEFAULT_THEME_SETTINGS, mode: legacyTheme }
    }
  } catch {
    // ignore
  }
  return DEFAULT_THEME_SETTINGS
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<ThemeSettings>(loadInitialSettings)
  const [systemIsDark, setSystemIsDark] = useState<boolean>(() => getSystemTheme() === 'dark')

  // Resolve active theme: light or dark
  const resolvedTheme: 'light' | 'dark' = useMemo(() => {
    if (settings.mode === 'system') {
      return systemIsDark ? 'dark' : 'light'
    }
    return settings.mode
  }, [settings.mode, systemIsDark])

  // Resolve preset definition
  const preset: ThemeDefinition = useMemo(() => {
    return (
      BUILTIN_PRESETS.find((p) => p.id === settings.presetId) ??
      BUILTIN_PRESETS.find((p) => p.id === DEFAULT_THEME_ID)!
    )
  }, [settings.presetId])

  // Derive effective tokens & options
  const tokens: ThemeTokens = useMemo(() => {
    return computeEffectiveTokens(preset, resolvedTheme, settings.overrides)
  }, [preset, resolvedTheme, settings.overrides])

  const options: ThemeOptions = useMemo(() => {
    return computeEffectiveOptions(preset, settings.overrides)
  }, [preset, settings.overrides])

  // Apply to DOM whenever tokens or options change
  useEffect(() => {
    applyThemeToDom(tokens, options, resolvedTheme)
    // Synchronize tray & window chrome in Rust
    void emit('ols:theme', resolvedTheme).catch(() => undefined)
  }, [tokens, options, resolvedTheme])

  // Follow OS system preference live when in 'system' mode
  useEffect(() => {
    if (typeof window === 'undefined') return
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (e: MediaQueryListEvent) => {
      setSystemIsDark(e.matches)
    }
    mql.addEventListener('change', onChange)
    return () => mql.removeEventListener('change', onChange)
  }, [])

  // Sync settings with portable data directory on boot
  useEffect(() => {
    let alive = true
    void runCommand({ type: 'get_setting', key: 'appearance' })
      .then((res) => {
        if (!alive) return
        if (res.type === 'setting' && res.value) {
          const validated = validateThemeSettings(res.value)
          setSettings(validated)
          try {
            localStorage.setItem(STORAGE_KEY_APPEARANCE, JSON.stringify(validated))
            localStorage.setItem(STORAGE_KEY_THEME_LEGACY, validated.mode)
          } catch {
            // ignore
          }
        }
      })
      .catch(() => undefined)
    return () => {
      alive = false
    }
  }, [])

  // Persist helper
  const saveSettings = (next: ThemeSettings) => {
    setSettings(next)
    try {
      localStorage.setItem(STORAGE_KEY_APPEARANCE, JSON.stringify(next))
      localStorage.setItem(STORAGE_KEY_THEME_LEGACY, next.mode)
    } catch {
      // ignore
    }
    void runCommand({ type: 'set_setting', key: 'appearance', value: next }).catch(() => undefined)
  }

  const setMode = (mode: ThemeMode) => {
    saveSettings({ ...settings, mode })
  }

  const setPresetId = (presetId: string) => {
    saveSettings({ ...settings, presetId })
  }

  const setOverrides = (patch: Partial<ThemeOverrides>) => {
    saveSettings({
      ...settings,
      overrides: {
        ...settings.overrides,
        ...patch,
      },
    })
  }

  const resetToDefault = () => {
    saveSettings(DEFAULT_THEME_SETTINGS)
  }

  const value: ThemeContextValue = {
    settings,
    mode: settings.mode,
    resolvedTheme,
    preset,
    tokens,
    options,
    setMode,
    setPresetId,
    setOverrides,
    resetToDefault,
    // Legacy compatibility
    theme: settings.mode,
    setTheme: setMode,
  }

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext)
  if (!ctx) {
    throw new Error('useTheme must be used within ThemeProvider')
  }
  return ctx
}

export function useThemeSafe(): ThemeContextValue | null {
  return useContext(ThemeContext)
}
