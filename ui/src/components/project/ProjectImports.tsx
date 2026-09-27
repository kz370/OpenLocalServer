import { open } from '@tauri-apps/plugin-dialog'
import { FileKey, FileUp, GitBranch } from 'lucide-react'
import { useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CloneResult, type ImportPreview, type Project, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'

function FolderInput({ value, onChange, title }: { value: string; onChange: (v: string) => void; title: string }) {
  return (
    <div className="flex gap-2">
      <Input value={value} onChange={(e) => onChange(e.target.value)} className="min-w-0 flex-1 font-mono text-xs" />
      <Button
        variant="secondary"
        onClick={async () => {
          const p = await open({ directory: true, title })
          if (p && !Array.isArray(p)) onChange(p)
        }}
      >
        Browse
      </Button>
    </div>
  )
}

/** §125: clone a repository into a new folder, registered as a project with its own site. */
export function GitCloneButton({ defaultParent, onDone }: { defaultParent: string; onDone: (p: Project) => void }) {
  const [openDialog, setOpen] = useState(false)
  const [url, setUrl] = useState('')
  const [branch, setBranch] = useState('')
  const [target, setTarget] = useState('')
  const [authMode, setAuthMode] = useState<'none' | 'https' | 'ssh'>('none')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(false)
  const [keyPath, setKeyPath] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const { busy, error, setError, run } = useAction()
  const repoName = url.trim().replace(/\.git$/, '').split(/[/:]/).pop() ?? ''
  const suggested = repoName && defaultParent ? `${defaultParent.replace(/[\\/]+$/, '')}\\${repoName}` : ''
  const resolved = target || suggested

  return (
    <>
      <Button size="sm" variant="secondary" onClick={() => setOpen(true)} title="Clone a Git repository into a new project">
        <GitBranch /> Clone from Git
      </Button>
      <Dialog
        open={openDialog}
        onClose={() => busy === null && setOpen(false)}
        title="Clone a repository"
        description="Clones into a new folder and adds it as a project. You can use saved HTTPS credentials or enter credentials for this clone."
        footer={
          <>
            <Button variant="ghost" onClick={() => { setPassword(''); setPassphrase(''); setOpen(false) }} disabled={busy !== null}>
              Cancel
            </Button>
            <Button
              disabled={busy !== null || !url.trim() || !resolved || (authMode === 'https' && (!username.trim() || !password))}
              onClick={() =>
                run('clone', async () => {
                  const dest = resolved
                  const auth = authMode === 'https'
                    ? { type: 'https' as const, username, password, remember }
                    : authMode === 'ssh'
                      ? { type: 'ssh' as const, key_path: keyPath, remember, passphrase: passphrase || null }
                      : null
                  const r = await runCommand({ type: 'git_clone', url: url.trim(), target: dest, branch: branch.trim() || null, auth })
                  if (r.type === 'project') {
                    setOpen(false)
                    setUrl('')
                    setTarget('')
                    setPassword('')
                    setPassphrase('')
                    onDone(r.project)
                  }
                })
              }
            >
              {busy ? <Spinner /> : <GitBranch />} {busy ? 'Cloning…' : 'Clone'}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <ErrorCard error={error} onDismiss={() => setError(null)} />
          <Field label="Repository address">
            <Input value={url} onChange={(e) => {
              const value = e.target.value
              setUrl(value)
              if (/^(git@|ssh:\/\/)/i.test(value.trim())) setAuthMode('ssh')
            }} placeholder="https://github.com/you/shop.git" className="font-mono" autoFocus />
          </Field>
          <Field label="Authentication" hint="Saved HTTPS credentials are used automatically when available.">
            <Select value={authMode} onChange={(e) => setAuthMode(e.target.value as typeof authMode)}>
              <option value="none">None / saved credentials</option>
              <option value="https">Username + password or token</option>
              <option value="ssh">SSH key</option>
            </Select>
          </Field>
          {authMode === 'https' && <>
            <Field label="Username"><Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" /></Field>
            <Field label="Password or access token"><Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" /></Field>
            <Toggle checked={remember} onChange={setRemember} label="Remember for this host" hint="Stores the username and token in the system credential store." />
          </>}
          {authMode === 'ssh' && <>
            <Field label="Private key file" hint="Defaults to ~/.ssh/id_ed25519 when present.">
              <div className="flex gap-2">
                <Input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} className="min-w-0 flex-1 font-mono text-xs" placeholder="~/.ssh/id_ed25519" />
                <Button variant="secondary" onClick={async () => {
                  const p = await open({ multiple: false, title: 'Choose SSH private key' })
                  if (p && !Array.isArray(p)) setKeyPath(p)
                }}><FileKey /> Browse</Button>
              </div>
            </Field>
            <Field label="Key passphrase" hint="Used for this clone only; it is not saved."><Input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="new-password" /></Field>
            <Toggle checked={remember} onChange={setRemember} label="Remember SSH key for this host" hint="Applies this key to later Git pull and push operations. Any passphrase is stored securely in the system credential store." />
          </>}
          <Field label="Branch" hint="Blank for the repository's default branch.">
            <Input value={branch} onChange={(e) => setBranch(e.target.value)} className="font-mono" />
          </Field>
          <Field label="Folder" hint={resolved ? `Will clone into ${resolved}. Edit name or Browse different location.` : 'A new or empty folder inside <install>\\sites by default.'}>
            <FolderInput value={target} onChange={setTarget} title="Folder to clone into" />
            {!target && suggested ? (
              <p className="mt-1 truncate font-mono text-xs text-muted-foreground" title={suggested}>
                Default: {suggested}
              </p>
            ) : null}
          </Field>
        </div>
      </Dialog>
    </>
  )
}

/** §132: create a project from an exported environment file, after reviewing it. */
export function ImportEnvironmentButton({ defaultParent, onDone }: { defaultParent: string; onDone: (p: Project) => void }) {
  const [preview, setPreview] = useState<ImportPreview | null>(null)
  const [name, setName] = useState('')
  const [target, setTarget] = useState('')
  const [result, setResult] = useState<CloneResult | null>(null)
  const { busy, error, setError, run } = useAction()

  async function pick() {
    const source = await open({ multiple: false, title: 'Import an environment', filters: [{ name: 'Environment', extensions: ['zip'] }] })
    if (!source || Array.isArray(source)) return
    await run('preview', async () => {
      const r = await runCommand({ type: 'preview_import', source })
      if (r.type === 'import_preview') {
        setPreview(r.preview)
        setName(r.preview.suggested_name)
        setTarget(defaultParent ? `${defaultParent}\\${r.preview.suggested_name}` : '')
        setResult(null)
      }
    })
  }

  return (
    <>
      <Button size="sm" variant="secondary" onClick={pick} title="Create a project from an exported environment file">
        {busy === 'preview' ? <Spinner /> : <FileUp />} Import environment
      </Button>
      {error && !preview && <ErrorCard error={error} onDismiss={() => setError(null)} />}
      <Dialog
        open={preview !== null}
        onClose={() => busy === null && setPreview(null)}
        title={`Import ${preview?.content.project.name ?? ''}`}
        description="Review what the file contains. Nothing is created until you import it."
        footer={
          result ? (
            <Button onClick={() => { onDone(result.project); setPreview(null) }}>Open project</Button>
          ) : (
            <>
              <Button variant="ghost" onClick={() => setPreview(null)} disabled={busy !== null}>
                Cancel
              </Button>
              <Button
                disabled={busy !== null || !name.trim() || !target.trim()}
                onClick={() =>
                  run('import', async () => {
                    if (!preview) return
                    const r = await runCommand({ type: 'import_environment', source: preview.source, target, name: name.trim() })
                    if (r.type === 'cloned') setResult(r.result)
                  })
                }
              >
                {busy === 'import' ? <Spinner /> : <FileUp />} Import
              </Button>
            </>
          )
        }
      >
        {preview && (
          <div className="flex flex-col gap-4 text-sm">
            <ErrorCard error={error} onDismiss={() => setError(null)} />
            {!result ? (
              <>
                <div className="rounded-lg border border-border p-3">
                  <p className="font-medium">Contains</p>
                  {preview.summary.map((s) => (
                    <p key={s} className="text-xs text-muted-foreground">
                      • {s}
                    </p>
                  ))}
                  {preview.content.file_count === 0 && <p className="text-xs text-warning">No project files: the folder must already hold the project, or you add them after.</p>}
                </div>
                {preview.conflicts.length > 0 && (
                  <div className="rounded-lg border border-warning/40 bg-warning/10 p-3 text-xs">
                    {preview.conflicts.map((c) => (
                      <p key={c}>⚠ {c}</p>
                    ))}
                  </div>
                )}
                <Field label="Project name" hint={preview.adjustments.length ? `Adjusted: ${preview.adjustments.join(', ')}` : undefined}>
                  <Input value={name} onChange={(e) => {
                    const next = e.target.value
                    setName(next)
                    const clean = next.trim().replace(/[\\/:*?"<>|]+/g, '-')
                    if (clean && defaultParent) {
                      const base = target.replace(/[\\/][^\\/]+$/, '')
                      if (!base || base.toLowerCase() === defaultParent.toLowerCase()) {
                        setTarget(`${defaultParent.replace(/[\\/]+$/, '')}\\${clean}`)
                      }
                    }
                  }} />
                </Field>
                <Field label="Folder" hint="Defaults to <install>\sites. Edit folder name or Browse different location.">
                  <FolderInput value={target} onChange={setTarget} title="Folder for the project" />
                </Field>
              </>
            ) : (
              <div className="flex flex-col gap-1">
                <p className="font-medium text-success">{result.project.name} was imported.</p>
                {result.changes.map((c) => (
                  <p key={c} className="text-xs text-muted-foreground">
                    ✓ {c}
                  </p>
                ))}
                {result.problems.map((p) => (
                  <p key={p} className="text-xs text-warning">
                    ⚠ {p}
                  </p>
                ))}
                <p className="text-xs text-muted-foreground">Next: open its Environment tab and apply the plan to install and start what it needs.</p>
              </div>
            )}
          </div>
        )}
      </Dialog>
    </>
  )
}
