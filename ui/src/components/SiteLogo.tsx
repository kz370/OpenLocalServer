import type React from 'react'
import { cn } from '@/lib/utils'

/**
 * A clean, modern internet globe icon matching the reference screenshot.
 * Uses crisp stroke lines for the classic wireframe globe internet icon.
 */
export function DefaultInternetIcon({ className = 'size-5' }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn('shrink-0 select-none text-[#00b4d8] dark:text-[#06b6d4]', className)}
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Default internet globe icon"
      role="img"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M3.6 9h16.8" />
      <path d="M3.6 15h16.8" />
      <path d="M11.5 3a17 17 0 0 0 0 18" />
      <path d="M12.5 3a17 17 0 0 1 0 18" />
    </svg>
  )
}

import faIconsData from './faIconsData.json'

/** Offline Font Awesome Free icon registry with SVG path data (viewBox 0 0 512 512). */
export interface FaIconDef {
  id: string
  name: string
  category: 'brands' | 'web' | 'tech' | 'commerce' | 'media' | 'general'
  svgPath: string
  viewBox?: string
  color?: string
}

export const FA_ICONS: FaIconDef[] = faIconsData as FaIconDef[]


/**
 * Normalizes an icon string (e.g. "fa-wordpress", "fa-brands fa-wordpress", "wordpress", "fa-globe")
 * to match against the offline catalog.
 */
export function findFaIcon(nameOrClass?: string | null): FaIconDef | undefined {
  if (!nameOrClass) return undefined
  const cleaned = nameOrClass
    .toLowerCase()
    .replace(/^fa-(solid|brands|regular|light|duotone|thin)\s+/, '')
    .replace(/^fa-/, '')
    .trim()
  return FA_ICONS.find((i) => i.id === cleaned || i.name.toLowerCase() === cleaned)
}

/**
 * Parses an icon spec that may optionally encode a custom color:
 * e.g. "rocket", "rocket|crimson", "server|#00b4d8", or "code|rgba(59, 130, 246, 0.85)".
 */
export function parseIconSpec(spec?: string | null): { name: string; color?: string } {
  if (!spec) return { name: '' }
  const idx = spec.indexOf('|')
  if (idx === -1) {
    return { name: spec.trim() }
  }
  return {
    name: spec.substring(0, idx).trim(),
    color: spec.substring(idx + 1).trim() || undefined,
  }
}

/** Formats an icon identifier and optional custom color into a persistable spec string. */
export function buildIconSpec(name: string, color?: string | null): string {
  const cleanColor = color?.trim()
  return cleanColor ? `${name.trim()}|${cleanColor}` : name.trim()
}

/** Renders an offline Font Awesome icon with crisp SVG path and custom color support (name, hex, rgba). */
export function FontAwesomeIcon({
  name,
  color,
  className = 'size-4',
  style,
}: {
  name: string
  color?: string
  className?: string
  style?: React.CSSProperties
}) {
  const parsed = parseIconSpec(name)
  const icon = findFaIcon(parsed.name)
  if (!icon) {
    // If not found in the custom FA catalog, render the default internet icon as a graceful fallback
    return <DefaultInternetIcon className={className} />
  }

  const activeColor = color ?? parsed.color ?? icon.color

  return (
    <svg
      viewBox={icon.viewBox ?? '0 0 512 512'}
      fill="currentColor"
      className={cn('shrink-0 select-none inline-block', className)}
      style={{ color: activeColor, ...style }}
      aria-label={icon.name}
      role="img"
    >
      <path d={icon.svgPath} />
    </svg>
  )
}

/**
 * Main SiteLogo component.
 * Priority:
 * 1. iconImage: local `ols_site_icon.webp` file in project directory (data URL)
 *    Rendered square with border radius, subtle shadow, and object-cover.
 * 2. icon: Font Awesome icon name (offline version)
 * 3. Default: Clean, colorful modern internet globe icon (no box/border)
 */
export function SiteLogo({
  icon,
  iconImage,
  hostname,
  className,
}: {
  icon?: string | null
  iconImage?: string | null
  hostname?: string
  className?: string
}) {
  if (iconImage) {
    return (
      <img
        src={iconImage}
        alt={hostname ? `Logo for ${hostname}` : 'Site logo'}
        className={cn(
          'shrink-0 select-none aspect-square rounded-md object-cover shadow-xs border border-border/60 overflow-hidden bg-muted/20',
          className || 'size-7.5',
        )}
        onError={(e) => {
          // If image fails to load or corrupted, hide and fallback handled by parent
          e.currentTarget.style.display = 'none'
        }}
      />
    )
  }

  if (icon) {
    return <FontAwesomeIcon name={icon} className={cn('size-6', className)} />
  }

  return <DefaultInternetIcon className={cn('size-6', className)} />
}
