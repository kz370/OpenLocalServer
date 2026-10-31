export type ThemeMode = 'system' | 'light' | 'dark'

export interface ThemeTokens {
  background: string
  foreground: string
  card: string
  cardForeground: string
  popover: string
  popoverForeground: string
  primary: string
  primaryForeground: string
  secondary: string
  secondaryForeground: string
  muted: string
  mutedForeground: string
  accent: string
  accentForeground: string
  destructive: string
  destructiveForeground: string
  success: string
  successForeground: string
  warning: string
  info: string
  border: string
  input: string
  ring: string
  sidebar: string
  sidebarForeground: string
  sidebarBorder: string
  sidebarAccent: string
  sidebarAccentForeground: string
}

export type SidebarIconStyle = 'monochrome' | 'colorful'

export type ThemeBackground =
  | { type: 'solid' }
  | { type: 'gradient'; from: string; to: string; angle: number }

export interface ThemeOptions {
  sidebarIcons: SidebarIconStyle
  background: ThemeBackground
  radius: string
}

export interface ThemeDefinition {
  id: string
  name: string
  schemaVersion: number
  light: ThemeTokens
  dark: ThemeTokens
  options: ThemeOptions
}

export interface ThemeOverrides {
  accent?: string
  sidebarIcons?: SidebarIconStyle
  background?: ThemeBackground
}

export interface ThemeSettings {
  mode: ThemeMode
  presetId: string
  overrides: ThemeOverrides
}
