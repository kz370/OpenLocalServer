import { listen } from '@tauri-apps/api/event'
import { Play } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { type Diagnostic, type PortStatus, type ProcessEvent, type ProcessInfo, runCommand } from '@/core'

const STATE_VARIANT: Record<ProcessInfo['state'], 'success' | 'secondary' | 'destructive' | 'outline'> = {
  running: 'success',
  starting: 'secondary',
  restarting: 'secondary',
  stopping: 'secondary',
  stopped: 'outline',
  crashed: 'destructive',
  failed: 'destructive',
  unknown: 'outline',
}

const PRESETS = [
  { label: 'ping (10s)', executable: 'ping', args: '127.0.0.1 -n 10' },
  { label: 'echo hello', executable: 'cmd', args: '/C echo hello from OpenLocalServer' },
  { label: 'exit 1 (crash demo)', executable: 'cmd', args: '/C exit 1' },
]

export function ProcessesPage() {
  const [processes, setProcesses] = useState<ProcessInfo[]>([])
  const [outputs, setOutputs] = useState<Record<number, string[]>>({})
  const [selected, setSelected] = useState<number | null>(null)

  const [name, setName] = useState('demo')
  const [executable, setExecutable] = useState('ping')
  const [args, setArgs] = useState('127.0.0.1 -n 10')

  const [port, setPort] = useState('3306')
  const [portResult, setPortResult] = useState<PortStatus | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const outputsRef = useRef(outputs)
  outputsRef.current = outputs

  async function refresh() {
    const res = await runCommand({ type: 'list_processes' })
    if (res.type === 'processes') setProcesses(res.processes)
  }

  useEffect(() => {
    refresh()
    const unlisten = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (e.kind === 'state_changed') {
        refresh()
      } else if (e.kind === 'output') {
        setOutputs((prev) => {
          const lines = prev[e.id] ?? []
          return { ...prev, [e.id]: [...lines.slice(-199), e.line] }
        })
      }
    })
    return () => {
      unlisten.then((f) => f())
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function startProcess() {
    setError(null)
    try {
      const res = await runCommand({
        type: 'start_process',
        spec: {
          name,
          executable,
          args: args.split(' ').filter(Boolean),
          cwd: null,
          env: [],
          restart: null,
        },
      })
      if (res.type === 'process_started') {
        setSelected(res.id)
        refresh()
      }
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function stopProcess(id: number) {
    await runCommand({ type: 'stop_process', id })
  }

  async function checkPort() {
    setError(null)
    try {
      const res = await runCommand({ type: 'check_port', port: Number(port) })
      if (res.type === 'port') setPortResult(res.status)
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  const selectedOutput = selected !== null ? (outputs[selected] ?? []) : []

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Processes</h1>
        <p className="text-sm text-muted-foreground">
          The Process Supervisor: start, stop, watch live output, and crash-restart (§107–108).
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Start a managed process</CardTitle>
          <CardDescription>Runs through the same supervisor that will run PHP-FPM, Nginx, MySQL, etc.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-wrap gap-2">
            {PRESETS.map((p) => (
              <Button
                key={p.label}
                variant="secondary"
                size="sm"
                onClick={() => {
                  setExecutable(p.executable)
                  setArgs(p.args)
                  setName(p.label)
                }}
              >
                {p.label}
              </Button>
            ))}
          </div>
          <div className="flex flex-wrap gap-2">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="name" className="w-32" />
            <Input
              value={executable}
              onChange={(e) => setExecutable(e.target.value)}
              placeholder="executable"
              className="w-32"
            />
            <Input
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              placeholder="args (space separated)"
              className="flex-1 min-w-40"
            />
            <Button onClick={startProcess} size="sm">
              <Play /> Start
            </Button>
          </div>
        </CardContent>
      </Card>

      <div className="grid gap-4 lg:grid-cols-[1fr_1.2fr]">
        <Card>
          <CardHeader>
            <CardTitle>Running &amp; recent</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {processes.length === 0 && (
              <p className="text-sm text-muted-foreground">No processes started yet.</p>
            )}
            {processes.map((p) => (
              <div
                key={p.id}
                role="button"
                tabIndex={0}
                onClick={() => setSelected(p.id)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') setSelected(p.id)
                }}
                className={`flex cursor-pointer items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors ${
                  selected === p.id ? 'border-primary bg-accent' : 'border-border hover:bg-accent/50'
                }`}
              >
                <div className="flex flex-col">
                  <span className="font-medium">{p.name}</span>
                  <span className="text-xs text-muted-foreground">
                    {p.pid ? `PID ${p.pid}` : '—'}
                    {p.restarts > 0 && ` · ${p.restarts} restart${p.restarts > 1 ? 's' : ''}`}
                    {p.exit_code !== null && ` · exit ${p.exit_code}`}
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <Badge variant={STATE_VARIANT[p.state]}>{p.state}</Badge>
                  {(p.state === 'running' || p.state === 'starting') && (
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={(e) => {
                        e.stopPropagation()
                        stopProcess(p.id)
                      }}
                      title="Stop"
                    >
                      <StopIcon className="size-3.5" />
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Live output {selected !== null && `— #${selected}`}</CardTitle>
          </CardHeader>
          <CardContent>
            <pre className="h-64 overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">
              {selected === null
                ? 'Select a process to view its output.'
                : selectedOutput.length === 0
                  ? '(no output yet)'
                  : selectedOutput.join('\n')}
            </pre>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Port checker</CardTitle>
          <CardDescription>§109 — reports the owner instead of guessing; never kills anything.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-2">
          <Input value={port} onChange={(e) => setPort(e.target.value)} className="w-24" />
          <Button onClick={checkPort} size="sm" variant="secondary">
            Check
          </Button>
          {portResult && (
            <span className="text-sm">
              {portResult.status === 'free' ? (
                <Badge variant="success">Free</Badge>
              ) : (
                <>
                  <Badge variant="destructive">In use</Badge>{' '}
                  <span className="text-muted-foreground">
                    {portResult.process_name ?? 'unknown process'}
                    {portResult.pid && ` (PID ${portResult.pid})`}
                  </span>
                </>
              )}
            </span>
          )}
        </CardContent>
      </Card>

      {error && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardHeader>
            <CardTitle className="text-destructive">{error.problem}</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-sm">{error.cause}</p>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
