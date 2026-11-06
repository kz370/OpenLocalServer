import {
  ArrowDownToLine,
  Copy,
  ExternalLink,
  FileCode2,
  FileText,
  Globe,
  Hammer,
  KeyRound,
  Layers,
  Link2,
  Play,
  Power,
  RotateCw,
  Settings2,
  SquareTerminal,
  Square,
  Wand2,
} from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ConnectDialog } from '@/components/docker/ConnectDialog'
import { CopyId, Row, RowList } from '@/components/docker/DockerRows'
import { ErrorCard } from '@/components/ErrorCard'
import { asDiagnostic } from '@/lib/diagnostic'
import { ServiceIconAction } from '@/components/ServiceIconAction'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { type ComposeAction, type ComposeStack, type Diagnostic, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'

const STATE: Record<string, { label: string; variant: 'success' | 'secondary' | 'warning' | 'destructive' | 'outline' }> = {
  running: { label: '● Running', variant: 'success' },
  paused: { label: 'Paused', variant: 'warning' },
  restarting: { label: 'Restarting…', variant: 'secondary' },
  exited: { label: 'Stopped', variant: 'secondary' },
  created: { label: 'Created', variant: 'outline' },
  dead: { label: 'Dead', variant: 'destructive' },
}

/**
 * §166 Compose tab: compose files in your projects and running stacks. Loads on open and
 * when `reloadKey` changes (after an action finishes); no polling.
 */
export function ComposeTab({
  dockerRunning,
  reloadKey,
  onRunTask,
  onEdit,
  onNew,
  onLogs,
  onCreateSite,
  onSettings,
  onShell,
}: {
  dockerRunning: boolean
  reloadKey: number
  onRunTask: (taskId: string) => void
  onEdit: (path: string) => void
  onNew: () => void
  onLogs: (container: string) => void
  onCreateSite: (container: string) => void
  onSettings: (containerId: string) => void
  onShell: (container: string) => void
}) {
  const [stacks, setStacks] = useState<ComposeStack[] | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)
  const [secretsFor, setSecretsFor] = useState<{ file: string; secrets: [string, string][] } | null>(null)
  const [connectFor, setConnectFor] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const r = await runCommand({ type: 'list_docker_compose_stacks' })
      if (r.type === 'docker_compose_stacks') setStacks(r.stacks)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load, reloadKey])

  async function act(stack: ComposeStack, action: ComposeAction, service?: string) {
    if (!stack.file) return
    if (
      action === 'down' &&
      !(await confirmAction(
        `Stops and removes the containers of "${stack.name}". Volumes (your data) are kept.`,
        'Stop and remove containers?',
        'Down',
        'amber',
      ))
    )
      return
    setError(null)
    try {
      const r = await runCommand({ type: 'docker_compose_action', path: stack.file, action, service: service ?? null })
      if (r.type === 'task_started') onRunTask(r.id)
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  async function showSecrets(file: string) {
    setError(null)
    try {
      const r = await runCommand({ type: 'reveal_docker_compose_secrets', path: file })
      if (r.type === 'docker_compose_secrets') setSecretsFor({ file, secrets: r.secrets })
    } catch (err) {
      setError(asDiagnostic(err))
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} overlay />
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          Compose files found in your projects, and stacks Docker is running.
          {!dockerRunning && ' Docker is stopped: you can still create and edit files.'}
        </p>
        <Button size="sm" className="cursor-pointer" onClick={onNew}>
          <Wand2 className="size-3.5" /> New compose file
        </Button>
      </div>

      {stacks === null ? (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner className="size-4" /> Looking for compose files…
        </p>
      ) : stacks.length === 0 ? (
        <Card>
          <CardContent className="flex flex-col items-center gap-3 py-10 text-center">
            <Layers className="size-8 text-muted-foreground" />
            <p className="text-sm text-muted-foreground">
              No compose files in your projects yet. Create one by picking services like MariaDB, Redis or Mailpit.
            </p>
            <Button size="sm" className="cursor-pointer" onClick={onNew}>
              <Wand2 className="size-3.5" /> New compose file
            </Button>
          </CardContent>
        </Card>
      ) : (
        stacks.map((stack) => {
          const canRun = !!stack.file && dockerRunning && !stack.file_error
          const hasBuild = stack.services.some((s) => s.build)
          return (
            <Card key={`${stack.name}:${stack.file ?? ''}`}>
              <CardHeader className="flex flex-col gap-3 pb-2">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0">
                    <CardTitle className="flex flex-wrap items-center gap-2 text-sm">
                      <Layers className="size-4 shrink-0 text-sky-500" />
                      <span className="min-w-0 break-all">{stack.name}</span>
                      {stack.project_name && <Badge variant="outline">{stack.project_name}</Badge>}
                      <Badge variant={stack.running > 0 ? 'success' : 'secondary'}>
                        {stack.running}/{stack.services.length} running
                      </Badge>
                      {stack.connected && (
                        <Badge variant="outline" className="gap-1" title="Containers can reach your OLS sites and trust OLS's HTTPS certificate">
                          <Link2 className="size-3" /> Connected to OLS
                        </Badge>
                      )}
                    </CardTitle>
                    <p className="mt-1 font-mono text-xs break-all text-muted-foreground">
                      {stack.file ?? 'Started outside OLS; its compose file is not on this computer.'}
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {stack.file && (
                      <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" onClick={() => onEdit(stack.file!)}>
                        <FileCode2 className="size-3.5" /> Edit file
                      </Button>
                    )}
                    {stack.file && (
                      <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" onClick={() => setConnectFor(stack.file!)}>
                        <Link2 className="size-3.5" /> {stack.connected ? 'Connection' : 'Connect to OLS'}
                      </Button>
                    )}
                    {stack.uses_secrets && stack.file && (
                      <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" onClick={() => void showSecrets(stack.file!)}>
                        <KeyRound className="size-3.5" /> Passwords
                      </Button>
                    )}
                  </div>
                </div>
                {stack.file_error && (
                  <p className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs">
                    The file has a problem: {stack.file_error}. Open it with Edit file to fix it.
                  </p>
                )}
                <div className="flex flex-wrap gap-1.5">
                  <Button size="sm" className="h-7 cursor-pointer text-xs" disabled={!canRun} onClick={() => void act(stack, 'up')}>
                    <Play className="size-3.5" /> Up
                  </Button>
                  <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" disabled={!canRun || stack.running === 0} onClick={() => void act(stack, 'stop')}>
                    <Square className="size-3.5" /> Stop
                  </Button>
                  <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" disabled={!canRun} onClick={() => void act(stack, 'restart')}>
                    <RotateCw className="size-3.5" /> Restart
                  </Button>
                  <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" disabled={!canRun} onClick={() => void act(stack, 'down')}>
                    <Power className="size-3.5" /> Down
                  </Button>
                  <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" disabled={!canRun} onClick={() => void act(stack, 'pull')}>
                    <ArrowDownToLine className="size-3.5" /> Pull
                  </Button>
                  {hasBuild && (
                    <Button size="sm" variant="outline" className="h-7 cursor-pointer text-xs" disabled={!canRun} onClick={() => void act(stack, 'build')}>
                      <Hammer className="size-3.5" /> Build
                    </Button>
                  )}
                </div>
              </CardHeader>
              <CardContent className="p-0">
                <RowList first="Service">
                  {stack.services.map((s) => {
                    const st = s.state ? (STATE[s.state] ?? { label: s.state, variant: 'outline' as const }) : null
                    const running = s.state === 'running'
                    const published = s.ports.filter((p) => p.host !== null)
                    return (
                      <Row
                        key={s.name}
                        title={s.name}
                        subtitle={s.image ?? (s.build ? 'built from a Dockerfile' : undefined)}
                        badge={s.container_id ? <CopyId id={s.container_id} /> : undefined}
                        ports={
                          <>
                            {published.length === 0 && <span className="text-xs text-muted-foreground">—</span>}
                            {published.map((p) => (
                              <button
                                key={`${p.host}:${p.container}`}
                                type="button"
                                disabled={!running}
                                title={`Container port ${p.container} → this computer's port ${p.host}`}
                                className="inline-flex cursor-pointer items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-xs tabular-nums hover:bg-muted disabled:cursor-default disabled:opacity-60"
                                onClick={() =>
                                  runCommand({ type: 'open_url', url: `http://localhost:${p.host}` }).catch((e) => setError(asDiagnostic(e)))
                                }
                              >
                                :{p.host}
                                {running && <ExternalLink className="size-3" />}
                              </button>
                            ))}
                          </>
                        }
                        status={st ? <Badge variant={st.variant}>{st.label}</Badge> : <Badge variant="outline">Not created</Badge>}
                        actions={
                          <span className="flex items-center justify-end gap-0.5">
                            <ServiceIconAction
                              title={running ? `Restart ${s.name}` : `Start ${s.name}`}
                              disabled={!canRun}
                              iconColor={running ? 'text-amber-400' : 'text-emerald-400'}
                              onClick={() => void act(stack, running ? 'restart' : 'up', s.name)}
                            >
                              {running ? <RotateCw className="size-3.5" /> : <Play className="size-3.5" />}
                            </ServiceIconAction>
                            <ServiceIconAction
                              title={s.container ? `Logs of ${s.name}` : 'No container yet'}
                              disabled={!s.container}
                              iconColor="text-violet-400"
                              onClick={() => s.container && onLogs(s.container)}
                            >
                              <FileText className="size-3.5" />
                            </ServiceIconAction>
                            <ServiceIconAction
                              title={running ? `Shell in ${s.name}` : 'Start the service to open a shell'}
                              disabled={!running || !s.container}
                              iconColor="text-emerald-400"
                              onClick={() => s.container && onShell(s.container)}
                            >
                              <SquareTerminal className="size-3.5" />
                            </ServiceIconAction>
                            <ServiceIconAction
                              title={s.container && published.length ? `Create a site for ${s.name}` : 'Start the service with a published port first'}
                              disabled={!s.container || !running || published.length === 0}
                              iconColor="text-sky-400"
                              onClick={() => s.container && onCreateSite(s.container)}
                            >
                              <Globe className="size-3.5" />
                            </ServiceIconAction>
                            <ServiceIconAction
                              title={s.container_id ? `Settings of ${s.name}` : 'No container yet'}
                              disabled={!s.container_id}
                              iconColor="text-slate-400"
                              onClick={() => s.container_id && onSettings(s.container_id)}
                            >
                              <Settings2 className="size-3.5" />
                            </ServiceIconAction>
                          </span>
                        }
                      />
                    )
                  })}
                </RowList>
              </CardContent>
            </Card>
          )
        })
      )}

      {connectFor && (
        <ConnectDialog key={connectFor} path={connectFor} onClose={() => setConnectFor(null)} onChanged={() => void load()} />
      )}

      <Dialog
        open={!!secretsFor}
        onClose={() => setSecretsFor(null)}
        title="Stack passwords"
        description="Generated by OLS and kept in the Windows credential store. They are passed to Docker when OLS starts the stack."
      >
        <div className="flex flex-col gap-2">
          {secretsFor?.secrets.map(([name, value]) => (
            <div key={name} className="flex items-center justify-between gap-3 rounded-md border px-3 py-2">
              <span className="min-w-0">
                <span className="block truncate font-mono text-xs text-muted-foreground">{name}</span>
                <span className="block truncate font-mono text-sm">{value}</span>
              </span>
              <Button
                size="sm"
                variant="ghost"
                className="shrink-0 cursor-pointer"
                onClick={() =>
                  void navigator.clipboard.writeText(value).then(() => notify.success(`${name} copied.`, { title: 'Copied' }))
                }
              >
                <Copy className="size-3.5" /> Copy
              </Button>
            </div>
          ))}
        </div>
      </Dialog>
    </div>
  )
}
