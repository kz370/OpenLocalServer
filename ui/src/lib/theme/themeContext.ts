import { createContext, useContext } from 'react'

import type { ThemeDefinition, ThemeMode, ThemeOptions, ThemeOverrides, ThemeSettings, ThemeTokens } from './types'

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

export const ThemeContext = createContext<ThemeContextValue | null>(null)

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
