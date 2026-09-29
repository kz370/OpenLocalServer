import { ArrowLeftRight, Blocks, FileCode2, Layers, Package, Wrench } from 'lucide-react'
import type { ReactNode } from 'react'
import {
  type SimpleIcon,
  siApache,
  siBun,
  siDotnet,
  siGo,
  siK6,
  siOpenjdk,
  siCaddy,
  siComposer,
  siDjango,
  siExpress,
  siFastapi,
  siGit,
  siHtml5,
  siLaravel,
  siMariadb,
  siMongodb,
  siMysql,
  siNextdotjs,
  siNginx,
  siNodedotjs,
  siNpm,
  siPhp,
  siPnpm,
  siPostgresql,
  siPython,
  siReact,
  siRedis,
  siSqlite,
  siSymfony,
  siVuedotjs,
  siWordpress,
  siYarn,
} from 'simple-icons'

import { cn } from '@/lib/utils'

/** Brand marks by the ids used across the app (Quick Apps, command categories, servers). */
const BRANDS: Record<string, SimpleIcon> = {
  laravel: siLaravel,
  symfony: siSymfony,
  wordpress: siWordpress,
  php: siPhp,
  'plain-php': siPhp,
  node: siNodedotjs,
  npm: siNpm,
  pnpm: siPnpm,
  yarn: siYarn,
  bun: siBun,
  go: siGo,
  java: siOpenjdk,
  dotnet: siDotnet,
  k6: siK6,
  composer: siComposer,
  'express-api': siExpress,
  react: siReact,
  'react-vite': siReact,
  vue: siVuedotjs,
  'vue-vite': siVuedotjs,
  nextjs: siNextdotjs,
  python: siPython,
  django: siDjango,
  fastapi: siFastapi,
  static: siHtml5,
  'static-html': siHtml5,
  nginx: siNginx,
  apache: siApache,
  caddy: siCaddy,
  mysql: siMysql,
  mariadb: siMariadb,
  mongodb: siMongodb,
  postgres: siPostgresql,
  redis: siRedis,
  sqlite: siSqlite,
  git: siGit,
}

/**
 * Mailpit's mark: an envelope, drawn here because simple-icons has no mail-trap brand.
 * The earlier isometric outline read as an empty shape at 16px, so this is a filled body
 * with a full-strength outline and flap.
 */
const ENVELOPE = (
  <svg viewBox="0 0 24 24" role="img" aria-label="Mailpit">
    <rect x="2.6" y="5" width="18.8" height="14" rx="2.2" fill="currentColor" fillOpacity="0.18" />
    <path
      d="M3.6 6.6 12 13l8.4-6.4"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <rect x="2.6" y="5" width="18.8" height="14" rx="2.2" fill="none" stroke="currentColor" strokeWidth="1.6" />
  </svg>
)

/** Tiny RDM: a desktop client window over a key/value list — a Redis GUI, not a server. */
const REDIS_CLIENT = (
  <svg viewBox="0 0 24 24" role="img" aria-label="Tiny RDM">
    <rect x="2.5" y="4" width="19" height="16" rx="2.2" fill="currentColor" fillOpacity="0.14" />
    <rect x="2.5" y="4" width="19" height="16" rx="2.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <path d="M2.5 8.4h19" stroke="currentColor" strokeWidth="1.5" />
    <path
      d="M5.2 12h5.4M5.2 14.8h9M5.2 17.4h4.4"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      opacity="0.75"
    />
  </svg>
)

/** Memcached's supplied symbol-only logo, scaled to the runtime icon slot. */
const MEMCACHED_MARK = (
  <svg viewBox="0 0 24 24" role="img" aria-label="Memcached">
    <rect x="1" y="1" width="22" height="22" rx="5.2" fill="#756b6d" />
    <path d="M5.1 19.1 6.2 5.3h3.1l2.7 5.3 2.7-5.3h3.1l1.1 13.8h-3.2l-.6-7.8-3.1 5.2-3.1-5.2-.6 7.8z" fill="#36a69f" />
    <circle cx="10.3" cy="19.2" r=".85" fill="#f05d67" />
    <circle cx="14.5" cy="19.2" r=".85" fill="#f05d67" />
  </svg>
)

/** Generic stand-ins for ids without a brand. */
const GENERIC: Record<string, ReactNode> = {
  all: <Layers />,
  custom: <Blocks />,
  'custom-app': <Blocks />,
  tools: <Wrench />,
  proxy: <ArrowLeftRight />,
  'reverse-proxy': <ArrowLeftRight />,
  other: <Package />,
  memcached: MEMCACHED_MARK,
  mailpit: ENVELOPE,
  mail: ENVELOPE,
  tinyrdm: REDIS_CLIENT,
}

/** Near-black brand colours vanish on a dark background; those follow the text colour. */
function isDark(hex: string): boolean {
  const n = parseInt(hex, 16)
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255]
  return 0.299 * r + 0.587 * g + 0.114 * b < 60
}

/** A bare brand mark (for tabs and chips), sized like a lucide icon. */
export function TechIcon({ id, className }: { id: string; className?: string }) {
  const brand = BRANDS[id]
  if (!brand) {
    const generic = GENERIC[id] ?? <FileCode2 />
    return <span className={cn('inline-flex size-4 shrink-0 [&>svg]:size-full', className)}>{generic}</span>
  }
  return (
    <svg viewBox="0 0 24 24" role="img" aria-label={brand.title} className={cn('size-4 shrink-0', className)} fill={isDark(brand.hex) ? 'currentColor' : `#${brand.hex}`}>
      <path d={brand.path} />
    </svg>
  )
}

/** The mark on a soft tinted tile (for cards). */
export function TechTile({ id, className }: { id: string; className?: string }) {
  const brand = BRANDS[id]
  const tint = brand && !isDark(brand.hex) ? `#${brand.hex}1f` : undefined
  return (
    <span className={cn('flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted', className)} style={tint ? { backgroundColor: tint } : undefined}>
      <TechIcon id={id} className="size-5" />
    </span>
  )
}
