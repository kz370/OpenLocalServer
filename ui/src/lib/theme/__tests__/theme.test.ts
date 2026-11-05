import { describe, it } from 'node:test'
import assert from 'node:assert/strict'

import {
  parseColor,
  relativeLuminance,
  contrastRatio,
  pickReadableForeground,
  getContrastWarning,
  adjustAccentForMode,
  rgbToHsl,
  hslToHex,
} from '../color.ts'
import {
  adaptGradientForMode,
  backgroundSuitsMode,
  glassAlphaFor,
  isBackgroundInherited,
  resolveBackgroundForMode,
} from '../background.ts'
import { BUILTIN_PRESETS, DEFAULT_THEME_ID } from '../presets.ts'
import { DEFAULT_THEME_SETTINGS, validateThemeSettings } from '../validation.ts'
import { computeEffectiveTokens, computeEffectiveOptions } from '../apply.ts'

describe('Theme Color & Contrast Helpers', () => {
  it('parses hex colors in 3, 4, 6, and 8 digit formats', () => {
    assert.deepEqual(parseColor('#fff'), { r: 255, g: 255, b: 255, a: 1 })
    assert.deepEqual(parseColor('#000000'), { r: 0, g: 0, b: 0, a: 1 })
    assert.deepEqual(parseColor('#059669'), { r: 5, g: 150, b: 105, a: 1 })
    assert.deepEqual(parseColor('#12345678'), {
      r: 18,
      g: 52,
      b: 86,
      a: 120 / 255,
    })
    assert.equal(parseColor('not-a-color'), null)
    assert.equal(parseColor(''), null)
  })

  it('calculates WCAG relative luminance correctly', () => {
    // Pure black has luminance 0.0
    const black = parseColor('#000000')!
    assert.equal(relativeLuminance(black), 0)

    // Pure white has luminance 1.0
    const white = parseColor('#ffffff')!
    assert.equal(Math.round(relativeLuminance(white) * 100) / 100, 1)

    // Mid-gray (#808080)
    const gray = parseColor('#808080')!
    const grayLum = relativeLuminance(gray)
    assert.ok(grayLum > 0.2 && grayLum < 0.25)
  })

  it('calculates WCAG contrast ratio correctly', () => {
    // Black on white is 21:1
    const maxRatio = contrastRatio('#000000', '#ffffff')
    assert.equal(Math.round(maxRatio), 21)

    // Identical colors have 1:1 contrast
    const minRatio = contrastRatio('#059669', '#059669')
    assert.equal(minRatio, 1)

    // White on dark navy (#050B16) is high contrast (> 15:1)
    const navyRatio = contrastRatio('#ffffff', '#050B16')
    assert.ok(navyRatio > 15)
  })

  it('auto-picks readable foreground for light and dark backgrounds', () => {
    // Dark backgrounds should pick white foreground
    assert.equal(pickReadableForeground('#050B16'), '#FFFFFF')
    assert.equal(pickReadableForeground('#0F172A'), '#FFFFFF')
    assert.equal(pickReadableForeground('#064E3B'), '#FFFFFF') // Deep forest emerald

    // Medium/bright backgrounds should pick dark slate foreground
    assert.equal(pickReadableForeground('#FFFFFF'), '#0F172A')
    assert.equal(pickReadableForeground('#F8FAFF'), '#0F172A')
    assert.equal(pickReadableForeground('#FDE047'), '#0F172A') // Bright yellow
    assert.equal(pickReadableForeground('#059669'), '#0F172A') // Mid emerald (4.74:1 vs 3.81:1)
  })

  it('evaluates contrast warning for accent colors', () => {
    // High contrast accent in dark mode (bright cyan #00E599 against #050B16)
    const good = getContrastWarning('#00E599', true)
    assert.equal(good.isLow, false)

    // Very dark accent in dark mode (#0a0f1d against #050B16) gives poor contrast warning
    const poor = getContrastWarning('#0a0f1d', true)
    assert.equal(poor.isLow, true)
    assert.ok(poor.message !== null)

    // Mid-gray accent with low contrast against on-accent text (#777777)
    const midTone = getContrastWarning('#777777', false)
    assert.equal(midTone.isLow, true)
    assert.ok(midTone.message?.includes('contrast'))
  })

  it('adjusts accent per mode if needed', () => {
    // Very dark navy in dark mode gets brightened
    const adjustedDark = adjustAccentForMode('#050B16', 'dark')
    assert.notEqual(adjustedDark, '#050B16')

    // Very bright yellow in light mode gets darkened
    const adjustedLight = adjustAccentForMode('#FFFFE0', 'light')
    assert.notEqual(adjustedLight, '#FFFFE0')

    // Normal balanced accent passes through unchanged
    assert.equal(adjustAccentForMode('#059669', 'light'), '#059669')
  })
})

