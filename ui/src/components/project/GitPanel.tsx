import { ArrowDown, ArrowUp, Download, GitBranch, GitCommitHorizontal, KeyRound, Minus, Plus, RefreshCw, Trash2, Undo2, Upload } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Tabs, Textarea, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type GitBranch as Branch, type GitCommit, type GitFile, type GitStatus, runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { timeAgo, useAction } from '@/lib/hooks'
import { cn } from '@/lib/utils'

type Tab = 'changes' | 'history' | 'branches' | 'remotes' | 'stash'

const KIND_MARK: Record<GitFile['kind'], { letter: string; className: string }> = {
  modified: { letter: 'M', className: 'text-warning' },
  added: { letter: 'A', className: 'text-success' },
  deleted: { letter: 'D', className: 'text-destructive' },
  renamed: { letter: 'R', className: 'text-primary' },
  untracked: { letter: 'U', className: 'text-success' },
  conflicted: { letter: '!', className: 'text-destructive' },
}

function DiffText({ text }: { text: string }) {
  return (
    <pre className="max-h-80 overflow-auto rounded-md bg-muted p-2 font-mono text-[11px] leading-relaxed">
      {text.split('\n').map((l, i) => (
        <div
          key={i}
          className={cn(
            'whitespace-pre-wrap break-all',
            l.startsWith('+') && !l.startsWith('+++') && 'bg-success/10 text-success',
            l.startsWith('-') && !l.startsWith('---') && 'bg-destructive/10 text-destructive',
            l.startsWith('@@') && 'text-primary',
          )}
        >
          {l || ' '}
        </div>
      ))}
    </pre>
  )
}

