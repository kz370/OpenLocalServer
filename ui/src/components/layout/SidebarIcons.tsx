import { cn } from '@/lib/utils'
import { useThemeSafe } from '@/lib/theme'

export function useIsMonochrome(explicit?: boolean): boolean {
  const theme = useThemeSafe()
  if (explicit !== undefined) return explicit
  return theme?.options.sidebarIcons === 'monochrome'
}

export function DashboardIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <rect width="7" height="7" x="3" y="3" rx="1.5" />
        <rect width="7" height="7" x="14" y="3" rx="1.5" />
        <rect width="7" height="7" x="14" y="14" rx="1.5" />
        <rect width="7" height="7" x="3" y="14" rx="1.5" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-dash-tl" x1="3" y1="3" x2="10.5" y2="10.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#93c5fd" />
          <stop offset="0.5" stopColor="#6366f1" />
          <stop offset="1" stopColor="#4f46e5" />
        </linearGradient>
        <linearGradient id="sb-dash-tr" x1="13.5" y1="3" x2="21" y2="10.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#c084fc" />
          <stop offset="1" stopColor="#7c3aed" />
        </linearGradient>
        <linearGradient id="sb-dash-bl" x1="3" y1="13.5" x2="10.5" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#60a5fa" />
          <stop offset="1" stopColor="#3b82f6" />
        </linearGradient>
        <linearGradient id="sb-dash-br" x1="13.5" y1="13.5" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#a855f7" />
          <stop offset="1" stopColor="#6d28d9" />
        </linearGradient>
      </defs>
      <rect x="3" y="3" width="7.5" height="7.5" rx="2.2" fill="url(#sb-dash-tl)" />
      <rect x="13.5" y="3" width="7.5" height="7.5" rx="2.2" fill="url(#sb-dash-tr)" />
      <rect x="3" y="13.5" width="7.5" height="7.5" rx="2.2" fill="url(#sb-dash-bl)" />
      <rect x="13.5" y="13.5" width="7.5" height="7.5" rx="2.2" fill="url(#sb-dash-br)" />
      {/* Specular gloss highlights */}
      <rect x="3.8" y="3.8" width="5.9" height="2" rx="1" fill="#ffffff" fillOpacity="0.32" />
      <rect x="14.3" y="3.8" width="5.9" height="2" rx="1" fill="#ffffff" fillOpacity="0.25" />
    </svg>
  )
}

export function ProjectsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" />
        <path d="m10 13 2 2 4-4" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <radialGradient id="sb-proj-bg" cx="12" cy="12" r="9" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#3b82f6" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#1d4ed8" stopOpacity="0.06" />
        </radialGradient>
        <linearGradient id="sb-proj-stroke" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#60a5fa" />
          <stop offset="50%" stopColor="#3b82f6" />
          <stop offset="100%" stopColor="#2563eb" />
        </linearGradient>
      </defs>
      <path
        d="M3 6.5C3 5.67 3.67 5 4.5 5H9.2C9.7 5 10.2 5.25 10.5 5.65L11.8 7.35C12.1 7.75 12.6 8 13.1 8H19.5C20.33 8 21 8.67 21 9.5V17.5C21 18.33 20.33 19 19.5 19H4.5C3.67 19 3 18.33 3 17.5V6.5Z"
        fill="url(#sb-proj-bg)"
        stroke="url(#sb-proj-stroke)"
        strokeWidth="1.8"
      />
      <path d="m9.5 13.5 2 2 4-4" stroke="url(#sb-proj-stroke)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

export function SitesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <circle cx="12" cy="12" r="10" />
        <path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" />
        <path d="M2 12h20" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <radialGradient id="sb-globe-bg" cx="12" cy="12" r="9" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#0284c7" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#0369a1" stopOpacity="0.06" />
        </radialGradient>
        <linearGradient id="sb-globe-stroke" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#0284c7" />
          <stop offset="50%" stopColor="#0ea5e9" />
          <stop offset="100%" stopColor="#0369a1" />
        </linearGradient>
      </defs>
      <circle cx="12" cy="12" r="9" fill="url(#sb-globe-bg)" stroke="url(#sb-globe-stroke)" strokeWidth="1.8" />
      <ellipse cx="12" cy="12" rx="4.5" ry="9" stroke="url(#sb-globe-stroke)" strokeWidth="1.5" />
      <line x1="3" y1="12" x2="21" y2="12" stroke="url(#sb-globe-stroke)" strokeWidth="1.5" />
      <path d="M4.8 7.5C7 9 10 9.8 12 9.8s5-.8 7.2-2.3" stroke="url(#sb-globe-stroke)" strokeWidth="1.4" />
      <path d="M4.8 16.5C7 15 10 14.2 12 14.2s5 .8 7.2 2.3" stroke="url(#sb-globe-stroke)" strokeWidth="1.4" />
    </svg>
  )
}

