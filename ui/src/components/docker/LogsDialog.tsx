import { RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { type Diagnostic, runCommand } from '@/core'
import { cn } from '@/lib/utils'

/** §166: the last 500 log lines of a container. Loads on open and on Refresh; no polling. */
export function LogsDialog({ container, onClose }: { container: string | null; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [filter, setFilter] = useState('')
  const box = useRef<HTMLDivElement>(null)

  const load = useCallback(async (id: string) => {
    try {
      const r = await runCommand({ type: 'get_docker_container_logs', id, tail: 500 })
      if (r.type === 'docker_logs') {
        setLines(r.lines)
        setError(null)
      }
    } catch (err) {
      setError(asDiagnostic(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (container) void load(container)
  }, [container, load])

  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase()
    return f ? lines.filter((l) => l.toLowerCase().includes(f)) : lines
  }, [lines, filter])

  useEffect(() => {
    const el = box.current
    if (el) el.scrollTop = el.scrollHeight
  }, [shown])

  return (
    <Dialog
      open={!!container}
      onClose={onClose}
      size="wide"
      title={`Logs · ${container ?? ''}`}
      description="The last 500 lines. Passwords and tokens are hidden."
    >
      <div className="mb-3 flex items-center gap-2">
        <Input placeholder="Filter lines…" value={filter} onChange={(e) => setFilter(e.target.value)} className="h-8" />
        <Button
          size="sm"
          variant="outline"
          className="cursor-pointer"
          disabled={loading}
          onClick={() => {
            if (!container) return
            setLoading(true)
            void load(container)
          }}
        >
          <RefreshCw className={cn('size-3.5', loading && 'animate-spin')} /> Refresh
        </Button>
      </div>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div
        ref={box}
        className="h-96 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all"
      >
        {shown.length ? shown.join('\n') : filter ? 'No lines match the filter.' : 'No log lines yet.'}
      </div>
    </Dialog>
  )
}
