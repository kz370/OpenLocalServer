import { ArrowLeftRight, Blocks, FileCode2, Layers, Package, Wrench } from 'lucide-react'
import type { ReactNode } from 'react'
import {
  type SimpleIcon,
  siApache,
  siCaddy,
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
  siPhp,
  siPostgresql,
  siPython,
  siReact,
  siRedis,
  siSqlite,
  siSymfony,
  siVuedotjs,
  siWordpress,
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
}

/** Generic stand-ins for ids without a brand. */
const GENERIC: Record<string, ReactNode> = {
  all: <Layers />,
  custom: <Blocks />,
  'custom-app': <Blocks />,
  tools: <Wrench />,
  proxy: <ArrowLeftRight />,
  'reverse-proxy': <ArrowLeftRight />,
  other: <Package />,
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