export function QuickAppsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z" />
        <path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z" />
        <path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0" />
        <path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-rocket-body" x1="9" y1="15" x2="19" y2="5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#f8fafc" />
          <stop offset="0.6" stopColor="#ffffff" />
          <stop offset="1" stopColor="#e2e8f0" />
        </linearGradient>
        <linearGradient id="sb-rocket-flame" x1="3" y1="21" x2="9" y2="15" gradientUnits="userSpaceOnUse">
          <stop stopColor="#dc2626" />
          <stop offset="0.5" stopColor="#f97316" />
          <stop offset="1" stopColor="#fde047" />
        </linearGradient>
      </defs>
      {/* Exhaust Flame */}
      <path
        d="M8.5 15.5L3.8 20.2C5.5 19.8 6.5 18.2 6.5 18.2L5.2 21.2C7.2 20.2 8.5 18.6 9.5 16.5Z"
        fill="url(#sb-rocket-flame)"
      />
      <path
        d="M7.8 16.5L5.8 19.2C6.8 18.6 7.4 17.9 8 17Z"
        fill="#fde047"
      />
      {/* Stars / Sparks */}
      <path d="M2.5 15.2L3 13.8L3.5 15.2L4.9 15.7L3.5 16.2L3 17.6L2.5 16.2L1.1 15.7Z" fill="#d97706" />
      <path d="M7.2 21.5L7.6 20.4L8.7 20.9L7.9 21.7L8.2 22.9L7.4 22.2L6.5 22.9L6.8 21.8L6 21.1Z" fill="#b45309" />
      {/* Left Fin */}
      <path d="M9 15L5.8 17.6C5.6 15 6.8 13 7.8 12.4L9 15Z" fill="#dc2626" />
      {/* Right Fin */}
      <path d="M15 9L17.6 5.8C15 5.6 13 6.8 12.4 7.8L15 9Z" fill="#dc2626" />
      {/* Rocket Fuselage with crisp outline for Light Mode */}
      <path
        d="M9 15C8 12.5 8.5 9.5 12 6C15.5 2.5 19 2 21 3C22 5 21.5 8.5 18 12C14.5 15.5 11.5 16 9 15Z"
        fill="url(#sb-rocket-body)"
        stroke="#94a3b8"
        strokeWidth="0.8"
      />
      {/* Red Nosecone */}
      <path
        d="M21 3C21.8 4.2 21.2 6.5 19.5 8.5L15.5 4.5C17.5 2.8 19.8 2.2 21 3Z"
        fill="#ef4444"
      />
      {/* Circular Glass Window */}
      <circle cx="14.5" cy="9.5" r="2.2" fill="#0284c7" stroke="#ffffff" strokeWidth="0.9" />
      <circle cx="15.2" cy="8.8" r="0.65" fill="#ffffff" />
    </svg>
  )
}

export function CommandsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <polyline points="4 17 10 11 4 5" />
        <line x1="12" x2="20" y1="19" y2="19" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-cmd-bg" x1="2.5" y1="3.5" x2="21.5" y2="20.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#8b5cf6" />
          <stop offset="1" stopColor="#6366f1" />
        </linearGradient>
      </defs>
      <rect x="2.5" y="3.5" width="19" height="17" rx="4.5" fill="url(#sb-cmd-bg)" />
      <rect x="3.2" y="4.2" width="17.6" height="1" rx="0.5" fill="#ffffff" fillOpacity="0.25" />
      {/* >_ terminal prompt */}
      <path
        d="M7 8.5L10.5 12L7 15.5"
        stroke="#ffffff"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <line
        x1="12.5"
        y1="15.5"
        x2="17"
        y2="15.5"
        stroke="#ffffff"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function TunnelsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9" />
        <path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5" />
        <circle cx="12" cy="12" r="2" />
        <path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5" />
        <path d="M19.1 4.9C23 8.8 23 15.1 19.1 19" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        {/* Outer Ring Gradient (High contrast violet-indigo) */}
        <linearGradient id="sb-tun-ring1" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#4f46e5" />
          <stop offset="100%" stopColor="#7c3aed" />
        </linearGradient>
        {/* Middle Ring Gradient (Electric blue) */}
        <linearGradient id="sb-tun-ring2" x1="4" y1="4" x2="20" y2="20" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#0284c7" />
          <stop offset="100%" stopColor="#2563eb" />
        </linearGradient>
        {/* Core Cyber Beam Gradient */}
        <linearGradient id="sb-tun-beam" x1="5" y1="19" x2="19" y2="5" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="#0284c7" />
          <stop offset="45%" stopColor="#00f5d4" />
          <stop offset="100%" stopColor="#ffffff" />
        </linearGradient>
      </defs>

      {/* Back Portal Ring (Receding in distance) */}
      <ellipse cx="14" cy="10" rx="4.5" ry="6" transform="rotate(-25 14 10)" stroke="#6366f1" strokeWidth="1.5" strokeOpacity="0.6" />

      {/* Perspective Warp Guides */}
      <line x1="6.5" y1="9.5" x2="11.5" y2="6" stroke="#0284c7" strokeWidth="1.2" strokeDasharray="1.5 1.5" strokeOpacity="0.5" />
      <line x1="14" y1="20" x2="17.5" y2="15" stroke="#0284c7" strokeWidth="1.2" strokeDasharray="1.5 1.5" strokeOpacity="0.5" />

      {/* Middle Portal Ring */}
      <ellipse cx="12" cy="12" rx="6.5" ry="8" transform="rotate(-25 12 12)" stroke="url(#sb-tun-ring2)" strokeWidth="1.8" />

      {/* Front Portal Ring (High contrast foreground ring) */}
      <ellipse cx="10" cy="14" rx="8" ry="9.5" transform="rotate(-25 10 14)" stroke="url(#sb-tun-ring1)" strokeWidth="2.2" />

      {/* Piercing Cyber Beam cutting through tunnel center */}
      <line x1="5" y1="18.5" x2="18.5" y2="5.5" stroke="url(#sb-tun-beam)" strokeWidth="2.4" strokeLinecap="round" />

      {/* Center Luminous Warp Node / Flare */}
      <circle cx="12" cy="12" r="2.2" fill="#00f5d4" />
      <circle cx="12" cy="12" r="1.1" fill="#ffffff" />
    </svg>
  )
}

