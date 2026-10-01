import { open, save } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, Download, Eye, EyeOff, FilePlus2, GitCompare, Plus, Trash2, Upload } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { SaveButton } from '@/components/SaveButton'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Select, Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type EnvDiffRow, type EnvFileInfo, type EnvFileView, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { confirmAction, confirmThen } from '@/lib/confirm'

/** §103: a project's .env files as a table or as raw text, with compare, import and export. */
export function EnvEditor({ projectId }: { projectId: string }) {
  const [files, setFiles] = useState<EnvFileInfo[] | null>(null)
  const [current, setCurrent] = useState<string | null>(null)
  const [view, setView] = useState<EnvFileView | null>(null)
  const [mode, setMode] = useState<'table' | 'raw' | 'compare'>('table')
  const [raw, setRaw] = useState('')
  const [revealed, setRevealed] = useState<Set<string>>(new Set())
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [newKey, setNewKey] = useState('')
  const [newValue, setNewValue] = useState('')
  const [other, setOther] = useState('')
  const [diff, setDiff] = useState<EnvDiffRow[] | null>(null)
  const [newFile, setNewFile] = useState('')
  const action = useAction()
  const { busy, error, setError, run } = action

  const applyView = (v: EnvFileView) => {
    setView(v)
    setRaw(v.content)
    setDrafts({})
  }

  const loadFiles = useCallback(async (pick?: string) => {
    const res = await runCommand({ type: 'list_env_files', project_id: projectId })
    if (res.type !== 'env_files') return
    setFiles(res.files)
    setCurrent((cur) => (pick && res.files.some((f) => f.name === pick) ? pick : cur && res.files.some((f) => f.name === cur) ? cur : (res.files[0]?.name ?? null)))
  }, [projectId])

  useEffect(() => {
    void loadFiles()
  }, [loadFiles])

  useEffect(() => {
    if (!current) {
      setView(null)
      return
    }
    void runCommand({ type: 'read_env_file', project_id: projectId, file: current }).then((r) => r.type === 'env_file' && applyView(r.view))
  }, [current, projectId])

  const send = (cmd: Parameters<typeof runCommand>[0]) =>
    run('env', async () => {
      const r = await runCommand(cmd)
      if (r.type === 'env_file') applyView(r.view)
      await loadFiles(r.type === 'env_file' ? r.view.name : undefined)
    })

  if (!files) return <p className="text-sm text-muted-foreground">Looking for .env files…</p>

  const example = files.find((f) => f.name === '.env.example')
  const dirty = view !== null && raw !== view.content

  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} />

      {files.length === 0 ? (
        <div className="flex flex-col gap-2 text-sm">
          <p className="text-muted-foreground">This project has no .env file yet.</p>
          <div className="flex gap-2">
            {example && (
              <Button size="sm" disabled={busy !== null} onClick={() => send({ type: 'create_env_file', project_id: projectId, file: '.env', from: '.env.example' })}>
                <FilePlus2 /> Create .env from .env.example
              </Button>
            )}
            <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => send({ type: 'create_env_file', project_id: projectId, file: '.env', from: null })}>
              Create empty .env
            </Button>
          </div>
        </div>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Select className="w-52" value={current ?? ''} onChange={(e) => setCurrent(e.target.value)}>
              {files.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.name} ({f.entries})
                </option>
              ))}
            </Select>
            <Input className="h-8 w-40" value={newFile} onChange={(e) => setNewFile(e.target.value)} placeholder=".env.testing" />
            <Button
              size="sm"
              variant="secondary"
              disabled={busy !== null || !newFile.trim()}
              onClick={() => send({ type: 'create_env_file', project_id: projectId, file: newFile.trim(), from: current }).then(() => setNewFile(''))}
              title="Create a new file as a copy of the one selected"
            >
              <FilePlus2 /> New file
            </Button>
            <span className="flex-1" />
            <Button
              size="sm"
              variant="ghost"
              disabled={!current || busy !== null}
              onClick={() =>
                run('import', async () => {
                  const picked = await open({ title: 'Import variables from a file', multiple: false })
                  if (!picked || Array.isArray(picked) || !current) return
                  const replace = await confirmAction(`Replace the whole of ${current} with that file?\n\nChoose Cancel to merge instead: its variables are added to ${current} and the others are kept.`, 'Import mode')
                  const r = await runCommand({ type: 'import_env_file', project_id: projectId, file: current, source: picked, mode: replace ? 'replace' : 'merge' })
                  if (r.type === 'env_file') applyView(r.view)
                  await loadFiles(current)
                })
              }
            >
              <Upload /> Import
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!current || busy !== null}
              onClick={() =>
                run('export', async () => {
                  const dest = await save({ title: 'Export variables', defaultPath: current ?? '.env' })
                  if (dest && current) await runCommand({ type: 'export_env_file', project_id: projectId, file: current, dest })
                })
              }
            >
              <Download /> Export
            </Button>
          </div>

          <Tabs
            tabs={[
              { id: 'table', label: 'Variables', badge: view?.entries.length },
              { id: 'raw', label: 'Raw text' },
              { id: 'compare', label: 'Compare', icon: <GitCompare className="size-3.5" /> },
            ]}
            value={mode}
            onChange={setMode}
          />

          {view && view.issues.length > 0 && (
            <ul className="flex flex-col gap-0.5 rounded-md border border-warning/40 bg-warning/5 p-2 text-xs">
              {view.issues.map((i) => (
                <li key={`${i.line}-${i.message}`} className="flex items-start gap-1.5">
                  <AlertTriangle className={`mt-0.5 size-3 shrink-0 ${i.severity === 'error' ? 'text-destructive' : 'text-warning'}`} />
                  <span>
                    Line {i.line}: {i.message}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {mode === 'table' && view && (
            <div className="flex flex-col">
              <div className="max-h-80 overflow-y-auto rounded-md border border-border">
                {view.entries.length === 0 && <p className="p-3 text-sm text-muted-foreground">No variables in this file.</p>}
                {view.entries.map((e) => {
                  const shown = !e.secret || revealed.has(e.key)
                  const value = drafts[e.key] ?? e.value
                  return (
                    <div key={`${e.line}-${e.key}`} className="group flex items-center gap-2 border-b border-border px-3 py-1 last:border-b-0">
                      <span className="w-56 shrink-0 truncate font-mono text-xs font-medium" title={e.key}>
                        {e.key}
                      </span>
                      <Input
                        className="h-7 flex-1 font-mono text-xs"
                        type={shown ? 'text' : 'password'}
                        value={value}
                        onChange={(ev) => setDrafts((d) => ({ ...d, [e.key]: ev.target.value }))}
                        onBlur={() => {
                          if (drafts[e.key] !== undefined && drafts[e.key] !== e.value) void send({ type: 'set_env_value', project_id: projectId, file: view.name, key: e.key, value: drafts[e.key] })
                        }}
                        onKeyDown={(ev) => ev.key === 'Enter' && (ev.target as HTMLInputElement).blur()}
                      />
                      {e.secret && (
                        <Button
                          size="icon"
                          variant="ghost"
                          className="size-6"
                          title={shown ? 'Hide' : 'Show'}
                          onClick={() =>
                            setRevealed((s) => {
                              const n = new Set(s)
                              if (n.has(e.key)) n.delete(e.key)
                              else n.add(e.key)
                              return n
                            })
                          }
                        >
                          {shown ? <EyeOff className="size-3" /> : <Eye className="size-3" />}
                        </Button>
                      )}
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-6"
                        title="Delete this variable"
                        onClick={() => confirmThen(`Delete ${e.key} from ${view.name}?`, () => send({ type: 'delete_env_key', project_id: projectId, file: view.name, key: e.key }))}
                      >
                        <Trash2 className="size-3" />
                      </Button>
                    </div>
                  )
                })}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <Input className="h-8 w-56 font-mono text-xs" value={newKey} onChange={(e) => setNewKey(e.target.value.toUpperCase())} placeholder="NEW_VARIABLE" />
                <Input className="h-8 flex-1 font-mono text-xs" value={newValue} onChange={(e) => setNewValue(e.target.value)} placeholder="value" />
                <Button
                  size="sm"
                  disabled={busy !== null || !newKey.trim()}
                  onClick={() => send({ type: 'set_env_value', project_id: projectId, file: view.name, key: newKey.trim(), value: newValue }).then(() => {
                    setNewKey('')
                    setNewValue('')
                  })}
                >
                  <Plus /> Add
                </Button>
              </div>
              <p className="mt-2 text-xs text-muted-foreground">Passwords, tokens and keys are hidden until you show them. Each change keeps the file's comments and order; the previous version is saved in OLS's data folder.</p>
            </div>
          )}

          {mode === 'raw' && view && (
            <div className="flex flex-col gap-2">
              <CodeEditor value={raw} onChange={setRaw} language="text" height="320px" />
              <div className="flex items-center gap-2">
                <SaveButton action={action} name="env" size="sm" disabled={!dirty} busyLabel="Saving…" savedLabel="Saved" onClick={() => void send({ type: 'save_env_file', project_id: projectId, file: view.name, content: raw })}>
                  Save
                </SaveButton>
                <Button size="sm" variant="ghost" disabled={!dirty} onClick={() => setRaw(view.content)}>
                  Discard changes
                </Button>
                {dirty && <Badge variant="warning">unsaved</Badge>}
              </div>
            </div>
          )}

          {mode === 'compare' && view && (
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-2">
                <span className="text-sm text-muted-foreground">{view.name} against</span>
                <Select
                  className="w-52"
                  value={other}
                  onChange={(e) => {
                    setOther(e.target.value)
                    if (!e.target.value) return setDiff(null)
                    void run('compare', async () => {
                      const r = await runCommand({ type: 'compare_env_files', project_id: projectId, a: view.name, b: e.target.value })
                      if (r.type === 'env_compare') setDiff(r.rows)
                    })
                  }}
                >
                  <option value="">Pick a file…</option>
                  {files.filter((f) => f.name !== view.name).map((f) => (
                    <option key={f.name} value={f.name}>
                      {f.name}
                    </option>
                  ))}
                </Select>
              </div>
              {diff && (
                <div className="max-h-80 overflow-y-auto rounded-md border border-border text-xs">
                  {diff.filter((r) => r.status !== 'same').length === 0 && <p className="p-3 text-sm text-muted-foreground">The two files hold the same variables and values.</p>}
                  {diff
                    .filter((r) => r.status !== 'same')
                    .map((r) => {
                      const show = (v: string | null) => (v === null ? '—' : r.secret ? '••••••' : v)
                      return (
                        <div key={r.key} className="grid grid-cols-[14rem_1fr_1fr_5rem] items-center gap-2 border-b border-border px-3 py-1 last:border-b-0">
                          <span className="truncate font-mono font-medium">{r.key}</span>
                          <span className="truncate font-mono">{show(r.a)}</span>
                          <span className="truncate font-mono">{show(r.b)}</span>
                          <Badge variant={r.status === 'different' ? 'warning' : 'secondary'}>{r.status === 'different' ? 'differs' : r.status === 'only_a' ? 'only left' : 'only right'}</Badge>
                        </div>
                      )
                    })}
                </div>
              )}
            </div>
          )}
        </>
      )}
    </div>
  )
}
