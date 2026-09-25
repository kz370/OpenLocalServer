import { listen } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderPlus, FolderSearch, Play, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { type Diagnostic, type Project, type ProjectDetail, type ProcessEvent, runCommand } from '@/core'

const SOURCE_LABEL: Record<string, string> = {
  manifest: 'from .devforge/environment.yaml',
  detected: 'detected from project files',
  global: 'global default',
  none: 'not resolved',
}

export function ProjectsPage() {
  const [projects, setProjects] = useState<Project[]>([])
  const [scanMessage, setScanMessage] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<ProjectDetail | null>(null)
  const [error, setError] = useState<Diagnostic | null>(null)

  const [runOutput, setRunOutput] = useState<string[]>([])
  const [runningProcessId, setRunningProcessId] = useState<number | null>(null)

  async function refreshProjects() {
    const res = await runCommand({ type: 'list_projects' })
    if (res.type === 'projects') setProjects(res.projects)
  }

  useEffect(() => {
    refreshProjects()
  }, [])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    runCommand({ type: 'get_project_detail', id: selectedId }).then((res) => {
      if (res.type === 'project_detail') setDetail(res.detail)
    })
  }, [selectedId])

  useEffect(() => {
    const unlisten = listen<ProcessEvent>('process-event', (event) => {
      const e = event.payload
      if (runningProcessId !== null && e.id === runningProcessId && e.kind === 'output') {
        setRunOutput((prev) => [...prev.slice(-199), e.line])
      }
    })
    return () => {
      unlisten.then((f) => f())
    }
  }, [runningProcessId])

  async function browseAndRegisterOne() {
    const picked = await open({ directory: true, title: 'Select a project folder' })
    if (!picked || Array.isArray(picked)) return
    setError(null)
    setScanMessage(null)
    try {
      const res = await runCommand({ type: 'register_project', path: picked })
      if (res.type === 'project') {
        await refreshProjects()
        setSelectedId(res.project.id)
      }
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function browseAndScanFolder() {
    const picked = await open({ directory: true, title: 'Select a folder containing multiple projects' })
    if (!picked || Array.isArray(picked)) return
    setError(null)
    setScanMessage(null)
    try {
      const res = await runCommand({ type: 'scan_and_register_projects', path: picked })
      if (res.type === 'projects') {
        await refreshProjects()
        setScanMessage(
          res.projects.length === 0
            ? 'No project folders found in there.'
            : `Found and registered ${res.projects.length} project${res.projects.length > 1 ? 's' : ''}.`,
        )
      }
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  async function removeProject(id: string) {
    const name = projects.find((p) => p.id === id)?.name ?? 'this project'
    if (!window.confirm(`Remove ${name} from OpenLocalServer?

Your project files are not deleted.`)) return
    await runCommand({ type: 'remove_project', id })
    if (selectedId === id) setSelectedId(null)
    await refreshProjects()
  }

  async function runResolved(runtimeId: string, args: string[]) {
    if (!selectedId) return
    setError(null)
    setRunOutput([])
    try {
      const res = await runCommand({ type: 'run_in_project', project_id: selectedId, runtime_id: runtimeId, args })
      if (res.type === 'process_started') setRunningProcessId(res.id)
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Projects</h1>
        <p className="text-sm text-muted-foreground">
          Register a folder; DevForge detects its framework and resolves its runtime versions (§18, §42).
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Add projects</CardTitle>
          <CardDescription>
            Pick one project folder, or a workspace folder holding several separate projects side by side.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={browseAndRegisterOne}>
            <FolderPlus /> Add a project
          </Button>
          <Button size="sm" variant="secondary" onClick={browseAndScanFolder}>
            <FolderSearch /> Scan a folder for projects
          </Button>
          {scanMessage && <span className="text-sm text-muted-foreground">{scanMessage}</span>}
        </CardContent>
      </Card>

      <div className="grid gap-4 lg:grid-cols-[1fr_1.6fr]">
        <Card>
          <CardHeader>
            <CardTitle>Registered</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            {projects.length === 0 && <p className="text-sm text-muted-foreground">No projects yet.</p>}
            {projects.map((p) => (
              <div
                key={p.id}
                role="button"
                tabIndex={0}
                onClick={() => setSelectedId(p.id)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') setSelectedId(p.id)
                }}
                className={`flex cursor-pointer items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors ${
                  selectedId === p.id ? 'border-primary bg-accent' : 'border-border hover:bg-accent/50'
                }`}
              >
                <div className="flex flex-col overflow-hidden">
                  <span className="font-medium">{p.name}</span>
                  <span className="truncate text-xs text-muted-foreground">{p.path}</span>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={(e) => {
                    e.stopPropagation()
                    removeProject(p.id)
                  }}
                  title="Remove"
                >
                  <Trash2 className="size-3.5" />
                </Button>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{detail ? detail.project.name : 'Select a project'}</CardTitle>
            {detail && <CardDescription>{detail.detection.framework.replace(/_/g, ' ')}</CardDescription>}
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {!detail && <p className="text-sm text-muted-foreground">Pick a project on the left.</p>}
            {detail && (
              <>
                {detail.detection.markers.length > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {detail.detection.markers.map((m) => (
                      <Badge key={m} variant="secondary">
                        {m}
                      </Badge>
                    ))}
                  </div>
                )}

                <div className="flex flex-col gap-2">
                  {detail.resolved
                    .filter((r) => r.requested_version || r.installed_version)
                    .map((r) => (
                      <div key={r.id} className="flex items-center justify-between rounded-md border border-border px-3 py-2 text-sm">
                        <div className="flex flex-col">
                          <span className="font-medium uppercase">{r.id}</span>
                          <span className="text-xs text-muted-foreground">
                            wants {r.requested_version ?? '?'} · {SOURCE_LABEL[r.source]}
                          </span>
                        </div>
                        {r.installed_version ? (
                          <div className="flex items-center gap-2">
                            <Badge variant="success">{r.installed_version} installed</Badge>
                            <Button size="sm" variant="secondary" onClick={() => runResolved(r.id, ['--version'])}>
                              <Play /> --version
                            </Button>
                          </div>
                        ) : (
                          <Badge variant="destructive">not installed</Badge>
                        )}
                      </div>
                    ))}
                  {detail.resolved.every((r) => !r.requested_version && !r.installed_version) && (
                    <p className="text-sm text-muted-foreground">
                      No runtime requirement detected and no manifest present.
                    </p>
                  )}
                </div>

                {runOutput.length > 0 && (
                  <pre className="max-h-40 overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">
                    {runOutput.join('\n')}
                  </pre>
                )}
              </>
            )}
          </CardContent>
        </Card>
      </div>

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