export function WebServerIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <rect width="20" height="8" x="2" y="2" rx="2" />
        <rect width="20" height="8" x="2" y="14" rx="2" />
        <line x1="6" x2="6.01" y1="6" y2="6" />
        <line x1="6" x2="6.01" y1="18" y2="18" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-srv-grad" x1="2.5" y1="4" x2="21.5" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#00d2ff" />
          <stop offset="1" stopColor="#0284c7" />
        </linearGradient>
      </defs>
      {/* Top unit */}
      <rect x="2.5" y="4" width="19" height="6.5" rx="2" fill="url(#sb-srv-grad)" />
      <line x1="5.5" y1="7.25" x2="13" y2="7.25" stroke="#ffffff" strokeWidth="1.6" strokeLinecap="round" strokeOpacity="0.85" />
      <circle cx="16" cy="7.25" r="1" fill="#22c55e" />
      <circle cx="18.5" cy="7.25" r="1" fill="#ffffff" />

      {/* Bottom unit */}
      <rect x="2.5" y="13.5" width="19" height="6.5" rx="2" fill="url(#sb-srv-grad)" />
      <line x1="5.5" y1="16.75" x2="13" y2="16.75" stroke="#ffffff" strokeWidth="1.6" strokeLinecap="round" strokeOpacity="0.85" />
      <circle cx="16" cy="16.75" r="1" fill="#22c55e" />
      <circle cx="18.5" cy="16.75" r="1" fill="#67e8f9" />
    </svg>
  )
}

export function DatabasesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <ellipse cx="12" cy="5" rx="9" ry="3" />
        <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
        <path d="M3 12c0 1.66 4 3 9 3s9-1.34 9-3" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-db-cyan" x1="3" y1="2" x2="21" y2="9" gradientUnits="userSpaceOnUse">
          <stop stopColor="#38bdf8" />
          <stop offset="1" stopColor="#00f0ff" />
        </linearGradient>
        <linearGradient id="sb-db-mid" x1="3" y1="9" x2="21" y2="15" gradientUnits="userSpaceOnUse">
          <stop stopColor="#06b6d4" />
          <stop offset="1" stopColor="#d946ef" />
        </linearGradient>
        <linearGradient id="sb-db-pink" x1="3" y1="14" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#ec4899" />
          <stop offset="1" stopColor="#f43f5e" />
        </linearGradient>
      </defs>
      {/* Bottom tier (pink) */}
      <path d="M3 15C3 17.5 7 19.5 12 19.5C17 19.5 21 17.5 21 15V17C21 19.5 17 21.5 12 21.5C7 21.5 3 19.5 3 17V15Z" fill="url(#sb-db-pink)" />
      {/* Middle tier (cyan-to-magenta) */}
      <path d="M3 10C3 12.5 7 14.5 12 14.5C17 14.5 21 12.5 21 10V12.5C21 15 17 17 12 17C7 17 3 15 3 12.5V10Z" fill="url(#sb-db-mid)" />
      {/* Top cylinder (cyan) */}
      <path d="M3 5.5V8.5C3 11 7 12.5 12 12.5C17 12.5 21 11 21 8.5V5.5C21 3 17 1.5 12 1.5C7 1.5 3 3 3 5.5Z" fill="url(#sb-db-cyan)" />
      {/* Top cap ellipse */}
      <ellipse cx="12" cy="5.5" rx="9" ry="3.5" fill="#67e8f9" />
      <ellipse cx="12" cy="5.5" rx="7.5" ry="2.5" fill="#00f0ff" />
    </svg>
  )
}

