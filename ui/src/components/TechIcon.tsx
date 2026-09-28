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
  // simple-icons has no Memcached mark, so it borrows Redis' cache-family tile.
  memcached: siRedis,
  sqlite: siSqlite,
}

/**
 * Mailpit's mark: an envelope in isometric, so the lid reads as a top face and the
 * seam as the fold. simple-icons has no mail-trap brand, so it is drawn here.
 */
const ENVELOPE_3D = (
  <svg viewBox="0 0 24 24" role="img" aria-label="Mailpit">
    <path d="M4 7.5 12 3.2l8 4.3v9L12 20.8 4 16.5v-9Z" fill="currentColor" fillOpacity="0.1" />
    <path d="M12 3.2 20 7.5l-8 4.3-8-4.3 8-4.3Z" fill="currentColor" fillOpacity="0.34" />
    <path
      d="M4 7.5 12 11.8l8-4.3M12 11.8v9"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      fill="none"
      opacity="0.85"
    />
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
  mailpit: ENVELOPE_3D,
  mail: ENVELOPE_3D,
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
