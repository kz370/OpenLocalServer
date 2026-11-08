import { listen } from '@tauri-apps/api/event'
import { Play, Square, Trash2 } from 'lucide-react'
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { SwitchRow } from '@/components/ui/form'
import { type Diagnostic, type DumpStatus, type ProcessEvent, type ProjectDetail, runCommand } from '@/core'
import { asDiagnostic } from '@/lib/diagnostic'

/** A Windows path to a PHP file followed by `on line N` or `:N`. */
const FILE_LINE = /([A-Za-z]:[\\/][^\s"'<>|]+?\.php)(?:"|'|\s)*(?:on line|:)\s*(\d+)/g

/** Splits one output line into plain text and clickable file:line links. */
function linkify(line: string, open: (path: string, n: number) => void): ReactNode[] {
  const parts: ReactNode[] = []
  let last = 0
  for (const m of line.matchAll(FILE_LINE)) {
    const at = m.index ?? 0
    if (at > last) parts.push(line.slice(last, at))
    const path = m[1]
    const n = Number(m[2])
    parts.push(
      <button key={at} type="button" className="cursor-pointer text-primary underline" onClick={() => open(path, n)}>
        {m[0]}
      </button>,
    )
    last = at + m[0].length
  }
  if (last < line.length) parts.push(line.slice(last))
  return parts.length ? parts : [line]
}

/**
 * §175 Dump viewer: runs the project's own `var-dump-server` and shows what it prints, so
 * `dump()` and `dd()` land here instead of on the page. Output is plain text, never HTML.
 */
export function DumpsPanel({ detail }: { detail: ProjectDetail }) {
  const projectId = detail.project.id
  const [status, setStatus] = useState<DumpStatus | null>(null)
  const [lines, setLines] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)
  const outRef = useRef<HTMLPreElement>(null)
  const running = status?.running ?? null

  const load = useCallback(async () => {
    try {
      const r = await runCommand({ type: 'get_dump_status', project_id: projectId })
      if (r.type === 'dump_status') setStatus(r.status)
    } catch (e) {
      setError(asDiagnostic(e))
    }
  }, [projectId])
  useEffect(() => {
    void load()
  }, [load])

  // The newest lines the server printed so far (the core keeps the last 500).
  useEffect(() => {
    if (running === null) return
    void runCommand({ type: 'get_process_output', id: running })
      .then((r) => {
        if (r.type === 'process_output') setLines(r.lines.slice(-500))
      })
      .catch(() => undefined)
  }, [running])

  useEffect(() => {
    if (running === null) return
    const un = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (e.id !== running) return
      if (e.kind === 'output') setLines((prev) => [...prev.slice(-499), e.line])
      else if (e.kind === 'state_changed' && (e.state === 'stopped' || e.state === 'failed')) void load()
    })
    return () => {
      un.then((f) => f())
    }
  }, [running, load])

  useEffect(() => {
    outRef.current?.scrollTo({ top: outRef.current.scrollHeight })
  }, [lines.length])

  async function toggle() {
    setBusy(true)
    setError(null)
    try {
      await runCommand(running === null ? { type: 'start_dump_server', project_id: projectId } : { type: 'stop_dump_server', project_id: projectId })
      await load()
    } catch (e) {
      setError(asDiagnostic(e))
    } finally {
      setBusy(false)
    }
  }

  const openAt = (path: string, line: number) => void runCommand({ type: 'open_in_editor_at', path, line }).catch((e) => setError(asDiagnostic(e)))

  if (status && !status.enabled) {
    return <p className="text-sm text-muted-foreground">The dump viewer is off. Turn on Dump viewer in Settings, then come back here to start it.</p>
  }
  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border px-3 py-2 text-sm">
        <span className="flex items-center gap-2">
          {running !== null ? <Badge variant="success">Listening on port {status?.port}</Badge> : <Badge variant="outline">Stopped</Badge>}
          {status?.conflict && <Badge variant="outline">Your php.ini already sets auto_prepend_file</Badge>}
          {status && !status.supported && <Badge variant="outline">symfony/var-dumper missing</Badge>}
        </span>
        <span className="flex gap-1">
          <Button size="sm" className="cursor-pointer" disabled={busy || !status} onClick={() => void toggle()}>
            {busy ? <Spinner /> : running === null ? <Play /> : <Square />}
            {running === null ? 'Start' : 'Stop'}
          </Button>
          <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => setLines([])}>
            <Trash2 /> Clear
          </Button>
        </span>
      </div>
      {status && !status.supported && (
        <p className="text-xs text-muted-foreground">
          This project needs symfony/var-dumper: <code>composer require --dev symfony/var-dumper</code> (Laravel and Symfony already have it).
        </p>
      )}
      <pre ref={outRef} className="max-h-96 min-h-40 overflow-auto rounded-md border border-border bg-muted/30 p-3 font-mono text-xs whitespace-pre-wrap">
        {lines.length === 0 ? <span className="text-muted-foreground">Dumps appear here. Shows the newest 500 lines.</span> : lines.map((l, i) => <div key={i}>{linkify(l, openAt)}</div>)}
      </pre>
      <p className="text-xs text-muted-foreground">
        dd() and dump() now go here instead of the page. dd() still stops the request. Restart workers to send their dumps here too.
      </p>
    </div>
  )
}

/** Settings card: the global dump viewer switch. */
export function DumpViewerSettings() {
  const [on, setOn] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    void runCommand({ type: 'get_setting', key: 'php.dump_capture' })
      .then((r) => {
        if (r.type === 'setting' && typeof r.value === 'boolean') setOn(r.value)
      })
      .catch(() => undefined)
  }, [])

  async function change(v: boolean) {
    setOn(v)
    setError(null)
    try {
      await runCommand({ type: 'set_dump_capture', on: v })
    } catch (e) {
      setOn(!v)
      setError(asDiagnostic(e))
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <SwitchRow
        checked={on}
        onChange={(v) => void change(v)}
        title="Dump viewer"
        hint="Start it in a project's Dumps tab, and dump() / dd() output appears in OLS instead of on the page. Turning this on or off restarts the running PHP pools once."
      />
    </div>
  )
}
