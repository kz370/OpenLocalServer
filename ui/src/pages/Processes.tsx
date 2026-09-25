import { listen } from '@tauri-apps/api/event'
import { Play } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { SystemMonitor } from '@/components/SystemMonitor'
import { StopIcon } from '@/components/StopIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { type Diagnostic, type PortStatus, type ProcessEvent, type ProcessInfo, type SystemStats, runCommand } from '@/core'
import { formatBytes, usePoll } from '@/lib/hooks'
import { waitForProcessExit } from '@/lib/wait'

const PRESETS = [
  { label: 'ping (10s)', executable: 'ping', args: '127.0.0.1 -n 10' },
  { label: 'echo hello', executable: 'cmd', args: '/C echo hello from OpenLocalServer' },
  { label: 'exit 1 (crash demo)', executable: 'cmd', args: '/C exit 1' },
]

export function ProcessesPage() {
  const [processes, setProcesses] = useState<ProcessInfo[]>([])
  const [outputs, setOutputs] = useState<Record<number, string[]>>({})
  const [selected, setSelected] = useState<number | null>(null)
  const [stopping, setStopping] = useState<number | null>(null)

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

  const [stats, setStats] = useState<SystemStats | null>(null)
  usePoll(async () => {
    const r = await runCommand({ type: 'get_system_stats' }).catch(() => null)
    if (r?.type === 'system_stats') setStats(r.stats)
  }, 2000)

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
    setStopping(id)
    try {
      await runCommand({ type: 'stop_process', id })
      await waitForProcessExit(id)
    } catch (err) {
      setError(err as Diagnostic)
    } finally {
      setStopping(null)
    }
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

      <SystemMonitor stats={stats} />

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
            <CardTitle className="text-sm">Running &amp; recent · {processes.filter((p) => isLive(p.state)).length} running</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {processes.length === 0 && (
              <p className="text-sm text-muted-foreground">No processes started yet.</p>
            )}
            <div className="max-h-96 overflow-y-auto">
              {[...processes]
                // Running first, then what finished, newest first.
                .sort((a, b) => Number(isLive(b.state)) - Number(isLive(a.state)) || b.id - a.id)
                .map((p) => {
                  const usage = p.pid ? stats?.processes[p.pid] : undefined
                  const live = isLive(p.state)
                  return (
                    <div
                      key={p.id}
                      role="button"
                      tabIndex={0}
                      title={[p.pid && `PID ${p.pid}`, usage && usage.count > 1 && `${usage.count} processes`, p.restarts > 0 && `${p.restarts} restarts`, p.exit_code !== null && `exit ${p.exit_code}`]
                        .filter(Boolean)
                        .join(' · ')}
                      onClick={() => setSelected(p.id)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') setSelected(p.id)
                      }}
                      className={`group flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 text-sm transition-colors ${
                        selected === p.id ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50'
                      } ${live ? '' : 'opacity-60'}`}
                    >
                      <span className={`size-2 shrink-0 rounded-full ${STATE_DOT[p.state] ?? 'bg-muted-foreground'}`} title={p.state} />
                      <span className="min-w-0 flex-1 truncate font-medium">{p.name}</span>
                      {usage?.count ? (
                        <span className="hidden text-xs tabular-nums text-muted-foreground sm:inline">
                          CPU {usage.cpu_percent.toFixed(1)}% · RAM {formatBytes(usage.memory)}
                        </span>
                      ) : (
                        !live && <span className="text-xs text-muted-foreground">{p.state}{p.exit_code !== null ? ` (${p.exit_code})` : ''}</span>
                      )}
                      {(live || stopping === p.id) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="size-6"
                          disabled={stopping === p.id}
                          onClick={(e) => {
                            e.stopPropagation()
                            stopProcess(p.id)
                          }}
                          title="Stop"
                        >
                          {stopping === p.id ? <Spinner className="size-3.5" /> : <StopIcon className="size-3.5" />}
                        </Button>
                      )}
                    </div>
                  )
                })}
            </div>
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

const isLive = (state: ProcessInfo['state']) => state === 'running' || state === 'starting' || state === 'restarting'

const STATE_DOT: Partial<Record<ProcessInfo['state'], string>> = {
  running: 'bg-success',
  starting: 'bg-warning',
  restarting: 'bg-warning',
  stopping: 'bg-warning',
  crashed: 'bg-destructive',
  failed: 'bg-destructive',
}