export function ServicesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <polygon points="12 2 2 7 12 12 22 7 12 2" />
        <polyline points="2 17 12 22 22 17" />
        <polyline points="2 12 12 17 22 12" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-srv-top" x1="12" y1="3" x2="12" y2="11" gradientUnits="userSpaceOnUse">
          <stop stopColor="#f472b6" />
          <stop offset="1" stopColor="#ec4899" />
        </linearGradient>
        <linearGradient id="sb-srv-mid" x1="12" y1="9" x2="12" y2="15" gradientUnits="userSpaceOnUse">
          <stop stopColor="#ec4899" />
          <stop offset="1" stopColor="#d946ef" />
        </linearGradient>
        <linearGradient id="sb-srv-bot" x1="12" y1="14" x2="12" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#d946ef" />
          <stop offset="1" stopColor="#a855f7" />
        </linearGradient>
      </defs>
      {/* Top layer */}
      <path d="M12 3L20 7L12 11L4 7L12 3Z" fill="url(#sb-srv-top)" />
      {/* Middle layer */}
      <path d="M4 11.5L12 15.5L20 11.5L20 13L12 17L4 13L4 11.5Z" fill="url(#sb-srv-mid)" />
      {/* Bottom layer */}
      <path d="M4 16L12 20L20 16L20 17.5L12 21.5L4 17.5L4 16Z" fill="url(#sb-srv-bot)" />
    </svg>
  )
}

export function RuntimesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z" />
        <path d="m3.3 7 8.7 5 8.7-5" />
        <path d="M12 22V12" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-box-top" x1="12" y1="2.5" x2="12" y2="11.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#fde047" />
          <stop offset="1" stopColor="#fbbf24" />
        </linearGradient>
        <linearGradient id="sb-box-left" x1="3.5" y1="7" x2="12" y2="21.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#f59e0b" />
          <stop offset="1" stopColor="#d97706" />
        </linearGradient>
        <linearGradient id="sb-box-right" x1="12" y1="11.5" x2="20.5" y2="21.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#d97706" />
          <stop offset="1" stopColor="#b45309" />
        </linearGradient>
      </defs>
      {/* Box Top face */}
      <path d="M12 2.5L20.5 7L12 11.5L3.5 7L12 2.5Z" fill="url(#sb-box-top)" />
      {/* Box Left face */}
      <path d="M3.5 7L12 11.5V21.5L3.5 17V7Z" fill="url(#sb-box-left)" />
      {/* Box Right face */}
      <path d="M12 11.5L20.5 7V17L12 21.5V11.5Z" fill="url(#sb-box-right)" />

      {/* Parcel Tape */}
      <path d="M10 3.6L14 5.7L14 10.4L10 8.3Z" fill="#78350f" fillOpacity="0.45" />
      <path d="M5.5 8L9.5 5.8L18.5 10.7L14.5 12.8Z" fill="#78350f" fillOpacity="0.3" />
      <path d="M10 8.3L12 9.4V19.4L10 18.3Z" fill="#78350f" fillOpacity="0.4" />
      <path d="M12 9.4L14 8.3V18.3L12 19.4Z" fill="#78350f" fillOpacity="0.5" />
    </svg>
  )
}

export function ProfilesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
        <circle cx="9" cy="7" r="4" />
        <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
        <path d="M16 3.13a4 4 0 0 1 0 7.75" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-prof-top" x1="12" y1="3" x2="12" y2="11" gradientUnits="userSpaceOnUse">
          <stop stopColor="#67e8f9" />
          <stop offset="1" stopColor="#06b6d4" />
        </linearGradient>
        <linearGradient id="sb-prof-mid" x1="12" y1="9" x2="12" y2="16" gradientUnits="userSpaceOnUse">
          <stop stopColor="#06b6d4" />
          <stop offset="1" stopColor="#10b981" />
        </linearGradient>
        <linearGradient id="sb-prof-bot" x1="12" y1="14" x2="12" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#10b981" />
          <stop offset="1" stopColor="#059669" />
        </linearGradient>
      </defs>
      {/* Top layer */}
      <path d="M12 3L20 7L12 11L4 7L12 3Z" fill="url(#sb-prof-top)" />
      {/* Middle layer */}
      <path d="M4 11.5L12 15.5L20 11.5L20 13L12 17L4 13L4 11.5Z" fill="url(#sb-prof-mid)" />
      {/* Bottom layer */}
      <path d="M4 16L12 20L20 16L20 17.5L12 21.5L4 17.5L4 16Z" fill="url(#sb-prof-bot)" />
    </svg>
  )
}

export function PluginsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="m19 11-4-4" />
        <path d="m17 9-2.5-2.5a2.12 2.12 0 0 0-3 0L9.5 8.5 4 14l6 6 5.5-5.5 2-2a2.12 2.12 0 0 0 0-3L17 9z" />
        <path d="m5 13 4 4" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-plug-body" x1="6" y1="7" x2="18" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#a855f7" />
          <stop offset="1" stopColor="#7c3aed" />
        </linearGradient>
      </defs>
      {/* Prongs */}
      <rect x="8" y="2.5" width="2.2" height="5" rx="0.8" fill="#e2e8f0" stroke="#94a3b8" strokeWidth="0.5" />
      <rect x="13.8" y="2.5" width="2.2" height="5" rx="0.8" fill="#e2e8f0" stroke="#94a3b8" strokeWidth="0.5" />
      {/* Plug body */}
      <path
        d="M6 7H18C19 7 20 8 20 9.5V13.5C20 16.5 17 19 13.5 19.5V22H10.5V19.5C7 19 4 16.5 4 13.5V9.5C4 8 5 7 6 7Z"
        fill="url(#sb-plug-body)"
      />
      {/* Lightning bolt */}
      <path
        d="M12.8 9.5L9.5 13.5H12L11.2 16.5L14.5 12.5H12L12.8 9.5Z"
        fill="#ffffff"
      />
    </svg>
  )
}