/** §125: the everyday Git work for a project. Not a replacement for a full Git client. */
export function GitPanel({ projectId }: { projectId: string }) {
  const [status, setStatus] = useState<GitStatus | null>(null)
  const [tab, setTab] = useState<Tab>('changes')
  const [output, setOutput] = useState<{ ok: boolean; text: string } | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'git_status', project_id: projectId })
    if (r.type === 'git_status') setStatus(r.status)
  }, [projectId])

  useEffect(() => {
    load().catch(setError)
  }, [load, setError])

  const sync = (action: 'pull' | 'push' | 'fetch') =>
    run(action, async () => {
      const r = await runCommand({ type: 'git_sync', project_id: projectId, action, remote: null })
      if (r.type === 'git_result') setOutput({ ok: r.result.ok, text: r.result.output })
      await load()
    })

  if (!status) return <p className="flex items-center gap-2 text-sm text-muted-foreground"><Spinner /> Reading the repository…</p>

  if (!status.available) {
    return (
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <p className="text-sm">Git was not found on this computer.</p>
        <p className="text-sm text-muted-foreground">Install Git for Windows, or a portable Git that OLS keeps in its own folder.</p>
        <div>
          <Button
            disabled={busy !== null}
            onClick={() =>
              run('install', async () => {
                const cat = await runCommand({ type: 'list_runtime_catalog' })
                const git = cat.type === 'runtime_catalog' ? cat.entries.find((e) => e.id === 'git') : undefined
                if (!git) throw { problem: 'Portable Git is not available for this computer.', cause: 'It is not in the catalog.', fix: null }
                await runCommand({ type: 'install_runtime', id: 'git', version: git.version })
                for (let i = 0; i < 600; i++) {
                  await new Promise((r) => setTimeout(r, 1000))
                  const s = await runCommand({ type: 'git_status', project_id: projectId })
                  if (s.type === 'git_status' && s.status.available) {
                    setStatus(s.status)
                    return
                  }
                }
              })
            }
          >
            {busy === 'install' ? <Spinner /> : <Download />} Install portable Git
          </Button>
        </div>
      </div>
    )
  }

  if (!status.is_repo) {
    return (
      <div className="flex flex-col gap-3">
        <ErrorCard error={error} onDismiss={() => setError(null)} />
        <p className="text-sm">This project is not a Git repository yet.</p>
        <div>
          <Button
            disabled={busy !== null}
            onClick={() =>
              run('init', async () => {
                const r = await runCommand({ type: 'git_init', project_id: projectId })
                if (r.type === 'git_status') setStatus(r.status)
              })
            }
          >
            {busy === 'init' ? <Spinner /> : <GitBranch />} Create a repository here
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm">
          <GitBranch className="size-4 text-muted-foreground" />
          <span className="font-medium">{status.detached ? 'detached HEAD' : status.branch}</span>
          {status.upstream && <span className="text-xs text-muted-foreground">→ {status.upstream}</span>}
          {status.ahead > 0 && (
            <Badge variant="secondary" title="Commits to push">
              <ArrowUp className="size-3" />
              {status.ahead}
            </Badge>
          )}
          {status.behind > 0 && (
            <Badge variant="warning" title="Commits to pull">
              <ArrowDown className="size-3" />
              {status.behind}
            </Badge>
          )}
          {status.operation && <Badge variant="destructive">{status.operation} in progress</Badge>}
        </div>
        <div className="flex flex-wrap gap-1.5">
          <Button size="sm" variant="ghost" onClick={() => run('refresh', load)} title="Refresh">
            <RefreshCw />
          </Button>
          <Button size="sm" variant="secondary" disabled={busy !== null || status.remotes.length === 0} onClick={() => sync('fetch')}>
            {busy === 'fetch' ? <Spinner /> : <RefreshCw />} Fetch
          </Button>
          <Button size="sm" variant="secondary" disabled={busy !== null || status.remotes.length === 0} onClick={() => sync('pull')}>
            {busy === 'pull' ? <Spinner /> : <Download />} Pull
          </Button>
          <Button size="sm" variant="secondary" disabled={busy !== null || status.remotes.length === 0} onClick={() => sync('push')}>
            {busy === 'push' ? <Spinner /> : <Upload />} Push
          </Button>
        </div>
      </div>

      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {output && (
        <div className={cn('rounded-lg border p-2', output.ok ? 'border-border' : 'border-destructive/40 bg-destructive/5')}>
          <div className="flex justify-between gap-2">
            <pre className="whitespace-pre-wrap break-all font-mono text-[11px]">{output.text}</pre>
            <button className="shrink-0 cursor-pointer text-xs text-muted-foreground hover:text-foreground" onClick={() => setOutput(null)}>
              Dismiss
            </button>
          </div>
        </div>
      )}

      <Tabs
        tabs={[
          { id: 'changes', label: 'Changes', badge: status.files.length || undefined },
          { id: 'history', label: 'History' },
          { id: 'branches', label: 'Branches' },
          { id: 'remotes', label: 'Remotes', badge: status.remotes.length || undefined },
          { id: 'stash', label: 'Stash', badge: status.stashes.length || undefined },
        ]}
        value={tab}
        onChange={setTab}
      />

      {tab === 'changes' && <Changes projectId={projectId} status={status} reload={load} />}
      {tab === 'history' && <History projectId={projectId} />}
      {tab === 'branches' && <Branches projectId={projectId} reload={load} />}
      {tab === 'remotes' && <Remotes projectId={projectId} status={status} reload={load} />}
      {tab === 'stash' && <Stash projectId={projectId} status={status} reload={load} />}
    </div>
  )
}

