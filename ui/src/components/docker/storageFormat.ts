/** Bytes as KB / MB / GB with one decimal. */
export function formatBytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v >= 10 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`
}

/** "3 days ago" from Unix seconds. */
export function ago(unixSeconds: number): string {
  if (!unixSeconds) return '—'
  const s = Math.max(0, Date.now() / 1000 - unixSeconds)
  const steps: [number, string][] = [
    [60, 'second'],
    [60, 'minute'],
    [24, 'hour'],
    [30, 'day'],
    [12, 'month'],
    [Infinity, 'year'],
  ]
  let v = s
  for (const [size, unit] of steps) {
    if (v < size) {
      const n = Math.floor(v)
      return `${n} ${unit}${n === 1 ? '' : 's'} ago`
    }
    v /= size
  }
  return '—'
}