export function SettingsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-gear-grad" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#38bdf8" />
          <stop offset="1" stopColor="#0284c7" />
        </linearGradient>
      </defs>
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M10.3 2.5C10.7 2.2 11.3 2 12 2s1.3.2 1.7.5l.8 1.8c.6.2 1.2.6 1.7 1l1.9-.5c.6 0 1.2.3 1.6.8l1.4 2.4c.3.5.3 1.1.1 1.7l-1.1 1.6c.2.6.3 1.3.3 1.9s-.1 1.3-.3 1.9l1.1 1.6c.2.5.2 1.2-.1 1.7l-1.4 2.4c-.4.5-1 .8-1.6.8l-1.9-.5c-.5.4-1.1.8-1.7 1l-.8 1.8c-.4.3-1 .5-1.7.5s-1.3-.2-1.7-.5l-.8-1.8c-.6-.2-1.2-.6-1.7-1l-1.9.5c-.6 0-1.2-.3-1.6-.8l-1.4-2.4c-.3-.5-.3-1.1-.1-1.7l1.1-1.6c-.2-.6-.3-1.3-.3-1.9s.1-1.3.3-1.9l-1.1-1.6c-.2-.5-.2-1.2.1-1.7l1.4-2.4c.4-.5 1-.8 1.6-.8l1.9.5c.5-.4 1.1-.8 1.7-1l.8-1.8zM12 8a4 4 0 100 8 4 4 0 000-8z"
        fill="url(#sb-gear-grad)"
      />
      <circle cx="12" cy="12" r="2.3" fill="var(--color-sidebar, #0b0f19)" />
    </svg>
  )
}

export function MoonThemeIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-moon-gold" x1="4" y1="4" x2="20" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#fde047" />
          <stop offset="0.6" stopColor="#fbbf24" />
          <stop offset="1" stopColor="#f59e0b" />
        </linearGradient>
      </defs>
      <path
        d="M20.5 13.5C19.5 17.5 15.5 20.5 11 20.5C5.8 20.5 1.5 16.2 1.5 11C1.5 6.5 4.5 2.5 8.5 1.5C7.5 3.5 7.5 6.5 8.5 8.5C10 11.5 13 13 16 13C17.8 13 19.5 12.5 20.5 13.5Z"
        fill="url(#sb-moon-gold)"
      />
    </svg>
  )
}

export function SunThemeIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-sun-gold" x1="4" y1="4" x2="20" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#fde047" />
          <stop offset="0.7" stopColor="#fbbf24" />
          <stop offset="1" stopColor="#f59e0b" />
        </linearGradient>
      </defs>
      <circle cx="12" cy="12" r="5" fill="url(#sb-sun-gold)" />
      <path
        d="M12 2V4M12 20V22M4 12H2M22 12H20M4.9 4.9L6.3 6.3M17.7 17.7L19.1 19.1M4.9 19.1L6.3 17.7M17.7 6.3L19.1 4.9"
        stroke="#f59e0b"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function SystemThemeIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-sys-gold" x1="2" y1="2" x2="12" y2="22" gradientUnits="userSpaceOnUse">
          <stop stopColor="#fde047" />
          <stop offset="1" stopColor="#f59e0b" />
        </linearGradient>
        <linearGradient id="sb-sys-indigo" x1="12" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
          <stop stopColor="#818cf8" />
          <stop offset="1" stopColor="#4f46e5" />
        </linearGradient>
      </defs>
      <path d="M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22V2Z" fill="url(#sb-sys-gold)" />
      <path d="M12 2C17.52 2 22 6.48 22 12C22 17.52 17.52 22 12 22V2Z" fill="url(#sb-sys-indigo)" />
      <circle cx="12" cy="12" r="4" fill="#ffffff" fillOpacity="0.3" />
    </svg>
  )
}

export function LogsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <polyline points="14 2 14 8 20 8" />
        <line x1="16" x2="8" y1="13" y2="13" />
        <line x1="16" x2="8" y1="17" y2="17" />
        <polyline points="10 9 9 9 8 9" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-log-grad" x1="4" y1="2" x2="20" y2="22" gradientUnits="userSpaceOnUse">
          <stop stopColor="#38bdf8" />
          <stop offset="1" stopColor="#0284c7" />
        </linearGradient>
      </defs>
      <path d="M5 3C5 2.45 5.45 2 6 2H15L19 6V21C19 21.55 18.55 22 18 22H6C5.45 22 5 21.55 5 21V3Z" fill="url(#sb-log-grad)" />
      <path d="M15 2V6H19" fill="#e0f2fe" fillOpacity="0.4" />
      <line x1="8" y1="10" x2="16" y2="10" stroke="#ffffff" strokeWidth="1.6" strokeLinecap="round" />
      <line x1="8" y1="14" x2="16" y2="14" stroke="#ffffff" strokeWidth="1.6" strokeLinecap="round" strokeOpacity="0.85" />
      <line x1="8" y1="18" x2="13" y2="18" stroke="#ffffff" strokeWidth="1.6" strokeLinecap="round" strokeOpacity="0.85" />
    </svg>
  )
}

