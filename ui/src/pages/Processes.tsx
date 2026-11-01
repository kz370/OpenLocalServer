import { listen } from '@tauri-apps/api/event'
import { ChevronRight, Play } from 'lucide-react'
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
  { label: 'echo hello', executable: 'cmd', args: '/C echo hello from OLS' },
  { label: 'exit 1 (crash demo)', executable: 'cmd', args: '/C exit 1' },
]

export function ProcessesPage() {
  const [processes, setProcesses] = useState<ProcessInfo[]>([])
  const [outputs, setOutputs] = useState<Record<number, string[]>>({})
  const [selected, setSelected] = useState<number | null>(null)
  const [stopping, setStopping] = useState<number | null>(null)
  const [expanded, setExpanded] = useState<string[]>([])

  const [name, setName] = useState('demo')
  const [executable, setExecutable] = useState('ping')
  const [args, setArgs] = useState('127.0.0.1 -n 10')
  const [starting, setStarting] = useState(false)

  const [port, setPort] = useState('3306')
  const [portResult, setPortResult] = useState<PortStatus | null>(null)
  const [checkingPort, setCheckingPort] = useState(false)
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
    if (!executable.trim()) return
    setError(null)
    setStarting(true)
    try {
      const res = await runCommand({
        type: 'start_process',
        spec: {
          name: name.trim() || executable.trim(),
          executable: executable.trim(),
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
    } finally {
      setStarting(false)
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
    const p = Number.parseInt(port.trim(), 10)
    if (isNaN(p) || p < 1 || p > 65535) {
      setError({
        problem: 'Invalid port number',
        cause: `"${port}" is not a valid TCP port number (must be between 1 and 65535).`,
        fix: 'Enter a valid port number, such as 80, 443, or 3306.',
      })
      return
    }
    setError(null)
    setCheckingPort(true)
    try {
      const res = await runCommand({ type: 'check_port', port: p })
      if (res.type === 'port') setPortResult(res.status)
    } catch (err) {
      setError(err as Diagnostic)
    } finally {
      setCheckingPort(false)
    }
  }

  const selectedOutput = selected !== null ? (outputs[selected] ?? []) : []

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Processes</h1>
        <p className="text-sm text-muted-foreground">
          The services the app supervises — PHP, Nginx, MariaDB and the rest — and the processes it runs for you.
          Background work has its own{' '}
          <a href="#/tasks" className="underline underline-offset-2 hover:text-foreground">
            Tasks page
          </a>
          .
        </p>
      </div>

      <SystemMonitor stats={stats} />

      <>
      <Card>
        <CardHeader>
          <CardTitle>Start a managed process</CardTitle>
          <CardDescription>Runs through the same supervisor that runs PHP, Nginx, MariaDB, etc.</CardDescription>
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
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void startProcess()
              }}
              placeholder="name"
              className="w-32"
            />
            <Input
              value={executable}
              onChange={(e) => setExecutable(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void startProcess()
              }}
              placeholder="executable"
              className="w-32"
            />
            <Input
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void startProcess()
              }}
              placeholder="args (space separated)"
              className="flex-1 min-w-40"
            />
            <Button onClick={startProcess} size="sm" disabled={starting || !executable.trim()}>
              {starting ? <Spinner className="size-3.5" /> : <Play />} Start
            </Button>
          </div>
        </CardContent>
      </Card>

      <div className="flex flex-col gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm">Running &amp; recent · {processes.filter((p) => isLive(p.state)).length} running</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {processes.length === 0 && (
              <p className="text-sm text-muted-foreground">No processes started yet.</p>
            )}
            <div className="max-h-96 overflow-y-auto">
              {processes.length > 0 && (
                // Inside the scroll area (sticky) so a scrollbar can't shift the columns.
                <div className="sticky top-0 z-10 flex h-6 items-center gap-2 bg-card px-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  <span className="flex-1">Process</span>
                  <span className="w-14 text-right">CPU</span>
                  <span className="w-20 text-right">RAM</span>
                  <span className="w-6" />
                </div>
              )}
              {groupProcesses(processes).map((g) => {
                const open = expanded.includes(g.key)
                const many = g.members.length > 1
                const lead = g.members[0]
                const usage = g.members.reduce(
                  (acc, p) => {
                    const u = p.pid ? stats?.processes[p.pid] : undefined
                    return u ? { cpu: acc.cpu + u.cpu_percent, mem: acc.mem + u.memory, seen: true } : acc
                  },
                  { cpu: 0, mem: 0, seen: false },
                )
                const rows = many && open ? g.members : []
                return (
                  <div key={g.key}>
                    <ProcessRow
                      name={g.name}
                      count={many ? g.members.length : undefined}
                      open={open}
                      state={lead.state}
                      selected={!many && selected === lead.id}
                      usage={usage.seen ? usage : undefined}
                      detail={many ? undefined : detailOf(lead, stats?.processes[lead.pid ?? -1]?.count)}
                      stopping={g.members.some((p) => stopping === p.id)}
                      canStop={g.members.some((p) => isLive(p.state))}
                      onClick={() => (many ? setExpanded(open ? expanded.filter((k) => k !== g.key) : [...expanded, g.key]) : setSelected(lead.id))}
                      onStop={() => g.members.filter((p) => isLive(p.state)).forEach((p) => void stopProcess(p.id))}
                    />
                    {rows.map((p) => {
                      const u = p.pid ? stats?.processes[p.pid] : undefined
                      return (
                        <ProcessRow
                          key={p.id}
                          name={p.name.slice(g.name.length).trim() || p.name}
                          nested
                          state={p.state}
                          selected={selected === p.id}
                          usage={u ? { cpu: u.cpu_percent, mem: u.memory } : undefined}
                          detail={detailOf(p, u?.count)}
                          stopping={stopping === p.id}
                          canStop={isLive(p.state)}
                          onClick={() => setSelected(p.id)}
                          onStop={() => void stopProcess(p.id)}
                        />
                      )
                    })}
                  </div>
                )
              })}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle className="text-sm">
              Live output {selected !== null && `· ${processes.find((p) => p.id === selected)?.name ?? `#${selected}`}`}
            </CardTitle>
            {selected !== null && selectedOutput.length > 0 && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 text-xs"
                onClick={() => setOutputs((prev) => ({ ...prev, [selected]: [] }))}
              >
                Clear output
              </Button>
            )}
          </CardHeader>
          <CardContent>
            <pre className="h-64 overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">
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
          <CardDescription>Reports the owning process and PID instead of guessing; never terminates anything.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <Input
              value={port}
              onChange={(e) => {
                setPort(e.target.value)
                setPortResult(null)
                setError(null)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void checkPort()
              }}
              placeholder="Port (e.g. 3306)"
              className="w-28"
            />
            <Button onClick={checkPort} size="sm" variant="secondary" disabled={checkingPort || !port.trim()}>
              {checkingPort ? <Spinner className="size-3.5" /> : null}
              {checkingPort ? 'Checking...' : 'Check'}
            </Button>
            {portResult && (
              <span className="text-sm flex items-center gap-2">
                {portResult.status === 'free' ? (
                  <>
                    <Badge variant="success">Free</Badge>
                    <span className="text-muted-foreground">Port is available.</span>
                  </>
                ) : (
                  <>
                    <Badge variant="destructive">In use</Badge>
                    <span className="text-muted-foreground">
                      {portResult.process_name
                        ? portResult.pid
                          ? `${portResult.process_name} (PID ${portResult.pid})`
                          : portResult.process_name
                        : portResult.pid
                          ? `PID ${portResult.pid}`
                          : 'In use by another application (process details unavailable)'}
                    </span>
                  </>
                )}
              </span>
            )}
          </div>
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
        </>
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

