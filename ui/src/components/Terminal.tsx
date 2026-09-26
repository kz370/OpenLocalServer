import { listen } from '@tauri-apps/api/event'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal as XTerm } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'
import { useEffect, useRef, useState } from 'react'

import { ErrorCard, asDiagnostic } from '@/components/ErrorCard'
import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/form'
import { type Diagnostic, type TerminalEvent, runCommand } from '@/core'

/**
 * A real shell for a project (§19): PowerShell or cmd, started in the project's folder with its
 * runtimes first on PATH. The shell runs in the app's core; this only draws it and sends keys.
 */
export function ProjectTerminal({ projectId }: { projectId: string | null }) {
  const host = useRef<HTMLDivElement>(null)
  const [shell, setShell] = useState('powershell')
  const [session, setSession] = useState(0)
  const [ended, setEnded] = useState(false)
  const [error, setError] = useState<Diagnostic | null>(null)

  useEffect(() => {
    const el = host.current
    if (!el) return
    let alive = true
    let id: number | null = null
    let unlisten: (() => void) | null = null
    setEnded(false)
    setError(null)

    const term = new XTerm({ cursorBlink: true, fontSize: 13, fontFamily: 'Consolas, "Cascadia Mono", monospace', scrollback: 5000, convertEol: false })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(el)
    fit.fit()

    // Output can arrive before the open call returns the session id, so it is held until then.
    const early: TerminalEvent[] = []
    const show = (e: TerminalEvent) => {
      if (e.kind === 'output') term.write(e.data)
      else {
        term.write('\r\n\x1b[90m[the shell has exited]\x1b[0m\r\n')
        setEnded(true)
      }
    }
    const send = (cmd: Parameters<typeof runCommand>[0]) => runCommand(cmd).catch(() => undefined)

    void (async () => {
      unlisten = await listen<TerminalEvent>('terminal-event', (event) => {
        const e = event.payload
        if (id === null) early.push(e)
        else if (e.id === id) show(e)
      })
      if (!alive) return unlisten()
      try {
        const res = await runCommand({ type: 'open_terminal', project_id: projectId, shell, rows: term.rows, cols: term.cols })
        if (res.type === 'terminal') {
          id = res.id
          early.filter((e) => e.id === res.id).forEach(show)
        }
        if (!alive && id !== null) void send({ type: 'close_terminal', id })
      } catch (e) {
        setError(asDiagnostic(e))
      }
    })()

    const typed = term.onData((data) => {
      if (id !== null) void send({ type: 'terminal_input', id, data })
    })
    const resize = () => {
      fit.fit()
      if (id !== null) void send({ type: 'resize_terminal', id, rows: term.rows, cols: term.cols })
    }
    const observer = new ResizeObserver(resize)
    observer.observe(el)

    return () => {
      alive = false
      observer.disconnect()
      typed.dispose()
      unlisten?.()
      if (id !== null) void send({ type: 'close_terminal', id })
      term.dispose()
    }
  }, [projectId, shell, session])

  return (
    <div className="flex flex-col gap-2">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex items-center justify-between gap-2">
        <Select className="w-40" value={shell} onChange={(e) => setShell(e.target.value)}>
          <option value="powershell">PowerShell</option>
          <option value="cmd">Command Prompt</option>
        </Select>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">php, node, composer, python and the database clients are the ones this project uses.</span>
          <Button size="sm" variant={ended ? 'default' : 'ghost'} onClick={() => setSession((n) => n + 1)}>
            {ended ? 'Start a new shell' : 'Restart'}
          </Button>
        </div>
      </div>
      <div ref={host} className="h-80 overflow-hidden rounded-md bg-black p-2" />
    </div>
  )
}