describe('Theme Settings Validation and Fallback', () => {
  it('falls back to default settings when input is null or undefined or invalid', () => {
    assert.deepEqual(validateThemeSettings(null), DEFAULT_THEME_SETTINGS)
    assert.deepEqual(validateThemeSettings(undefined), DEFAULT_THEME_SETTINGS)
    assert.deepEqual(validateThemeSettings('string'), DEFAULT_THEME_SETTINGS)
    assert.deepEqual(validateThemeSettings({}), DEFAULT_THEME_SETTINGS)
  })

  it('validates mode and falls back if unrecognized', () => {
    assert.equal(validateThemeSettings({ mode: 'light' }).mode, 'light')
    assert.equal(validateThemeSettings({ mode: 'dark' }).mode, 'dark')
    assert.equal(validateThemeSettings({ mode: 'system' }).mode, 'system')
    assert.equal(validateThemeSettings({ mode: 'neon' }).mode, 'system')
  })

  it('validates presetId and falls back if unrecognized', () => {
    assert.equal(validateThemeSettings({ presetId: 'midnight' }).presetId, 'midnight')
    assert.equal(validateThemeSettings({ presetId: 'high-contrast' }).presetId, 'high-contrast')
    assert.equal(validateThemeSettings({ presetId: 'warm' }).presetId, 'warm')
    assert.equal(validateThemeSettings({ presetId: 'non-existent' }).presetId, DEFAULT_THEME_ID)
  })

  it('validates and sanitizes overrides', () => {
    // Valid accent override
    const validAccent = validateThemeSettings({
      overrides: { accent: '#6366F1' },
    })
    assert.equal(validAccent.overrides.accent, '#6366F1')

    // Invalid accent override dropped
    const invalidAccent = validateThemeSettings({
      overrides: { accent: 'invalid-color' },
    })
    assert.equal(invalidAccent.overrides.accent, undefined)

    // Valid sidebar icon style
    const monoIcons = validateThemeSettings({
      overrides: { sidebarIcons: 'monochrome' },
    })
    assert.equal(monoIcons.overrides.sidebarIcons, 'monochrome')

    // Invalid icon style dropped
    const invalidIcons = validateThemeSettings({
      overrides: { sidebarIcons: '3d-icons' },
    })
    assert.equal(invalidIcons.overrides.sidebarIcons, undefined)

    // Valid per-mode background
    const perMode = validateThemeSettings({
      overrides: {
        background: {
          dark: { type: 'gradient', from: '#050B16', to: '#0B1428', angle: 90 },
          light: { type: 'solid' },
          sepia: { type: 'solid' },
        },
      },
    })
    assert.deepEqual(perMode.overrides.background, {
      dark: { type: 'gradient', from: '#050B16', to: '#0B1428', angle: 90 },
      light: { type: 'solid' },
    })

    // Empty / garbage per-mode background is dropped
    const emptyBg = validateThemeSettings({ overrides: { background: { light: 'x' } } })
    assert.equal(emptyBg.overrides.background, undefined)
  })

  it('migrates the legacy single background to the per-mode shape', () => {
    // Dark legacy gradient belongs to dark mode; light mode inherits an adapted copy
    const darkLegacy = validateThemeSettings({
      overrides: { background: { type: 'gradient', from: '#050B16', to: '#083344', angle: 135 } },
    })
    assert.deepEqual(darkLegacy.overrides.background, {
      dark: { type: 'gradient', from: '#050B16', to: '#083344', angle: 135 },
    })

    // Light legacy gradient belongs to light mode
    const lightLegacy = validateThemeSettings({
      overrides: { background: { type: 'gradient', from: '#F8FAFF', to: '#E0F2FE', angle: 45 } },
    })
    assert.deepEqual(lightLegacy.overrides.background, {
      light: { type: 'gradient', from: '#F8FAFF', to: '#E0F2FE', angle: 45 },
    })

    // Legacy solid applies to both modes
    const solidLegacy = validateThemeSettings({ overrides: { background: { type: 'solid' } } })
    assert.deepEqual(solidLegacy.overrides.background, {
      light: { type: 'solid' },
      dark: { type: 'solid' },
    })
  })
})