interface Group {
  key: string
  name: string
  members: ProcessInfo[]
}

/**
 * Worker pools ("PHP 8.4.26 FastCGI :10840", ":10841", ...) collapse into one row. Running
 * groups come first, then finished ones, newest first.
 */
function groupProcesses(list: ProcessInfo[]): Group[] {
  const sorted = [...list].sort((a, b) => Number(isLive(b.state)) - Number(isLive(a.state)) || b.id - a.id)
  const groups: Group[] = []
  for (const p of sorted) {
    const base = p.name.replace(/\s+:\d+$/, '')
    const key = `${base}|${isLive(p.state)}`
    const g = base !== p.name ? groups.find((x) => x.key === key) : undefined
    if (g) g.members.push(p)
    else groups.push({ key: base !== p.name ? key : `${p.id}`, name: base, members: [p] })
  }
  return groups
}

function detailOf(p: ProcessInfo, children?: number): string {
  return [p.pid && `PID ${p.pid}`, children && children > 1 && `${children} processes`, p.restarts > 0 && `${p.restarts} restarts`, p.exit_code !== null && `exit ${p.exit_code}`]
    .filter(Boolean)
    .join(' · ')
}

function ProcessRow(props: {
  name: string
  count?: number
  open?: boolean
  nested?: boolean
  state: ProcessInfo['state']
  selected: boolean
  usage?: { cpu: number; mem: number }
  detail?: string
  stopping: boolean
  canStop: boolean
  onClick: () => void
  onStop: () => void
}) {
  const live = isLive(props.state)
  return (
    <div
      role="button"
      tabIndex={0}
      title={props.detail}
      onClick={props.onClick}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') props.onClick()
      }}
      className={`flex h-8 cursor-pointer items-center gap-2 rounded-md px-2 text-sm transition-colors ${
        props.selected ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50'
      } ${live ? '' : 'opacity-60'} ${props.nested ? 'pl-8' : ''}`}
    >
      {props.count !== undefined ? (
        <ChevronRight className={`size-3.5 shrink-0 text-muted-foreground transition-transform ${props.open ? 'rotate-90' : ''}`} />
      ) : (
        <span className={`mx-[3px] size-2 shrink-0 rounded-full ${STATE_DOT[props.state] ?? 'bg-muted-foreground'}`} title={props.state} />
      )}
      <span className={`min-w-0 flex-1 truncate ${props.nested ? 'text-muted-foreground' : 'font-medium'}`}>
        {props.name}
        {props.count !== undefined && <span className="ml-1.5 rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground">×{props.count}</span>}
        {!live && <span className="ml-2 text-xs font-normal text-muted-foreground">{props.state}</span>}
      </span>
      <span className="w-14 text-right text-xs tabular-nums text-muted-foreground">{props.usage ? `${props.usage.cpu.toFixed(1)}%` : ''}</span>
      <span className="w-20 text-right text-xs tabular-nums">{props.usage ? formatBytes(props.usage.mem) : ''}</span>
      <span className="flex w-6 justify-end">
        {(props.canStop || props.stopping) && (
          <Button
            variant="ghost"
            size="icon"
            className="size-6"
            disabled={props.stopping}
            onClick={(e) => {
              e.stopPropagation()
              props.onStop()
            }}
            title={props.count ? `Stop all ${props.count}` : 'Stop'}
          >
            {props.stopping ? <Spinner className="size-3.5" /> : <StopIcon className="size-3.5" />}
          </Button>
        )}
      </span>
    </div>
  )
}
