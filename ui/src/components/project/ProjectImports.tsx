import { open } from '@tauri-apps/plugin-dialog'
import { FileUp, GitBranch, Info, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type CloneResult, type ImportPreview, type Project, runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { buildSitePath, domainToFolderName, folderNameToDomain, getDefaultSitesDir } from '@/lib/sites'

const BROWSE_SENTINEL = '__browse__'

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

/**
 * Dropdown that shows SSH private keys detected from `~/.ssh`, plus a "Browse" fallback.
 * A refresh button lets the user re-scan without closing the dialog.
 * A hint explains where to add new keys and how to use the refresh button.
 */
function SshKeyPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [keys, setKeys] = useState<string[]>([])
  const [loading, setLoading] = useState(false)

  const loadKeys = useCallback(async () => {
    setLoading(true)
    try {
      const r = await runCommand({ type: 'list_ssh_keys' })
      if (r.type === 'ssh_keys') {
        setKeys(r.keys)
        if (!value && r.keys.length > 0) {
          const defaultKey = r.keys.find((k) => /[\\/](id_ed25519|id_rsa)$/i.test(k)) ?? r.keys[0]
          if (defaultKey) onChange(defaultKey)
        }
      }
    } catch {
      // Best-effort; fall back to manual entry.
    } finally {
      setLoading(false)
    }
  }, [value, onChange])

  useEffect(() => {
    void loadKeys()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const handleSelect = async (v: string) => {
    if (v === BROWSE_SENTINEL) {
      const p = await open({ multiple: false, title: 'Choose SSH private key' })
      if (p && !Array.isArray(p)) onChange(p)
      return
    }
    onChange(v)
  }

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex gap-2">
        <div className="relative min-w-0 flex-1">
          <Select value={value} onChange={(e) => void handleSelect(e.target.value)}>
            <option value="" disabled>
              {loading ? 'Detecting keys…' : keys.length === 0 ? 'No keys found in ~/.ssh' : 'Select a key from ~/.ssh…'}
            </option>
            {value && !keys.includes(value) && (
              <option value={value}>
                {value.replace(/\\/g, '/').split('/').pop() ?? value} (custom)
              </option>
            )}
            {keys.map((k) => {
              const label = k.replace(/\\/g, '/').split('/').pop() ?? k
              return (
                <option key={k} value={k}>
                  {label}
                </option>
              )
            })}
            <option value={BROWSE_SENTINEL}>Browse for a key file…</option>
          </Select>
        </div>
        <Button
          variant="secondary"
          title="Refresh key list from ~/.ssh"
          aria-label="Refresh SSH key list"
          disabled={loading}
          onClick={() => void loadKeys()}
        >
          {loading ? <Spinner /> : <RefreshCw className="size-4" />}
        </Button>
      </div>

      {/* Resolved path preview */}
      {value && (
        <p className="truncate font-mono text-xs text-muted-foreground" title={value}>
          {value}
        </p>
      )}

      {/* Contextual hint */}
      <div className="flex items-start gap-1.5 rounded-md bg-muted/40 px-2.5 py-2 text-xs text-muted-foreground">
        <Info className="mt-0.5 size-3.5 shrink-0 text-ring" aria-hidden="true" />
        <span>
          Only keys in <span className="font-mono">%USERPROFILE%\.ssh\</span> appear here. To add more keys, copy them into
          that folder and click <RefreshCw className="inline size-3 align-middle" aria-hidden="true" /> to update this list.
          You can also{' '}
          <button
            type="button"
            className="underline hover:text-foreground"
            onClick={() =>
              void open({ multiple: false, title: 'Choose SSH private key' }).then(
                (p) => p && !Array.isArray(p) && onChange(p),
              )
            }
          >
            browse for any file
          </button>{' '}
          on disk.
        </span>
      </div>
    </div>
  )
}

/** §125: clone a repository into a new folder, registered as a project with its own site. */
export function GitCloneButton({ defaultParent, onDone }: { defaultParent: string; onDone: (p: Project) => void }) {
  const [openDialog, setOpen] = useState(false)
  const [url, setUrl] = useState('')
  const [domain, setDomain] = useState('')
  const [domainTouched, setDomainTouched] = useState(false)
  const [target, setTarget] = useState('')
  const [targetTouched, setTargetTouched] = useState(false)
  const [parentDir, setParentDir] = useState(defaultParent || '')
  const [branch, setBranch] = useState('')
  const [authMode, setAuthMode] = useState<'none' | 'https' | 'ssh'>('none')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [remember, setRemember] = useState(false)
  const [keyPath, setKeyPath] = useState('')
  const [passphrase, setPassphrase] = useState('')
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    if (!parentDir) void getDefaultSitesDir().then((p) => p && setParentDir(p))
  }, [parentDir])
  useEffect(() => {
    if (defaultParent) setParentDir(defaultParent)
  }, [defaultParent])

  const repoName = url.trim().replace(/\.git$/, '').split(/[/:]/).pop() ?? ''
  const effectiveParent = parentDir || defaultParent
  const suggested = repoName && effectiveParent ? buildSitePath(effectiveParent, domainToFolderName(domain) || repoName) : ''
  const resolved = target || suggested

  const onUrlChange = (newUrl: string) => {
    setUrl(newUrl)
    if (/^(git@|ssh:\/\/)/i.test(newUrl.trim())) setAuthMode('ssh')
    const derived = newUrl.trim().replace(/\.git$/, '').split(/[/:]/).pop() ?? ''
    if (derived) {
      const newDom = folderNameToDomain(derived)
      if (!domainTouched) {
        setDomain(newDom)
      }
      if (!targetTouched && effectiveParent) {
        const folder = domainTouched && domain ? (domainToFolderName(domain) || derived) : derived
        setTarget(buildSitePath(effectiveParent, folder))
      }
    }
  }

  const onDomainChange = (newDom: string) => {
    setDomain(newDom)
    setDomainTouched(newDom.trim().length > 0)
    if (!targetTouched && effectiveParent) {
      const folder = domainToFolderName(newDom) || repoName
      if (folder) setTarget(buildSitePath(effectiveParent, folder))
    }
  }

  const onTargetChange = (val: string) => {
    setTargetTouched(val.trim().length > 0)
    setTarget(val)
  }

  function resetAndClose() {
    setPassword('')
    setPassphrase('')
    setOpen(false)
  }

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
            <Button variant="ghost" onClick={resetAndClose} disabled={busy !== null}>
              Cancel
            </Button>
            <Button
              disabled={
                busy !== null ||
                !url.trim() ||
                !resolved ||
                (authMode === 'https' && (!username.trim() || !password)) ||
                (authMode === 'ssh' && !keyPath.trim())
              }
              onClick={() =>
                run('clone', async () => {
                  const dest = resolved
                  const auth =
                    authMode === 'https'
                      ? { type: 'https' as const, username, password, remember }
                      : authMode === 'ssh'
                        ? { type: 'ssh' as const, key_path: keyPath, remember, passphrase: passphrase || null }
                        : null
                  const r = await runCommand({ type: 'git_clone', url: url.trim(), target: dest, branch: branch.trim() || null, auth })
                  if (r.type === 'project') {
                    const desiredDomain = domain.trim().toLowerCase()
                    if (desiredDomain) {
                      try {
                        const existing = await runCommand({ type: 'get_domain', hostname: desiredDomain })
                        if (existing.type !== 'domain') {
                          await runCommand({
                            type: 'add_domain',
                            domain: {
                              hostname: desiredDomain,
                              project_id: r.project.id,
                              root: dest,
                              kind: { type: 'static' },
                              https: true,
                              redirect_https: true,
                              wildcard: false,
                              enabled: true,
                              ownership: 'managed',
                              app: null,
                              blocks: { headers: [], redirects: [], mappings: [], upstreams: [], includes: [] },
                              generated_hashes: {},
                            },
                          })
                          await runCommand({ type: 'apply_web', overwrite: [] })
                        }
                      } catch {
                        // ignore
                      }
                    }
                    setOpen(false)
                    setUrl('')
                    setDomain('')
                    setDomainTouched(false)
                    setTarget('')
                    setTargetTouched(false)
                    setPassword('')
                    setPassphrase('')
                    setKeyPath('')
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
            <Input value={url} onChange={(e) => onUrlChange(e.target.value)} placeholder="https://github.com/you/shop.git" className="font-mono" autoFocus />
          </Field>
          <Field label="Domain" hint={domain ? `Site opens at https://${domain.trim().toLowerCase()} once cloned.` : 'Domain name for the site (e.g. shop.test)'}>
            <Input value={domain} onChange={(e) => onDomainChange(e.target.value)} placeholder={repoName ? folderNameToDomain(repoName) : 'shop.test'} />
          </Field>
          <Field
            label="Site folder"
            hint={
              resolved
                ? `Will clone into ${resolved}. Edit folder name or Browse different location.`
                : effectiveParent
                  ? `Defaults to ${effectiveParent}\\<repo>. Edit name or Browse.`
                  : 'Folder inside <install>\\sites by default.'
            }
          >
            <FolderInput value={target} onChange={onTargetChange} title="Folder to clone into" />
            {!target && suggested ? (
              <p className="mt-1 truncate font-mono text-xs text-muted-foreground" title={suggested}>
                Default: {suggested}
              </p>
            ) : null}
          </Field>
          <Field label="Authentication" hint="Saved HTTPS credentials are used automatically when available.">
            <Select value={authMode} onChange={(e) => setAuthMode(e.target.value as typeof authMode)}>
              <option value="none">None / saved credentials</option>
              <option value="https">Username + password or token</option>
              <option value="ssh">SSH key</option>
            </Select>
          </Field>
          {authMode === 'https' && (
            <>
              <Field label="Username">
                <Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" />
              </Field>
              <Field label="Password or access token">
                <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
              </Field>
              <Toggle checked={remember} onChange={setRemember} label="Remember for this host" hint="Stores the username and token in the system credential store." />
            </>
          )}
          {authMode === 'ssh' && (
            <>
              <Field label="Private key file">
                <SshKeyPicker value={keyPath} onChange={setKeyPath} />
              </Field>
              <Field label="Key passphrase" hint="Used for this clone only; it is not saved.">
                <Input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="new-password" />
              </Field>
              <Toggle
                checked={remember}
                onChange={setRemember}
                label="Remember SSH key for this host"
                hint="Applies this key to later Git pull and push operations. Any passphrase is stored securely in the system credential store."
              />
            </>
          )}
          <Field label="Branch" hint="Blank for the repository's default branch.">
            <Input value={branch} onChange={(e) => setBranch(e.target.value)} className="font-mono" />
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
                  <Input
                    value={name}
                    onChange={(e) => {
                      const next = e.target.value
                      setName(next)
                      const clean = next.trim().replace(/[\\/:*?"<>|]+/g, '-')
                      if (clean && defaultParent) {
                        const base = target.replace(/[\\/][^\\/]+$/, '')
                        if (!base || base.toLowerCase() === defaultParent.toLowerCase()) {
                          setTarget(`${defaultParent.replace(/[\\/]+$/, '')}\\${clean}`)
                        }
                      }
                    }}
                  />
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
