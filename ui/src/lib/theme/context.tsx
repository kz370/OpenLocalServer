import { useEffect, useMemo, useRef, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { runCommand } from '@/core'
import { applyThemeToDom, computeEffectiveOptions, computeEffectiveTokens } from './apply'
import { BUILTIN_PRESETS, DEFAULT_THEME_ID } from './presets'
import type { ThemeDefinition, ThemeMode, ThemeOptions, ThemeOverrides, ThemeSettings, ThemeTokens } from './types'
import { ThemeContext, type ThemeContextValue } from './themeContext'
import { DEFAULT_THEME_SETTINGS, validateThemeSettings } from './validation'


const STORAGE_KEY_APPEARANCE = 'ols-appearance'
const STORAGE_KEY_THEME_LEGACY = 'ols-theme'

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
    return computeEffectiveOptions(preset, settings.overrides, resolvedTheme)
  }, [preset, settings.overrides, resolvedTheme])

  // Apply to DOM whenever tokens or options change
  useEffect(() => {
    applyThemeToDom(tokens, options, resolvedTheme)
  }, [tokens, options, resolvedTheme])

  // Synchronize tray & window chrome in Rust (only when light/dark actually flips)
  useEffect(() => {
    void emit('ols:theme', resolvedTheme).catch(() => undefined)
  }, [resolvedTheme])

  useEffect(() => {
    if (getCurrentWindow().label !== 'main') return
    void emit('ols:tray-confirmation-theme', settings).catch((error: unknown) => {
      console.error('Could not sync the tray confirmation theme:', error)
    })
  }, [settings])

  useEffect(() => {
    if (getCurrentWindow().label !== 'tray-confirmation') return
    let unlisten: (() => void) | undefined
    void listen<unknown>('ols:tray-confirmation-theme', (event) => {
      setSettings(validateThemeSettings(event.payload))
    }).then((cleanup) => {
      unlisten = cleanup
    }).catch((error: unknown) => {
      console.error('Could not listen for tray confirmation theme changes:', error)
    })
    return () => unlisten?.()
  }, [])

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
  // Always points at the latest settings so rapid successive updates (color drags)
  // never build on a stale closure.
  const settingsRef = useRef(settings)
  settingsRef.current = settings
  const persistTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const persist = (next: ThemeSettings) => {
    try {
      localStorage.setItem(STORAGE_KEY_APPEARANCE, JSON.stringify(next))
      localStorage.setItem(STORAGE_KEY_THEME_LEGACY, next.mode)
    } catch {
      // ignore
    }
    void runCommand({ type: 'set_setting', key: 'appearance', value: next }).catch(() => undefined)
  }

  // Apply immediately, but debounce disk/IPC writes so dragging is cheap.
  const saveSettings = (next: ThemeSettings) => {
    settingsRef.current = next
    setSettings(next)
    if (persistTimer.current) clearTimeout(persistTimer.current)
    persistTimer.current = setTimeout(() => {
      persistTimer.current = null
      persist(settingsRef.current)
    }, 300)
  }

  const setMode = (mode: ThemeMode) => {
    saveSettings({ ...settingsRef.current, mode })
  }

  const setPresetId = (presetId: string) => {
    saveSettings({ ...settingsRef.current, presetId })
  }

  const setOverrides = (patch: Partial<ThemeOverrides>) => {
    const cur = settingsRef.current
    saveSettings({
      ...cur,
      overrides: {
        ...cur.overrides,
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
