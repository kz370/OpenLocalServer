import { Cpu, HardDrive, MemoryStick } from 'lucide-react'
import type { ReactNode } from 'react'

import { TechIcon } from '@/components/TechIcon'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { SiteUsage, SystemStats } from '@/core'
import { formatBytes } from '@/lib/hooks'

/** Each ring's own colour. CPU and RAM are close relatives (teal / blue-teal); disks differ. */
const CPU_COLOR = 'var(--primary)'
const RAM_COLOR = 'oklch(0.68 0.12 220)'
const DISK_COLORS = ['oklch(0.66 0.16 295)', 'oklch(0.7 0.14 340)', 'oklch(0.72 0.13 145)', 'oklch(0.7 0.12 250)']

/** The ring keeps its colour while there's headroom, drifts to amber past 60%, red at 90%. */
function ringColor(base: string, percent: number): string {
  if (percent >= 90) return 'var(--destructive)'
  if (percent <= 60) return base
  const toward = Math.round(((percent - 60) / 30) * 100)
  return `color-mix(in oklch, ${base}, var(--warning) ${toward}%)`
}

/** Machine resources on their own row, then what each site is using (Processes page). */
export function SystemMonitor({ stats }: { stats: SystemStats | null }) {
  const memPct = stats ? (stats.memory_used / Math.max(stats.memory_total, 1)) * 100 : 0
  const sites = [...(stats?.sites ?? [])].sort((a, b) => b.memory - a.memory)
  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Resources</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-wrap items-start justify-around gap-6">
          <Ring
            icon={<Cpu />}
            label="CPU"
            base={CPU_COLOR}
            percent={stats?.cpu_percent ?? 0}
            detail={stats ? `${stats.cpu_cores} cores` : ''}
            loading={!stats}
          />
          <Ring
            icon={<MemoryStick />}
            label="RAM"
            base={RAM_COLOR}
            percent={memPct}
            detail={stats ? `${formatBytes(stats.memory_used)} of ${formatBytes(stats.memory_total)}` : ''}
            loading={!stats}
          />
          {(stats?.disks ?? []).map((d, i) => (
            <Ring
              key={d.mount}
              icon={<HardDrive />}
              label={`Disk ${d.mount.replace(/\\$/, '')}`}
              base={DISK_COLORS[i % DISK_COLORS.length]}
              percent={(d.used / Math.max(d.total, 1)) * 100}
              detail={`${formatBytes(d.total - d.used)} free · ${d.holds.join(', ')}`}
            />
          ))}
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
            // The header scrolls with the rows (sticky) so a scrollbar can't shift the columns.
            <div className="max-h-72 overflow-y-auto">
              <div className={`${SITE_GRID} sticky top-0 z-10 h-7 border-b border-border bg-card text-[11px] font-medium uppercase tracking-wide text-muted-foreground`}>
                <span>Site</span>
                <span className="text-right" title="Share of the whole machine's CPU">
                  CPU
                </span>
                <span className="text-right" title="Memory in use">
                  RAM
                </span>
                <span className="text-right" title="Size of the site's folder, counted every 10 minutes">
                  Disk
                </span>
              </div>
              <div className="divide-y divide-border">
                {sites.map((s) => (
                  <SiteRow key={s.hostname} site={s} />
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}

/** Shared by the header and every row so the columns line up exactly. */
const SITE_GRID = 'grid grid-cols-[minmax(0,1fr)_4.5rem_5.5rem_5.5rem] items-center gap-3 px-1'

function SiteRow({ site }: { site: SiteUsage }) {
  const icon = site.via.startsWith('PHP') ? 'php' : site.via.startsWith('Web') ? 'static' : 'proxy'
  const shared = site.shared_by > 1 ? ` · shared by ${site.shared_by} sites` : ''
  const num = 'text-right text-xs tabular-nums'
  return (
    <div className={`${SITE_GRID} h-9 text-sm`}>
      <span className="flex min-w-0 items-center gap-2.5">
        <TechIcon id={icon} className="size-3.5" />
        <span className="truncate">
          <span className="font-medium">{site.hostname}</span>
          <span className="ml-2 text-xs text-muted-foreground">
            {site.via}
            {shared}
          </span>
        </span>
      </span>
      {site.measured ? (
        <>
          <span className={num}>{site.cpu_percent.toFixed(1)}%</span>
          <span className={num}>{formatBytes(site.memory)}</span>
        </>
      ) : (
        <span className="col-span-2 text-right text-xs text-muted-foreground">not on this computer</span>
      )}
      <span className={num}>{site.disk === null ? '…' : formatBytes(site.disk)}</span>
    </div>
  )
}

/** A donut: the arc is the used share, in the ring's colour (warming up as it fills). */
function Ring({ icon, label, base, percent, detail, loading }: { icon: ReactNode; label: string; base: string; percent: number; detail: string; loading?: boolean }) {
  const r = 34
  const c = 2 * Math.PI * r
  const p = Math.min(100, Math.max(0, percent))
  const color = ringColor(base, p)
  return (
    <div className="flex w-36 flex-col items-center gap-1.5 text-center">
      <div className="relative size-24">
        <svg viewBox="0 0 80 80" className="size-full -rotate-90">
          <circle cx="40" cy="40" r={r} fill="none" stroke="var(--muted)" strokeWidth="8" />
          <circle
            cx="40"
            cy="40"
            r={r}
            fill="none"
            stroke={color}
            strokeWidth="8"
            strokeLinecap="round"
            strokeDasharray={c}
            strokeDashoffset={c * (1 - p / 100)}
            className="transition-[stroke-dashoffset,stroke] duration-500"
          />
        </svg>
        <span className="absolute inset-0 flex items-center justify-center text-lg font-semibold tabular-nums" style={{ color }}>
          {loading ? '…' : `${p.toFixed(0)}%`}
        </span>
      </div>
      <span className="flex items-center gap-1 text-sm font-medium [&>svg]:size-3.5" style={{ color: loading ? undefined : base }}>
        {icon}
        {label}
      </span>
      <span className="text-xs text-muted-foreground">{detail}</span>
    </div>
  )
}