export function ProcessesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M22 12h-4l-3 9L9 3l-3 9H2" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-proc-bg" x1="2.5" y1="3.5" x2="21.5" y2="20.5" gradientUnits="userSpaceOnUse">
          <stop stopColor="#10b981" />
          <stop offset="1" stopColor="#047857" />
        </linearGradient>
      </defs>
      <rect x="2.5" y="3.5" width="19" height="17" rx="4" fill="url(#sb-proc-bg)" />
      <path d="M6 12L9 9L12 12L15 8L18 13" stroke="#ffffff" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="15" cy="8" r="1.2" fill="#fde047" />
    </svg>
  )
}

export function TasksIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <circle cx="12" cy="12" r="10" />
        <polyline points="12 6 12 12 16 14" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-task-bg" x1="4" y1="3" x2="20" y2="22" gradientUnits="userSpaceOnUse">
          <stop stopColor="#6366f1" />
          <stop offset="1" stopColor="#4338ca" />
        </linearGradient>
      </defs>
      <rect x="4" y="4" width="16" height="18" rx="3" fill="url(#sb-task-bg)" />
      <rect x="8" y="2" width="8" height="3.5" rx="1.5" fill="#a5b4fc" />
      <path d="M7.5 10L9.5 12L13 8.5" stroke="#4ade80" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      <line x1="14.5" y1="10" x2="16.5" y2="10" stroke="#ffffff" strokeWidth="1.5" strokeLinecap="round" />
      <path d="M7.5 16L9.5 18L13 14.5" stroke="#4ade80" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
      <line x1="14.5" y1="16" x2="16.5" y2="16" stroke="#ffffff" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  )
}

export function SearchNavIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-4 shrink-0', className)}>
        <circle cx="11" cy="11" r="8" />
        <line x1="21" x2="16.65" y1="21" y2="16.65" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-4 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-search-grad" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#818cf8" />
          <stop offset="1" stopColor="#06b6d4" />
        </linearGradient>
      </defs>
      <circle cx="11" cy="11" r="7" stroke="url(#sb-search-grad)" strokeWidth="2.2" />
      <path d="M16.5 16.5L21 21" stroke="url(#sb-search-grad)" strokeWidth="2.2" strokeLinecap="round" />
    </svg>
  )
}

/* ──────────────────────────────────────────────────────────────────────────
   Settings Page Sidebar Colorful Icons (matching Settings reference)
   ────────────────────────────────────────────────────────────────────────── */

export function SettingsGeneralIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <line x1="4" x2="20" y1="21" y2="21" />
        <line x1="4" x2="20" y1="14" y2="14" />
        <line x1="4" x2="20" y1="7" y2="7" />
        <circle cx="8" cy="7" r="2" fill="currentColor" />
        <circle cx="16" cy="14" r="2" fill="currentColor" />
        <circle cx="12" cy="21" r="2" fill="currentColor" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-gen" x1="3" y1="6" x2="21" y2="18" gradientUnits="userSpaceOnUse">
          <stop stopColor="#0d9488" />
          <stop offset="1" stopColor="#0284c7" />
        </linearGradient>
      </defs>
      {/* Top track */}
      <line x1="4" y1="8" x2="13" y2="8" stroke="url(#sb-set-gen)" strokeWidth="2.2" strokeLinecap="round" />
      <line x1="17.5" y1="8" x2="20" y2="8" stroke="url(#sb-set-gen)" strokeWidth="2.2" strokeLinecap="round" />
      <circle cx="15" cy="8" r="2.6" fill="url(#sb-set-gen)" stroke="var(--color-sidebar, #050b16)" strokeWidth="1.2" />
      {/* Bottom track */}
      <line x1="4" y1="16" x2="7" y2="16" stroke="url(#sb-set-gen)" strokeWidth="2.2" strokeLinecap="round" />
      <line x1="11.5" y1="16" x2="20" y2="16" stroke="url(#sb-set-gen)" strokeWidth="2.2" strokeLinecap="round" />
      <circle cx="9" cy="16" r="2.6" fill="url(#sb-set-gen)" stroke="var(--color-sidebar, #050b16)" strokeWidth="1.2" />
    </svg>
  )
}