function Changes({ projectId, status, reload }: { projectId: string; status: GitStatus; reload: () => Promise<void> }) {
  const [diff, setDiff] = useState<{ path: string; staged: boolean; text: string } | null>(null)
  const [message, setMessage] = useState('')
  const [amend, setAmend] = useState(false)
  const [ignoreKind, setIgnoreKind] = useState('')
  const { busy, error, setError, run } = useAction()
  const staged = status.files.filter((f) => f.staged)
  const unstaged = status.files.filter((f) => f.unstaged)

  const act = (key: string, fn: () => Promise<unknown>) =>
    run(key, async () => {
      await fn()
      await reload()
    })

  const show = (f: GitFile, isStaged: boolean) =>
    run(`diff:${f.path}`, async () => {
      if (diff?.path === f.path && diff.staged === isStaged) return setDiff(null)
      const r = await runCommand({ type: 'git_diff', project_id: projectId, path: f.path, staged: isStaged })
      if (r.type === 'text') setDiff({ path: f.path, staged: isStaged, text: r.text || '(no text changes)' })
    })

  const list = (files: GitFile[], isStaged: boolean) =>
    files.map((f) => {
      const mark = KIND_MARK[f.kind]
      return (
        <div key={`${isStaged}-${f.path}`}>
          <div className="group flex items-center gap-2 rounded-md px-2 py-1 text-sm hover:bg-accent/50">
            <span className={cn('w-3 font-mono text-xs font-bold', mark.className)} title={f.kind}>
              {mark.letter}
            </span>
            <button className="min-w-0 flex-1 truncate text-left font-mono text-xs" onClick={() => show(f, isStaged)} title={f.from ? `${f.from} → ${f.path}` : f.path}>
              {f.path}
            </button>
            <div className="flex gap-0.5 opacity-60 group-hover:opacity-100">
              {!isStaged && (
                <Button
                  size="icon"
                  variant="ghost"
                  className="size-6"
                  title="Discard changes"
                  onClick={async () => {
                    if (await confirmAction(`Throw away the changes to ${f.path}?${f.untracked ? ' The new file is deleted.' : ''} This can't be undone.`, 'Discard changes'))
                      await act(`x:${f.path}`, () => runCommand({ type: 'git_discard', project_id: projectId, paths: [f.path] }))
                  }}
                >
                  <Undo2 className="size-3.5" />
                </Button>
              )}
              <Button
                size="icon"
                variant="ghost"
                className="size-6"
                title={isStaged ? 'Unstage' : 'Stage'}
                onClick={() => act(`s:${f.path}`, () => runCommand({ type: isStaged ? 'git_unstage' : 'git_stage', project_id: projectId, paths: [f.path] }))}
              >
                {isStaged ? <Minus className="size-3.5" /> : <Plus className="size-3.5" />}
              </Button>
            </div>
          </div>
          {diff && diff.path === f.path && diff.staged === isStaged && <DiffText text={diff.text} />}
        </div>
      )
    })

  return (
    <div className="flex flex-col gap-4">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {status.files.length === 0 && <p className="text-sm text-muted-foreground">No changes. {status.last_commit && `Last commit: ${status.last_commit.subject} (${timeAgo(status.last_commit.time * 1000)}).`}</p>}

      {staged.length > 0 && (
        <div>
          <div className="mb-1 flex items-center justify-between">
            <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Staged · {staged.length}</p>
            <Button size="sm" variant="ghost" className="h-6 text-xs" onClick={() => act('unstage-all', () => runCommand({ type: 'git_unstage', project_id: projectId, paths: [] }))}>
              Unstage all
            </Button>
          </div>
          {list(staged, true)}
        </div>
      )}
      {unstaged.length > 0 && (
        <div>
          <div className="mb-1 flex items-center justify-between">
            <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Changes · {unstaged.length}</p>
            <Button size="sm" variant="ghost" className="h-6 text-xs" onClick={() => act('stage-all', () => runCommand({ type: 'git_stage', project_id: projectId, paths: [] }))}>
              Stage all
            </Button>
          </div>
          {list(unstaged, false)}
        </div>
      )}

      {(staged.length > 0 || amend) && (
        <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
          <Textarea value={message} onChange={(e) => setMessage(e.target.value)} placeholder="Commit message" rows={3} className="font-sans" />
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Toggle checked={amend} onChange={setAmend} label="Amend the last commit" />
              {staged.length > 0 && (
                <AiButton
                  label="Suggest message"
                  variant="secondary"
                  ask={{ title: 'Suggest a commit message', description: 'Reads the staged changes.', request: { feature: 'commit', project_id: projectId }, question: 'none', onCommit: setMessage }}
                />
              )}
            </div>
            <Button
              disabled={busy !== null || (!message.trim() && !amend)}
              onClick={() =>
                act('commit', async () => {
                  await runCommand({ type: 'git_commit', project_id: projectId, message, amend })
                  setMessage('')
                  setAmend(false)
                })
              }
            >
              {busy === 'commit' ? <Spinner /> : <GitCommitHorizontal />} Commit {staged.length > 0 && `${staged.length} file${staged.length === 1 ? '' : 's'}`}
            </Button>
          </div>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-muted-foreground">{status.has_gitignore ? 'Add to .gitignore:' : 'No .gitignore yet. Start one for:'}</span>
        <Select value={ignoreKind} onChange={(e) => setIgnoreKind(e.target.value)} className="h-8 w-40" aria-label=".gitignore template">
          <option value="">Choose…</option>
          <option value="laravel">Laravel</option>
          <option value="php">PHP</option>
          <option value="wordpress">WordPress</option>
          <option value="node">Node</option>
          <option value="python">Python</option>
          <option value="editor">Editors and OS files</option>
        </Select>
        <Button
          size="sm"
          variant="secondary"
          disabled={!ignoreKind || busy !== null}
          onClick={() =>
            act('ignore', async () => {
              await runCommand({ type: 'git_add_ignore', project_id: projectId, template: ignoreKind })
              setIgnoreKind('')
            })
          }
        >
          Add
        </Button>
      </div>
    </div>
  )
}

function History({ projectId }: { projectId: string }) {
  const [commits, setCommits] = useState<GitCommit[] | null>(null)
  const [shown, setShown] = useState<{ hash: string; text: string } | null>(null)
  const { error, setError, run } = useAction()
  useEffect(() => {
    runCommand({ type: 'git_log', project_id: projectId, limit: 100 })
      .then((r) => r.type === 'git_commits' && setCommits(r.commits))
      .catch(setError)
  }, [projectId, setError])
  if (!commits) return <Spinner />
  return (
    <div className="flex flex-col gap-1">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {commits.length === 0 && <p className="text-sm text-muted-foreground">No commits yet.</p>}
      {commits.map((c) => (
        <div key={c.hash}>
          <button
            className="flex w-full items-baseline gap-3 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent/50"
            onClick={() =>
              run(c.hash, async () => {
                if (shown?.hash === c.hash) return setShown(null)
                const r = await runCommand({ type: 'git_show', project_id: projectId, hash: c.hash })
                if (r.type === 'text') setShown({ hash: c.hash, text: r.text })
              })
            }
          >
            <span className="shrink-0 font-mono text-xs text-muted-foreground">{c.short}</span>
            <span className="min-w-0 flex-1 truncate">{c.subject}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {c.author} · {timeAgo(c.time * 1000)}
            </span>
          </button>
          {shown?.hash === c.hash && <DiffText text={shown.text} />}
        </div>
      ))}
    </div>
  )
}

function Branches({ projectId, reload }: { projectId: string; reload: () => Promise<void> }) {
  const [branches, setBranches] = useState<Branch[]>([])
  const [name, setName] = useState('')
  const { busy, error, setError, run } = useAction()
  const load = useCallback(async () => {
    const r = await runCommand({ type: 'git_branches', project_id: projectId })
    if (r.type === 'git_branches') setBranches(r.branches)
  }, [projectId])
  useEffect(() => {
    load().catch(setError)
  }, [load, setError])
  const act = (key: string, fn: () => Promise<unknown>) =>
    run(key, async () => {
      await fn()
      await load()
      await reload()
    })

  const row = (b: Branch) => (
    <div key={`${b.remote}-${b.name}`} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent/50">
      <GitBranch className={cn('size-3.5', b.current ? 'text-primary' : 'text-muted-foreground')} />
      <span className={cn('font-mono text-xs', b.current && 'font-semibold')}>{b.name}</span>
      {b.current && <Badge variant="success">current</Badge>}
      <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{b.subject}</span>
      {!b.current && (
        <Button size="sm" variant="ghost" className="h-6 text-xs" disabled={busy !== null} onClick={() => act(`sw:${b.name}`, () => runCommand({ type: 'git_switch_branch', project_id: projectId, name: b.name }))}>
          Switch
        </Button>
      )}
      {!b.current && !b.remote && (
        <Button
          size="icon"
          variant="ghost"
          className="size-6"
          title="Delete branch"
          onClick={async () => {
            if (!(await confirmAction(`Delete the branch ${b.name}?`))) return
            await act(`del:${b.name}`, async () => {
              try {
                await runCommand({ type: 'git_delete_branch', project_id: projectId, name: b.name, force: false })
              } catch (e) {
                if (await confirmAction(`${b.name} has commits that are not merged anywhere. Delete it anyway? Those commits will be lost.`, 'Force delete')) {
                  await runCommand({ type: 'git_delete_branch', project_id: projectId, name: b.name, force: true })
                } else throw e
              }
            })
          }}
        >
          <Trash2 className="size-3.5" />
        </Button>
      )}
    </div>
  )

  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex gap-2">
        <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="new-branch-name" className="h-8 max-w-64 font-mono" />
        <Button
          size="sm"
          disabled={!name.trim() || busy !== null}
          onClick={() =>
            act('create', async () => {
              await runCommand({ type: 'git_create_branch', project_id: projectId, name: name.trim(), checkout: true })
              setName('')
            })
          }
        >
          <Plus /> Create and switch
        </Button>
      </div>
      <div>
        <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">Local</p>
        {branches.filter((b) => !b.remote).map(row)}
      </div>
      {branches.some((b) => b.remote) && (
        <div>
          <p className="mb-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">Remote</p>
          {branches.filter((b) => b.remote).map(row)}
        </div>
      )}
    </div>
  )
}

function Remotes({ projectId, status, reload }: { projectId: string; status: GitStatus; reload: () => Promise<void> }) {
  const [name, setName] = useState('origin')
  const [url, setUrl] = useState('')
  const [creds, setCreds] = useState<{ host: string; username: string; token: string } | null>(null)
  const { busy, error, setError, run } = useAction()
  const act = (key: string, fn: () => Promise<unknown>) =>
    run(key, async () => {
      await fn()
      await reload()
    })
  const hostOf = (u: string) => u.match(/^https?:\/\/(?:[^@/]+@)?([^/]+)/)?.[1]?.toLowerCase() ?? null

  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      {status.remotes.map((r) => {
        const host = hostOf(r.url)
        return (
          <div key={r.name} className="flex flex-wrap items-center gap-2 rounded-lg border border-border px-3 py-2 text-sm">
            <span className="font-medium">{r.name}</span>
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">{r.url}</span>
            {host && (
              <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => setCreds({ host, username: '', token: '' })}>
                <KeyRound className="size-3.5" /> {r.has_credentials ? 'Credentials saved' : 'Add credentials'}
              </Button>
            )}
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              title="Remove remote"
              onClick={async () => {
                if (await confirmAction(`Remove the remote ${r.name}? The repository on the server is not touched.`)) await act(`rm:${r.name}`, () => runCommand({ type: 'git_remove_remote', project_id: projectId, name: r.name }))
              }}
            >
              <Trash2 className="size-3.5" />
            </Button>
          </div>
        )
      })}
      <div className="flex flex-wrap gap-2">
        <Input value={name} onChange={(e) => setName(e.target.value)} className="h-8 w-28" aria-label="Remote name" />
        <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://github.com/you/repo.git" className="h-8 min-w-0 flex-1 font-mono" aria-label="Remote address" />
        <Button
          size="sm"
          disabled={!name.trim() || !url.trim() || busy !== null}
          onClick={() =>
            act('add', async () => {
              await runCommand({ type: 'git_add_remote', project_id: projectId, name: name.trim(), url: url.trim() })
              setUrl('')
            })
          }
        >
          <Plus /> Add remote
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">SSH remotes use your SSH keys. For HTTPS remotes, save a username and a personal access token; they're kept in Windows Credential Manager.</p>

      <Dialog
        open={creds !== null}
        onClose={() => setCreds(null)}
        title={`Credentials for ${creds?.host ?? ''}`}
        description="Used for pull, push, fetch and clone over HTTPS. Use an access token, not your account password."
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() =>
                act('forget', async () => {
                  if (creds) await runCommand({ type: 'git_set_credentials', host: creds.host, username: '', token: null })
                  setCreds(null)
                })
              }
            >
              Forget
            </Button>
            <Button
              disabled={!creds?.token}
              onClick={() =>
                act('creds', async () => {
                  if (creds) await runCommand({ type: 'git_set_credentials', host: creds.host, username: creds.username, token: creds.token })
                  setCreds(null)
                })
              }
            >
              Save
            </Button>
          </>
        }
      >
        {creds && (
          <div className="flex flex-col gap-3">
            <Field label="Username">
              <Input value={creds.username} onChange={(e) => setCreds({ ...creds, username: e.target.value })} autoComplete="off" />
            </Field>
            <Field label="Access token">
              <Input type="password" value={creds.token} onChange={(e) => setCreds({ ...creds, token: e.target.value })} autoComplete="off" />
            </Field>
          </div>
        )}
      </Dialog>
    </div>
  )
}

