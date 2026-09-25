import { Cpu, MemoryStick } from 'lucide-react'
import type { ReactNode } from 'react'

import { TechIcon } from '@/components/TechIcon'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { SiteUsage, SystemStats } from '@/core'
import { formatBytes } from '@/lib/hooks'

/** Machine CPU and memory as rings, then what each site is using (Processes page). */
export function SystemMonitor({ stats }: { stats: SystemStats | null }) {
  const memPct = stats ? (stats.memory_used / Math.max(stats.memory_total, 1)) * 100 : 0
  const sites = [...(stats?.sites ?? [])].sort((a, b) => b.memory - a.memory)
  return (
    <div className="grid gap-4 lg:grid-cols-[auto_1fr]">
      <Card>
        <CardContent className="flex items-center justify-around gap-6 pt-5">
          <Ring icon={<Cpu />} label="CPU" percent={stats?.cpu_percent ?? 0} value={stats ? `${stats.cpu_percent.toFixed(0)}%` : '…'} detail={stats ? `${stats.cpu_cores} cores` : ''} />
          <Ring
            icon={<MemoryStick />}
            label="Memory"
            percent={memPct}
            value={stats ? `${memPct.toFixed(0)}%` : '…'}
            detail={stats ? `${formatBytes(stats.memory_used)} / ${formatBytes(stats.memory_total)}` : ''}
          />
        </CardContent>
      </Card>

      <Card className="min-w-0">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Usage by site</CardTitle>
        </CardHeader>
        <CardContent>
          {sites.length === 0 ? (
            <p className="text-sm text-muted-foreground">No sites are running.</p>
          ) : (
            <div className="max-h-52 divide-y divide-border overflow-y-auto">
              {sites.map((s) => (
                <SiteRow key={s.hostname} site={s} heaviest={Math.max(1, sites[0]?.memory ?? 1)} />
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

/** One site; its bar is its memory relative to the heaviest site. */
function SiteRow({ site, heaviest }: { site: SiteUsage; heaviest: number }) {
  const icon = site.via.startsWith('PHP') ? 'php' : site.via.startsWith('Web') ? 'static' : 'proxy'
  const shared = site.shared_by > 1 ? ` · shared by ${site.shared_by} sites` : ''
  return (
    <div className="flex h-9 items-center gap-2.5 text-sm">
      <TechIcon id={icon} className="size-3.5" />
      <span className="min-w-0 flex-1 truncate">
        <span className="font-medium">{site.hostname}</span>
        <span className="ml-2 text-xs text-muted-foreground">
          {site.via}
          {shared}
        </span>
      </span>
      {site.measured ? (
        <>
          <span className="w-14 text-right text-xs tabular-nums text-muted-foreground">{site.cpu_percent.toFixed(1)}%</span>
          <span className="w-16 text-right text-xs tabular-nums">{formatBytes(site.memory)}</span>
          <span className="hidden h-1 w-20 overflow-hidden rounded-full bg-muted sm:block">
            <span className="block h-full rounded-full bg-primary" style={{ width: `${(site.memory / heaviest) * 100}%` }} />
          </span>
        </>
      ) : (
        <span className="text-xs text-muted-foreground">not on this computer</span>
      )}
    </div>
  )
}

/** A donut: the arc is the used share, coloured by how full it is. */
function Ring({ icon, label, percent, value, detail }: { icon: ReactNode; label: string; percent: number; value: string; detail: string }) {
  const r = 34
  const c = 2 * Math.PI * r
  const p = Math.min(100, Math.max(0, percent))
  const tone = p >= 90 ? 'var(--destructive)' : p >= 70 ? 'var(--warning)' : 'var(--primary)'
  return (
    <div className="flex flex-col items-center gap-1.5">
      <div className="relative size-24">
        <svg viewBox="0 0 80 80" className="size-full -rotate-90">
          <circle cx="40" cy="40" r={r} fill="none" stroke="var(--muted)" strokeWidth="8" />
          <circle
            cx="40"
            cy="40"
            r={r}
            fill="none"
            stroke={tone}
            strokeWidth="8"
            strokeLinecap="round"
            strokeDasharray={c}
            strokeDashoffset={c * (1 - p / 100)}
            className="transition-[stroke-dashoffset] duration-500"
          />
        </svg>
        <span className="absolute inset-0 flex items-center justify-center text-lg font-semibold tabular-nums">{value}</span>
      </div>
      <span className="flex items-center gap-1 text-sm font-medium [&>svg]:size-3.5">
        {icon}
        {label}
      </span>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  )
}