export function SettingsMcpIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-mcp" x1="4" y1="3" x2="20" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#6366f1" />
          <stop offset="1" stopColor="#4f46e5" />
        </linearGradient>
      </defs>
      {/* Shield outline */}
      <path
        d="M12 3L4 6.5V11.5C4 16.5 7.5 20.8 12 22C16.5 20.8 20 16.5 20 11.5V6.5L12 3Z"
        stroke="url(#sb-set-mcp)"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="url(#sb-set-mcp)"
        fillOpacity="0.18"
      />
      {/* Center 4-point star */}
      <path
        d="M12 7.5L13 11L16.5 12L13 13L12 16.5L11 13L7.5 12L11 11L12 7.5Z"
        fill="#ffffff"
      />
    </svg>
  )
}

export function SettingsSitesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  return <SitesIcon className={className} monochrome={monochrome} />
}

export function SettingsRootsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" />
        <line x1="12" y1="10" x2="12" y2="16" />
        <line x1="9" y1="13" x2="15" y2="13" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-folder" x1="3" y1="5" x2="21" y2="19" gradientUnits="userSpaceOnUse">
          <stop stopColor="#f59e0b" />
          <stop offset="0.5" stopColor="#d97706" />
          <stop offset="1" stopColor="#b45309" />
        </linearGradient>
      </defs>
      <path
        d="M3 6.5C3 5.67 3.67 5 4.5 5H9.2C9.7 5 10.2 5.25 10.5 5.65L11.8 7.35C12.1 7.75 12.6 8 13.1 8H19.5C20.33 8 21 8.67 21 9.5V17.5C21 18.33 20.33 19 19.5 19H4.5C3.67 19 3 18.33 3 17.5V6.5Z"
        stroke="url(#sb-set-folder)"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="url(#sb-set-folder)"
        fillOpacity="0.22"
      />
    </svg>
  )
}

export function SettingsDeletedIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M9.88 9.88a3 3 0 1 0 4.24 4.24" />
        <path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68" />
        <path d="M6.61 6.61A13.526 13.526 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61" />
        <line x1="2" x2="22" y1="2" y2="22" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-eye" x1="3" y1="4" x2="21" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#e11d48" />
          <stop offset="1" stopColor="#be123c" />
        </linearGradient>
      </defs>
      {/* Eye shape */}
      <path
        d="M2.5 12C4.5 7.5 8 5 12 5C14.2 5 16.3 5.8 18 7.2M21.5 12C19.5 16.5 16 19 12 19C9.8 19 7.7 18.2 6 16.8"
        stroke="url(#sb-set-eye)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
      {/* Pupil arc */}
      <path
        d="M10 10.5C9.6 11 9.4 11.5 9.4 12C9.4 13.4 10.6 14.6 12 14.6C12.5 14.6 13 14.4 13.5 14"
        stroke="url(#sb-set-eye)"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {/* Diagonal slash */}
      <line
        x1="3.5"
        y1="3.5"
        x2="20.5"
        y2="20.5"
        stroke="url(#sb-set-eye)"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function SettingsStartupIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
        <line x1="12" x2="12" y1="2" y2="12" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-power" x1="4" y1="3" x2="20" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#0284c7" />
          <stop offset="1" stopColor="#0369a1" />
        </linearGradient>
      </defs>
      {/* Circular arc */}
      <path
        d="M18.36 6.64A9 9 0 1 1 5.64 6.64"
        stroke="url(#sb-set-power)"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
      {/* Vertical power bar */}
      <line
        x1="12"
        y1="2.5"
        x2="12"
        y2="11.5"
        stroke="url(#sb-set-power)"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function SettingsDiagnosticsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M4.8 2.3A.3.3 0 1 0 5 2H4a2 2 0 0 0-2 2v5a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6V4a2 2 0 0 0-2-2h-1a.2.2 0 1 0 .3.3" />
        <path d="M8 15v1a6 6 0 0 0 6 6v0a6 6 0 0 0 6-6v-4" />
        <circle cx="20" cy="10" r="2" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-steth" x1="4" y1="3" x2="20" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#c084fc" />
          <stop offset="0.6" stopColor="#a855f7" />
          <stop offset="1" stopColor="#7c3aed" />
        </linearGradient>
      </defs>
      {/* Stethoscope earpieces & U-loop */}
      <path
        d="M6 3V8C6 11.3 8.7 14 12 14C15.3 14 18 11.3 18 8V3"
        stroke="url(#sb-set-steth)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
      {/* Earpieces tips */}
      <circle cx="6" cy="3" r="1.3" fill="url(#sb-set-steth)" />
      <circle cx="18" cy="3" r="1.3" fill="url(#sb-set-steth)" />
      {/* Down stem and chestpiece loop */}
      <path
        d="M12 14V17C12 18.7 13.3 20 15 20C16.7 20 18 18.7 18 17V16"
        stroke="url(#sb-set-steth)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
      {/* Chestpiece */}
      <circle cx="18" cy="15" r="2.4" fill="url(#sb-set-steth)" />
      <circle cx="18" cy="15" r="1" fill="#ffffff" />
    </svg>
  )
}

