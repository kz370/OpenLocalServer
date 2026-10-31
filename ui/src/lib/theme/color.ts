/**
 * Pure color and contrast calculation helpers.
 * Independent of the UI so it can be reused in any environment.
 */

export interface RgbaColor {
  r: number
  g: number
  b: number
  a: number
}

/**
 * Parse hex or rgb/rgba string into RgbaColor components (0-255 for RGB, 0-1 for A).
 */
export function parseColor(input: string): RgbaColor | null {
  if (!input) return null
  const trimmed = input.trim().toLowerCase()

  // Hex (#RGB, #RGBA, #RRGGBB, #RRGGBBAA)
  if (trimmed.startsWith('#')) {
    const raw = trimmed.slice(1)
    if (raw.length === 3) {
      const r = parseInt(raw[0] + raw[0], 16)
      const g = parseInt(raw[1] + raw[1], 16)
      const b = parseInt(raw[2] + raw[2], 16)
      return { r, g, b, a: 1 }
    }
    if (raw.length === 4) {
      const r = parseInt(raw[0] + raw[0], 16)
      const g = parseInt(raw[1] + raw[1], 16)
      const b = parseInt(raw[2] + raw[2], 16)
      const a = parseInt(raw[3] + raw[3], 16) / 255
      return { r, g, b, a }
    }
    if (raw.length === 6) {
      const r = parseInt(raw.slice(0, 2), 16)
      const g = parseInt(raw.slice(2, 4), 16)
      const b = parseInt(raw.slice(4, 6), 16)
      return { r, g, b, a: 1 }
    }
    if (raw.length === 8) {
      const r = parseInt(raw.slice(0, 2), 16)
      const g = parseInt(raw.slice(2, 4), 16)
      const b = parseInt(raw.slice(4, 6), 16)
      const a = parseInt(raw.slice(6, 8), 16) / 255
      return { r, g, b, a }
    }
    return null
  }

  // rgb(r, g, b) or rgba(r, g, b, a)
  const rgbMatch = trimmed.match(/^rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)$/)
  if (rgbMatch) {
    const r = Math.min(255, Math.max(0, parseInt(rgbMatch[1], 10)))
    const g = Math.min(255, Math.max(0, parseInt(rgbMatch[2], 10)))
    const b = Math.min(255, Math.max(0, parseInt(rgbMatch[3], 10)))
    const a = rgbMatch[4] !== undefined ? Math.min(1, Math.max(0, parseFloat(rgbMatch[4]))) : 1
    return { r, g, b, a }
  }

  return null
}

/**
 * Calculates WCAG 2.1 relative luminance for an RGB color.
 * Formula: 0.2126*R + 0.7152*G + 0.0722*B
 */
export function relativeLuminance(rgb: { r: number; g: number; b: number }): number {
  const [r, g, b] = [rgb.r, rgb.g, rgb.b].map((v) => {
    const s = v / 255
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4)
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/**
 * Calculates WCAG 2.1 contrast ratio between two colors (ranging from 1.0 to 21.0).
 */
export function contrastRatio(c1: string, c2: string): number {
  const p1 = parseColor(c1)
  const p2 = parseColor(c2)
  if (!p1 || !p2) return 1.0

  const l1 = relativeLuminance(p1)
  const l2 = relativeLuminance(p2)
  const lighter = Math.max(l1, l2)
  const darker = Math.min(l1, l2)

  return (lighter + 0.05) / (darker + 0.05)
}

/**
 * Pick readable foreground color for a given background (white or dark slate).
 * Ensures maximum contrast according to WCAG.
 */
export function pickReadableForeground(
  bg: string,
  lightCandidate = '#FFFFFF',
  darkCandidate = '#0F172A',
): string {
  const ratioLight = contrastRatio(bg, lightCandidate)
  const ratioDark = contrastRatio(bg, darkCandidate)
  return ratioLight >= ratioDark ? lightCandidate : darkCandidate
}

/**
 * Format an RGB color and opacity as an rgba() string.
 */
export function hexToRgba(hex: string, alpha: number): string {
  const c = parseColor(hex)
  if (!c) return hex
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${alpha})`
}

/**
 * Check if an accent color provides sufficient contrast against card / background.
 * WCAG AA requires >= 4.5:1 for normal text, >= 3.0:1 for graphical UI objects and large text.
 */
export function getContrastWarning(
  accent: string,
  isDark: boolean,
): { isLow: boolean; ratio: number; message: string | null } {
  const bgToTest = isDark ? '#050B16' : '#F8FAFF'
  const ratio = contrastRatio(accent, bgToTest)

  if (ratio < 2.5) {
    return {
      isLow: true,
      ratio: Math.round(ratio * 10) / 10,
      message: `Poor contrast (${ratio.toFixed(1)}:1 against background). Text or icons with this accent may be hard to distinguish.`,
    }
  }

  if (ratio < 3.5) {
    return {
      isLow: true,
      ratio: Math.round(ratio * 10) / 10,
      message: `Moderate contrast (${ratio.toFixed(1)}:1). Consider choosing a slightly ${isDark ? 'brighter' : 'darker'} tone for optimal readability.`,
    }
  }

  return {
    isLow: false,
    ratio: Math.round(ratio * 10) / 10,
    message: null,
  }
}

/**
 * Adjust accent color per mode if needed (e.g. slight boost for dark mode if user selected a very dark shade,
 * or slight darkening in light mode if user selected a very pale shade).
 */
export function adjustAccentForMode(accent: string, mode: 'light' | 'dark'): string {
  const parsed = parseColor(accent)
  if (!parsed) return accent

  const lum = relativeLuminance(parsed)
  // In dark mode: if luminance is too low (< 0.15), brighten it slightly
  if (mode === 'dark' && lum < 0.15) {
    const factor = 1.35
    const r = Math.min(255, Math.round(parsed.r * factor + 30))
    const g = Math.min(255, Math.round(parsed.g * factor + 30))
    const b = Math.min(255, Math.round(parsed.b * factor + 30))
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
  }

  // In light mode: if luminance is very high (> 0.7), darken it slightly
  if (mode === 'light' && lum > 0.7) {
    const factor = 0.75
    const r = Math.max(0, Math.round(parsed.r * factor))
    const g = Math.max(0, Math.round(parsed.g * factor))
    const b = Math.max(0, Math.round(parsed.b * factor))
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
  }

  return accent
}