function Stash({ projectId, status, reload }: { projectId: string; status: GitStatus; reload: () => Promise<void> }) {
  const [message, setMessage] = useState('')
  const [out, setOut] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()
  const stash = (action: 'push' | 'pop' | 'apply' | 'drop', index: number | null) =>
    run(`${action}:${index}`, async () => {
      const r = await runCommand({ type: 'git_stash', project_id: projectId, action, message: action === 'push' ? message : null, index })
      if (r.type === 'git_result') setOut(r.result.output)
      if (action === 'push') setMessage('')
      await reload()
    })
  return (
    <div className="flex flex-col gap-3">
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <div className="flex gap-2">
        <Input value={message} onChange={(e) => setMessage(e.target.value)} placeholder="What you were doing (optional)" className="h-8 min-w-0 flex-1" />
        <Button size="sm" disabled={busy !== null || status.files.length === 0} onClick={() => stash('push', null)}>
          Stash changes
        </Button>
      </div>
      {status.stashes.length === 0 && <p className="text-sm text-muted-foreground">No stashed changes.</p>}
      {status.stashes.map((s, i) => (
        <div key={s} className="flex items-center gap-2 rounded-md border border-border px-3 py-1.5 text-sm">
          <span className="min-w-0 flex-1 truncate font-mono text-xs">{s}</span>
          <Button size="sm" variant="ghost" className="h-6 text-xs" onClick={() => stash('pop', i)} disabled={busy !== null}>
            Pop
          </Button>
          <Button size="sm" variant="ghost" className="h-6 text-xs" onClick={() => stash('apply', i)} disabled={busy !== null}>
            Apply
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="size-6"
            title="Drop"
            onClick={async () => {
              if (await confirmAction('Drop this stash? Its changes are lost.')) await stash('drop', i)
            }}
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      ))}
      {out && <pre className="whitespace-pre-wrap rounded-md bg-muted p-2 font-mono text-[11px]">{out}</pre>}
    </div>
  )
}