export function SettingsManualIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
        <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-book" x1="2" y1="5" x2="22" y2="19" gradientUnits="userSpaceOnUse">
          <stop stopColor="#059669" />
          <stop offset="1" stopColor="#047857" />
        </linearGradient>
      </defs>
      {/* Open book left & right pages */}
      <path
        d="M4 19.5C6.5 18 9 18 12 19.5C15 18 17.5 18 20 19.5V6.5C17.5 5 15 5 12 6.5C9 5 6.5 5 4 6.5V19.5Z"
        stroke="url(#sb-set-book)"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="url(#sb-set-book)"
        fillOpacity="0.2"
      />
      {/* Center spine */}
      <line x1="12" y1="6.5" x2="12" y2="19.5" stroke="url(#sb-set-book)" strokeWidth="2" strokeLinecap="round" />
    </svg>
  )
}

export function SettingsResourcesIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <path d="M12 14v4" />
        <path d="M3.34 19a10 10 0 1 1 17.32 0" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-gauge" x1="3" y1="4" x2="21" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#0284c7" />
          <stop offset="1" stopColor="#1d4ed8" />
        </linearGradient>
      </defs>
      {/* Dial arc */}
      <path
        d="M4.5 17.5A9 9 0 1 1 19.5 17.5"
        stroke="url(#sb-set-gauge)"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
      {/* Center hub */}
      <circle cx="12" cy="14" r="2.4" fill="url(#sb-set-gauge)" />
      {/* Needle pointing up-right */}
      <line
        x1="12"
        y1="14"
        x2="16.5"
        y2="9.5"
        stroke="#ffffff"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function SettingsBackupsIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <rect width="20" height="5" x="2" y="3" rx="1" />
        <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
        <path d="M10 12h4" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-archive" x1="3" y1="4" x2="21" y2="20" gradientUnits="userSpaceOnUse">
          <stop stopColor="#d97706" />
          <stop offset="1" stopColor="#b45309" />
        </linearGradient>
      </defs>
      {/* Top lid */}
      <rect x="3" y="4" width="18" height="4.5" rx="1.5" stroke="url(#sb-set-archive)" strokeWidth="2.2" fill="url(#sb-set-archive)" fillOpacity="0.25" />
      {/* Bottom crate */}
      <path
        d="M4.5 8.5V18.5C4.5 19.3 5.2 20 6 20H18C18.8 20 19.5 19.3 19.5 18.5V8.5"
        stroke="url(#sb-set-archive)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
      {/* Center handle latch */}
      <line x1="10" y1="12.5" x2="14" y2="12.5" stroke="#ffffff" strokeWidth="2.2" strokeLinecap="round" />
    </svg>
  )
}

export function SettingsAboutIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <circle cx="12" cy="12" r="10" />
        <line x1="12" x2="12" y1="16" y2="12" />
        <line x1="12" x2="12.01" y1="8" y2="8" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-about" x1="3" y1="3" x2="21" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#c084fc" />
          <stop offset="1" stopColor="#a855f7" />
        </linearGradient>
      </defs>
      {/* Outer circle */}
      <circle cx="12" cy="12" r="9" stroke="url(#sb-set-about)" strokeWidth="2" fill="url(#sb-set-about)" fillOpacity="0.15" />
      {/* Dot of i */}
      <circle cx="12" cy="8" r="1.2" fill="#ffffff" />
      {/* Stem of i */}
      <line x1="12" y1="11" x2="12" y2="16" stroke="#ffffff" strokeWidth="2" strokeLinecap="round" />
    </svg>
  )
}

export function SettingsAppearanceIcon({ className, monochrome }: { className?: string; monochrome?: boolean }) {
  if (useIsMonochrome(monochrome)) {
    return (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={cn('size-5 shrink-0', className)}>
        <circle cx="13.5" cy="6.5" r=".5" fill="currentColor" />
        <circle cx="17.5" cy="10.5" r=".5" fill="currentColor" />
        <circle cx="8.5" cy="7.5" r=".5" fill="currentColor" />
        <circle cx="6.5" cy="12.5" r=".5" fill="currentColor" />
        <path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.563-2.512 5.563-5.563C22 6.5 17.5 2 12 2Z" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 24 24" fill="none" className={cn('size-5 shrink-0', className)}>
      <defs>
        <linearGradient id="sb-set-palette" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
          <stop stopColor="#ec4899" />
          <stop offset="0.5" stopColor="#a855f7" />
          <stop offset="1" stopColor="#6366f1" />
        </linearGradient>
      </defs>
      <path
        d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.563-2.512 5.563-5.563C22 6.5 17.5 2 12 2Z"
        stroke="url(#sb-set-palette)"
        strokeWidth="2"
        fill="url(#sb-set-palette)"
        fillOpacity="0.15"
      />
      <circle cx="13.5" cy="6.5" r="1.5" fill="#f43f5e" />
      <circle cx="17.5" cy="10.5" r="1.5" fill="#a855f7" />
      <circle cx="8.5" cy="7.5" r="1.5" fill="#3b82f6" />
      <circle cx="6.5" cy="12.5" r="1.5" fill="#10b981" />
    </svg>
  )
}