describe('Per-mode Background Resolution', () => {
  const cyberAurora = { type: 'gradient' as const, from: '#050B16', to: '#083344', angle: 135 }

  it('converts between RGB and HSL', () => {
    const hsl = rgbToHsl({ r: 8, g: 51, b: 68 })
    assert.equal(hslToHex(hsl.h, hsl.s, hsl.l), '#083344')
    assert.equal(hslToHex(0, 0, 1), '#ffffff')
    assert.equal(hslToHex(0, 0, 0), '#000000')
  })

  it('judges whether a background suits a mode', () => {
    assert.equal(backgroundSuitsMode({ type: 'solid' }, 'light'), true)
    assert.equal(backgroundSuitsMode(cyberAurora, 'dark'), true)
    assert.equal(backgroundSuitsMode(cyberAurora, 'light'), false)
  })

  it('adapts a dark gradient to a light one with the same hue and angle', () => {
    const light = adaptGradientForMode(cyberAurora, 'light')
    assert.equal(light.angle, 135)
    assert.ok(backgroundSuitsMode(light, 'light'))
    const srcHue = rgbToHsl(parseColor(cyberAurora.to)!).h
    const outHue = rgbToHsl(parseColor(light.to)!).h
    assert.ok(Math.abs(srcHue - outHue) < 3)
    // Round trip back to dark suits dark
    assert.ok(backgroundSuitsMode(adaptGradientForMode(light, 'dark'), 'dark'))
    // Already-suitable gradient passes through unchanged
    assert.equal(adaptGradientForMode(cyberAurora, 'dark'), cyberAurora)
  })

  it('resolves explicit, inherited and preset backgrounds per mode', () => {
    const solid = { type: 'solid' as const }
    assert.deepEqual(resolveBackgroundForMode(solid, { dark: cyberAurora }, 'dark'), cyberAurora)
    const inherited = resolveBackgroundForMode(solid, { dark: cyberAurora }, 'light')
    assert.equal(inherited.type, 'gradient')
    assert.ok(backgroundSuitsMode(inherited, 'light'))
    assert.deepEqual(resolveBackgroundForMode(solid, { dark: cyberAurora, light: solid }, 'light'), solid)
    assert.deepEqual(resolveBackgroundForMode(solid, undefined, 'light'), solid)
    assert.equal(isBackgroundInherited({ dark: cyberAurora }, 'light'), true)
    assert.equal(isBackgroundInherited({ dark: cyberAurora }, 'dark'), false)
  })

  it('keeps default glass opacity when matched and thickens it on a mismatch', () => {
    assert.equal(glassAlphaFor({ type: 'solid' }, 'light'), 0.6)
    assert.equal(glassAlphaFor(cyberAurora, 'dark'), 0.45)
    const mismatched = glassAlphaFor(cyberAurora, 'light')
    assert.ok(mismatched > 0.8 && mismatched <= 0.9)
    const black = { type: 'gradient' as const, from: '#000000', to: '#000000', angle: 0 }
    assert.equal(glassAlphaFor(black, 'light'), 0.9)
  })
})

describe('Preset Tokens and Effective Derivation', () => {
  it('contains all required presets with complete tokens', () => {
    assert.ok(BUILTIN_PRESETS.length >= 4)
    const presetIds = BUILTIN_PRESETS.map((p) => p.id)
    assert.ok(presetIds.includes('default'))
    assert.ok(presetIds.includes('high-contrast'))
    assert.ok(presetIds.includes('midnight'))
    assert.ok(presetIds.includes('warm'))

    for (const p of BUILTIN_PRESETS) {
      assert.ok(p.light.background)
      assert.ok(p.light.foreground)
      assert.ok(p.light.primary)
      assert.ok(p.light.card)
      assert.ok(p.dark.background)
      assert.ok(p.dark.foreground)
      assert.ok(p.dark.primary)
      assert.ok(p.dark.card)
    }
  })

  it('computes effective tokens with accent override', () => {
    const defaultPreset = BUILTIN_PRESETS.find((p) => p.id === 'default')!
    const effective = computeEffectiveTokens(defaultPreset, 'dark', {
      accent: '#6366F1',
    })
    assert.equal(effective.primary, '#6366F1')
    assert.ok(effective.primaryForeground)
    assert.ok(effective.ring.includes('rgba'))
    assert.equal(effective.sidebarAccentForeground, '#6366F1')
  })

  it('computes effective options with icon and background overrides', () => {
    const defaultPreset = BUILTIN_PRESETS.find((p) => p.id === 'default')!
    const overrides = {
      sidebarIcons: 'monochrome' as const,
      background: { dark: { type: 'gradient' as const, from: '#000000', to: '#111111', angle: 45 } },
    }
    const effective = computeEffectiveOptions(defaultPreset, overrides, 'dark')
    assert.equal(effective.sidebarIcons, 'monochrome')
    assert.deepEqual(effective.background, overrides.background.dark)
    // Light mode gets an adapted (light) copy, never the dark gradient itself
    const lightOptions = computeEffectiveOptions(defaultPreset, overrides, 'light')
    assert.equal(lightOptions.background.type, 'gradient')
    assert.notDeepEqual(lightOptions.background, overrides.background.dark)
  })
})
