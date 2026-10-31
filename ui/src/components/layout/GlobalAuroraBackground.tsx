/**
 * GlobalAuroraBackground
 *
 * Implements the "Dark Aurora" and "Light Aurora" multi-layered mesh background
 * with flowing organic aurora waves, luminous ridge lines, and ambient blurs.
 *
 * - Dark Mode: Deep midnight navy (#050B16, #0B1428, #11132D), luminous cyan/teal top ribbon,
 *   and rich royal blue & purple/violet bottom S-curves.
 * - Light Mode: Soft ice-white (#F8FAFF, #EEF3FF, #E8EAFB), translucent mint/cyan top ribbon,
 *   and soft pastel blue & lilac/lavender bottom waves.
 * - Positioned fixed behind all UI content with pointer-events: none.
 */
import { useThemeSafe } from '@/lib/theme'

export function GlobalAuroraBackground() {
  const theme = useThemeSafe()
  const isGradient = theme?.options.background.type === 'gradient'
  const isAurora = !theme || theme.preset.id === 'default'

  if (isGradient) {
    return (
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 -z-10 overflow-hidden select-none transition-colors duration-700"
        style={{ background: 'var(--app-background)' }}
      />
    )
  }

  if (!isAurora) {
    return (
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 -z-10 overflow-hidden select-none transition-colors duration-700"
        style={{ background: 'var(--app-background, var(--background))' }}
      />
    )
  }

  return (
    <div
      aria-hidden="true"
      className="pointer-events-none fixed inset-0 -z-10 overflow-hidden select-none transition-colors duration-700"
      style={{ background: 'var(--app-background, var(--background))' }}
    >
      {/* ── Soft Multi-Layer Ambient Mesh Glows ────────────────────────────── */}
      {/* Top-Right Cyan/Teal Ambient Glow */}
      <div className="absolute -top-[12%] -right-[6%] h-[58vh] w-[58vw] rounded-full bg-gradient-to-br from-[#B5E9D9]/40 via-[#cffafe]/25 to-transparent blur-3xl transition-all duration-700 dark:from-[#087A72]/45 dark:via-[#00f5d4]/20 dark:to-transparent" />

      {/* Bottom-Center Purple/Lilac Ambient Glow */}
      <div className="absolute -bottom-[16%] left-[26%] h-[58vh] w-[62vw] rounded-full bg-gradient-to-t from-[#D8B9F5]/45 via-[#e9d5ff]/25 to-transparent blur-3xl transition-all duration-700 dark:from-[#482078]/55 dark:via-[#7c3aed]/25 dark:to-transparent" />

      {/* Bottom-Left Blue/Navy Ambient Glow */}
      <div className="absolute -bottom-[12%] -left-[6%] h-[52vh] w-[52vw] rounded-full bg-gradient-to-tr from-[#EEF3FF]/60 via-[#bae6fd]/30 to-transparent blur-3xl transition-all duration-700 dark:from-[#0B1428]/60 dark:via-[#0284c7]/25 dark:to-transparent" />

      {/* Subtle Center Ambient Depth */}
      <div className="absolute top-[35%] left-[20%] h-[40vh] w-[60vw] rounded-full bg-gradient-to-r from-transparent via-[#E8EAFB]/20 to-transparent blur-3xl transition-all duration-700 dark:via-[#11132D]/35" />

      {/* ── Flowing Aurora Waves (Vector SVG) ──────────────────────────────── */}
      <svg
        viewBox="0 0 1440 900"
        fill="none"
        preserveAspectRatio="none"
        className="absolute inset-0 size-full"
      >
        <defs>
          {/* ── Dark Mode Gradients ────────────────────────────────────────── */}
          {/* Dark Top Aurora Wave */}
          <linearGradient id="aurora-dark-top-grad" x1="850" y1="0" x2="1440" y2="120" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#087A72" stopOpacity="0.45" />
            <stop offset="45%" stopColor="#00dfa2" stopOpacity="0.32" />
            <stop offset="80%" stopColor="#00f0ff" stopOpacity="0.18" />
            <stop offset="100%" stopColor="#0B1428" stopOpacity="0" />
          </linearGradient>

          {/* Dark Top Ridge Highlight */}
          <linearGradient id="aurora-dark-top-ridge" x1="780" y1="0" x2="1440" y2="95" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#087A72" stopOpacity="0.2" />
            <stop offset="35%" stopColor="#00f5d4" stopOpacity="0.85" />
            <stop offset="75%" stopColor="#38bdf8" stopOpacity="0.8" />
            <stop offset="100%" stopColor="#00f0ff" stopOpacity="0.4" />
          </linearGradient>

          {/* Dark Top Secondary Veil */}
          <linearGradient id="aurora-dark-top-veil" x1="950" y1="0" x2="1440" y2="180" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#00f5d4" stopOpacity="0.18" />
            <stop offset="60%" stopColor="#087A72" stopOpacity="0.12" />
            <stop offset="100%" stopColor="#050B16" stopOpacity="0" />
          </linearGradient>

          {/* Dark Bottom Blue Wave */}
          <linearGradient id="aurora-dark-bot-blue" x1="0" y1="720" x2="1000" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#0B1428" stopOpacity="0.75" />
            <stop offset="30%" stopColor="#1e40af" stopOpacity="0.5" />
            <stop offset="65%" stopColor="#0284c7" stopOpacity="0.3" />
            <stop offset="100%" stopColor="#050B16" stopOpacity="0.05" />
          </linearGradient>

          {/* Dark Bottom Blue Ridge Highlight */}
          <linearGradient id="aurora-dark-blue-ridge" x1="0" y1="740" x2="1200" y2="870" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#0284c7" stopOpacity="0.6" />
            <stop offset="25%" stopColor="#38bdf8" stopOpacity="0.85" />
            <stop offset="60%" stopColor="#00f5d4" stopOpacity="0.7" />
            <stop offset="100%" stopColor="#1e3a8a" stopOpacity="0.2" />
          </linearGradient>

          {/* Dark Bottom Purple/Violet Crest Wave */}
          <linearGradient id="aurora-dark-bot-purple" x1="400" y1="710" x2="1440" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#482078" stopOpacity="0.85" />
            <stop offset="40%" stopColor="#7c3aed" stopOpacity="0.6" />
            <stop offset="75%" stopColor="#6366f1" stopOpacity="0.35" />
            <stop offset="100%" stopColor="#11132D" stopOpacity="0.1" />
          </linearGradient>

          {/* Dark Bottom Purple Ridge Highlight */}
          <linearGradient id="aurora-dark-purple-ridge" x1="300" y1="880" x2="1440" y2="730" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#818cf8" stopOpacity="0.3" />
            <stop offset="40%" stopColor="#c084fc" stopOpacity="0.9" />
            <stop offset="75%" stopColor="#a855f7" stopOpacity="0.85" />
            <stop offset="100%" stopColor="#e879f9" stopOpacity="0.4" />
          </linearGradient>

          {/* Dark Bottom Ambient Ground */}
          <linearGradient id="aurora-dark-ground" x1="720" y1="800" x2="720" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#050B16" stopOpacity="0" />
            <stop offset="60%" stopColor="#050B16" stopOpacity="0.55" />
            <stop offset="100%" stopColor="#050B16" stopOpacity="0.85" />
          </linearGradient>

          {/* ── Light Mode Gradients ───────────────────────────────────────── */}
          {/* Light Top Aurora Wave */}
          <linearGradient id="aurora-light-top-grad" x1="850" y1="0" x2="1440" y2="120" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#B5E9D9" stopOpacity="0.6" />
            <stop offset="45%" stopColor="#a5f3fc" stopOpacity="0.4" />
            <stop offset="80%" stopColor="#7dd3fc" stopOpacity="0.25" />
            <stop offset="100%" stopColor="#EEF3FF" stopOpacity="0" />
          </linearGradient>

          {/* Light Top Ridge Highlight */}
          <linearGradient id="aurora-light-top-ridge" x1="780" y1="0" x2="1440" y2="95" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#a7f3d0" stopOpacity="0.3" />
            <stop offset="35%" stopColor="#38bdf8" stopOpacity="0.8" />
            <stop offset="75%" stopColor="#bae6fd" stopOpacity="0.75" />
            <stop offset="100%" stopColor="#ffffff" stopOpacity="0.9" />
          </linearGradient>

          {/* Light Top Secondary Veil */}
          <linearGradient id="aurora-light-top-veil" x1="950" y1="0" x2="1440" y2="180" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#B5E9D9" stopOpacity="0.3" />
            <stop offset="60%" stopColor="#e0e7ff" stopOpacity="0.2" />
            <stop offset="100%" stopColor="#F8FAFF" stopOpacity="0" />
          </linearGradient>

          {/* Light Bottom Blue Wave */}
          <linearGradient id="aurora-light-bot-blue" x1="0" y1="720" x2="1000" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#EEF3FF" stopOpacity="0.8" />
            <stop offset="30%" stopColor="#c7d2fe" stopOpacity="0.55" />
            <stop offset="65%" stopColor="#bae6fd" stopOpacity="0.35" />
            <stop offset="100%" stopColor="#F8FAFF" stopOpacity="0.05" />
          </linearGradient>

          {/* Light Bottom Blue Ridge Highlight */}
          <linearGradient id="aurora-light-blue-ridge" x1="0" y1="740" x2="1200" y2="870" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#38bdf8" stopOpacity="0.5" />
            <stop offset="25%" stopColor="#60a5fa" stopOpacity="0.8" />
            <stop offset="60%" stopColor="#a5f3fc" stopOpacity="0.75" />
            <stop offset="100%" stopColor="#ffffff" stopOpacity="0.9" />
          </linearGradient>

          {/* Light Bottom Lilac/Purple Crest Wave */}
          <linearGradient id="aurora-light-bot-purple" x1="400" y1="710" x2="1440" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#D8B9F5" stopOpacity="0.75" />
            <stop offset="40%" stopColor="#e9d5ff" stopOpacity="0.55" />
            <stop offset="75%" stopColor="#c4b5fd" stopOpacity="0.35" />
            <stop offset="100%" stopColor="#E8EAFB" stopOpacity="0.1" />
          </linearGradient>

          {/* Light Bottom Lilac Ridge Highlight */}
          <linearGradient id="aurora-light-purple-ridge" x1="300" y1="880" x2="1440" y2="730" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#c4b5fd" stopOpacity="0.3" />
            <stop offset="40%" stopColor="#c084fc" stopOpacity="0.85" />
            <stop offset="75%" stopColor="#d8b4fe" stopOpacity="0.8" />
            <stop offset="100%" stopColor="#ffffff" stopOpacity="0.9" />
          </linearGradient>

          {/* Light Bottom Ambient Ground */}
          <linearGradient id="aurora-light-ground" x1="720" y1="800" x2="720" y2="900" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#F8FAFF" stopOpacity="0" />
            <stop offset="60%" stopColor="#F8FAFF" stopOpacity="0.55" />
            <stop offset="100%" stopColor="#F8FAFF" stopOpacity="0.85" />
          </linearGradient>

          {/* Soft Glow Filters */}
          <filter id="aurora-ridge-glow" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur stdDeviation="3" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* ── Top-Right Aurora Ribbon Waves ────────────────────────────────── */}
        {/* Secondary Veil Layer */}
        <path
          d="M 940 0 C 1040 75, 1190 120, 1440 180 L 1440 0 Z"
          className="fill-[url(#aurora-light-top-veil)] transition-all duration-700 dark:fill-[url(#aurora-dark-top-veil)]"
        />

        {/* Primary Top Aurora Wave */}
        <path
          d="M 780 0 C 890 60, 1060 140, 1220 80 C 1330 40, 1390 65, 1440 95 L 1440 0 Z"
          className="fill-[url(#aurora-light-top-grad)] transition-all duration-700 dark:fill-[url(#aurora-dark-top-grad)]"
        />

        {/* Top Glowing Ridge Line */}
        <path
          d="M 780 0 C 890 60, 1060 140, 1220 80 C 1330 40, 1390 65, 1440 95"
          filter="url(#aurora-ridge-glow)"
          strokeWidth="2.5"
          strokeLinecap="round"
          className="stroke-[url(#aurora-light-top-ridge)] transition-all duration-700 dark:stroke-[url(#aurora-dark-top-ridge)]"
        />

        {/* ── Bottom Flowing Aurora Waves ──────────────────────────────────── */}
        {/* Wave 1: Blue S-Curve Wave (Left to Center-Right) */}
        <path
          d="M -20 920 L -20 740 C 160 690, 360 720, 560 810 C 760 900, 980 890, 1200 870 L 1460 880 L 1460 920 Z"
          className="fill-[url(#aurora-light-bot-blue)] transition-all duration-700 dark:fill-[url(#aurora-dark-bot-blue)]"
        />
        {/* Blue Wave Ridge Line */}
        <path
          d="M -20 740 C 160 690, 360 720, 560 810 C 760 900, 980 890, 1200 870 L 1460 880"
          filter="url(#aurora-ridge-glow)"
          strokeWidth="2.2"
          strokeLinecap="round"
          className="stroke-[url(#aurora-light-blue-ridge)] transition-all duration-700 dark:stroke-[url(#aurora-dark-blue-ridge)]"
        />

        {/* Wave 2: Purple / Violet Crest Wave (Center to Right) */}
        <path
          d="M 220 920 C 420 860, 560 710, 780 710 C 980 710, 1180 810, 1460 730 L 1460 920 Z"
          className="fill-[url(#aurora-light-bot-purple)] transition-all duration-700 dark:fill-[url(#aurora-dark-bot-purple)]"
        />
        {/* Purple Wave Ridge Line */}
        <path
          d="M 220 920 C 420 860, 560 710, 780 710 C 980 710, 1180 810, 1460 730"
          filter="url(#aurora-ridge-glow)"
          strokeWidth="2.6"
          strokeLinecap="round"
          className="stroke-[url(#aurora-light-purple-ridge)] transition-all duration-700 dark:stroke-[url(#aurora-dark-purple-ridge)]"
        />

        {/* Wave 3: Foreground Grounding Ambient Fill */}
        <path
          d="M -20 830 C 380 800, 840 850, 1460 790 L 1460 920 L -20 920 Z"
          className="fill-[url(#aurora-light-ground)] transition-all duration-700 dark:fill-[url(#aurora-dark-ground)]"
        />
      </svg>
    </div>
  )
}
