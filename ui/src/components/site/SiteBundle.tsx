import { open } from '@tauri-apps/plugin-dialog'
import { Download, Lock, RefreshCw, ShieldCheck, Upload } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Field, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type BundleOptions, type BundlePreview, type OnConflict, runCommand } from '@/core'
import { timeAgo, useAction } from '@/lib/hooks'

/** One bundle covers this many sites; beyond it the user exports in batches. */
const MAX_SITES = 200

/** Without `0 O 1 l I`, so a generated password survives being read off a screen. */
const PASSWORD_ALPHABET = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'

/**
 * §165: exports the picked sites into one bundle. The four checkboxes are the whole choice —
 * settings and `.env` files are small and on by default, data is asked for — and the
 * encryption switch seals the two places passwords actually live.
 *
 * Controlled by its caller rather than owning a trigger, because it is opened from two
 * places — one site's row menu and the bulk bar — and two dialogs sharing one piece of
 * state would fight over it.
 */
export function SiteExportDialog({
  hostnames,
  isOpen,
  onClose,
  onWatch,
}: {
  hostnames: string[]
  isOpen: boolean
  onClose: () => void
  /** Called once the export is running, so the caller can show the user where to watch it. */
  onWatch?: () => void
}) {
  const [dest, setDest] = useState('')
  /** The app's own backups folder — the default, kept so the field can be put back to it. */
  const [pickedDest, setPickedDest] = useState('')
  const [options, setOptions] = useState<BundleOptions>({ settings: true, env: true, databases: false, files: false })
  const [encrypt, setEncrypt] = useState(false)
  const [password, setPassword] = useState('')
  const [shown, setShown] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  // Exports land in the app's own backups folder unless the user picks somewhere else, so
  // the field starts there rather than blank: a dialog that opens asking "where?" for a
  // choice that has a right answer is a question, not a setting. `paths.backups_dir` is
  // resolved by the core, so a moved or portable install is followed rather than guessed.
  useEffect(() => {
    if (!isOpen || pickedDest) return
    let alive = true
    void runCommand({ type: 'get_setting', key: 'paths.backups_dir' }).then((r) => {
      if (!alive || r.type !== 'setting' || typeof r.value !== 'string') return
      const appBackups = r.value
      setPickedDest(appBackups)
      // Filled only once: clearing the box afterwards leaves it empty rather than
      // snapping the default back while the user is halfway through typing a path.
      setDest((current) => current || appBackups)
    })
    return () => {
      alive = false
    }
  }, [isOpen, pickedDest])

  const chosen = hostnames.slice(0, MAX_SITES)
  const overflow = hostnames.length - chosen.length
  const nothingPicked = !options.settings && !options.env && !options.databases && !options.files

  const generate = () => {
    // Generated in the page and never sent anywhere but the export itself: the app does not
    // store it, so it has to be written down.
    const bytes = new Uint8Array(20)
    crypto.getRandomValues(bytes)
    const p = Array.from(bytes, (b) => PASSWORD_ALPHABET[b % PASSWORD_ALPHABET.length]).join('')
    setPassword(p)
    setShown(p)
  }

  const start = () =>
    run('export', async () => {
      const r = await runCommand({
        type: 'export_sites',
        hostnames: chosen,
        options,
        dest,
        password: encrypt ? password : null,
      })
      if (r.type === 'task_started') {
        setShown(null)
        setPassword('')
        onWatch?.()
      }
    })

  return (
    <Dialog
      open={isOpen}
      onClose={() => busy === null && onClose()}
      title={`Export ${chosen.length} site${chosen.length === 1 ? '' : 's'}`}
      description="One bundle file holding what you choose. Watch it on the Processes page."
      size="form"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy !== null}>
            Cancel
          </Button>
            <Button
              disabled={busy !== null || !dest.trim() || nothingPicked || (encrypt && !password)}
              title={nothingPicked ? 'Choose at least one thing to include' : undefined}
              onClick={() => void start()}
            >
            {busy === 'export' ? <Spinner /> : <Download />} Export
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
          <ErrorCard error={error} onDismiss={() => setError(null)} />

          <div className="rounded-lg border border-border p-3 text-xs">
            <p className="font-medium">Sites</p>
            <p className="break-words text-muted-foreground">{chosen.join(', ')}</p>
            {overflow > 0 && (
              <p className="mt-1 text-warning">
                {overflow} more site{overflow === 1 ? '' : 's'} were left out — one bundle covers up to {MAX_SITES}.
              </p>
            )}
            <p className="mt-1 text-muted-foreground">
              Sites of the same project travel as one entry, so they come back as one project rather than several.
            </p>
          </div>

          <Field label="What to include" hint="Data is left out unless you ask for it: a dump is the size of your database.">
            <div className="flex flex-col gap-2.5 rounded-md border border-border p-3">
              <Pick
                checked={options.settings}
                onChange={(v) => setOptions({ ...options, settings: v })}
                label="Site settings"
                hint="Domain, project manifest, workers, scheduled tasks, tunnels, mode"
              />
              <Pick
                checked={options.env}
                onChange={(v) => setOptions({ ...options, env: v })}
                label=".env files"
                hint="Environment variables — where the application's passwords and keys live"
              />
              <Pick
                checked={options.databases}
                onChange={(v) => setOptions({ ...options, databases: v })}
                label="Database"
                hint="SQL dump of each site's MariaDB / PostgreSQL database"
              />
              <Pick
                checked={options.files}
                onChange={(v) => setOptions({ ...options, files: v })}
                label="Project files"
                hint="The site's own files, without node_modules, vendor or virtual environments"
              />
            </div>
          </Field>

          <Field
            label="Destination folder"
            hint={
              dest && dest !== pickedDest
                ? 'A folder you chose. The bundle is named after the site(s) and never overwrites an earlier export.'
                : "The app's own backups folder. One bundle file is written here."
            }
          >
            <div className="flex gap-2">
              <Input
                value={dest}
                onChange={(e) => setDest(e.target.value)}
                placeholder={pickedDest || 'the app folder\\backups'}
                className="min-w-0 flex-1 font-mono text-xs"
              />
              <Button
                variant="secondary"
                onClick={() =>
                  void open({ directory: true, title: 'Folder to export the bundle into' }).then((p) => {
                    if (p && !Array.isArray(p)) setDest(p)
                  })
                }
              >
                Browse
              </Button>
            </div>
            {pickedDest && dest !== pickedDest && (
              <button
                type="button"
                className="self-start text-xs text-muted-foreground underline-offset-2 hover:underline"
                onClick={() => setDest(pickedDest)}
              >
                Back to the app's backups folder
              </button>
            )}
          </Field>

          <Toggle
            checked={encrypt}
            onChange={(v) => {
              setEncrypt(v)
              if (!v) setShown(null)
            }}
            label="Encrypt passwords in the bundle"
            hint="Seals the .env files and database dumps with a password. Project files and settings stay readable, so the bundle can still be reviewed. The password is never stored."
          />
          {encrypt && (
            <Field
              label="Bundle password"
              hint={shown ? 'Write this down now: it is shown once and cannot be recovered.' : 'Needed to import the bundle. Leave it empty and generate one.'}
            >
              <div className="flex gap-2">
                <Input
                  type="password"
                  value={password}
                  onChange={(e) => {
                    setPassword(e.target.value)
                    setShown(null)
                  }}
              placeholder="your own, or generate one"
              className="min-w-0 flex-1"
              autoComplete="off"
              aria-label="Bundle password"
                />
                <Button variant="secondary" onClick={generate} title="Make a password to write down">
                  <RefreshCw /> Generate
                </Button>
              </div>
              {shown && (
                <p className="mt-1.5 flex items-center gap-1.5 rounded-md bg-success/10 px-2 py-1.5 font-mono text-xs text-success">
                  <Lock className="size-3.5 shrink-0" /> {shown}
                </p>
              )}
            </Field>
          )}
      </div>
    </Dialog>
  )
}

function Pick({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label: string
  hint: string
}) {
  return (
    <label className="flex cursor-pointer items-start gap-2.5">
      <Checkbox checked={checked} onChange={onChange} label={label} className="mt-0.5" />
      <span className="min-w-0">
        <span className="block text-sm">{label}</span>
        <span className="block text-xs text-muted-foreground">{hint}</span>
      </span>
    </label>
  )
}

/**
 * §165: imports a bundle. Nothing is created until the file has been read and shown, and a
 * site that already exists is either updated — after a snapshot of what is there now — or
 * created under a free name.
 */
export function SiteImportButton({ onWatch }: { onWatch?: () => void }) {
  const [preview, setPreview] = useState<BundlePreview | null>(null)
  const [source, setSource] = useState('')
  const [password, setPassword] = useState('')
  const [mode, setMode] = useState<OnConflict['type']>('rename')
  const [name, setName] = useState('')
  const { busy, error, setError, run } = useAction()

  const colliding = preview?.sites.reduce((n, s) => n + s.existing.length, 0) ?? 0
  const needsPassword = preview?.encrypted === true

  const pick = () =>
    run('preview', async () => {
      const p = await open({
        multiple: false,
        title: 'Choose a site bundle',
        filters: [{ name: 'Site bundle', extensions: ['zip'] }],
      })
      if (!p || Array.isArray(p)) return
      setSource(p)
      setPassword('')
      setMode('rename')
      setName('')
      const r = await runCommand({ type: 'preview_site_bundle', source: p })
      if (r.type === 'site_bundle_preview') setPreview(r.preview)
    })

  const start = () =>
    run('import', async () => {
      const r = await runCommand({
        type: 'import_sites',
        source,
        password: needsPassword ? password : null,
        on_conflict: { type: mode },
        name: name.trim() || null,
        options: { config: true, env: true, databases: true, files: true },
      })
      if (r.type === 'task_started') {
        setPreview(null)
        setPassword('')
        onWatch?.()
      }
    })

  return (
    <>
      <Button
        size="sm"
        variant="secondary"
        onClick={() => void pick()}
        title="Import a site bundle exported from OLS"
      >
        {busy === 'preview' ? <Spinner /> : <Upload />} Import bundle
      </Button>
      {error && !preview && <ErrorCard error={error} onDismiss={() => setError(null)} />}
      <Dialog
        open={preview !== null}
        onClose={() => busy === null && setPreview(null)}
        title="Import a site bundle"
        description="Review what is in the file. Nothing is created until you import it."
        size="wide"
        footer={
          <>
            <Button variant="ghost" onClick={() => setPreview(null)} disabled={busy !== null}>
              Cancel
            </Button>
            <Button disabled={busy !== null || (needsPassword && password.length === 0)} onClick={() => void start()}>
              {busy === 'import' ? <Spinner /> : <Upload />} Import
            </Button>
          </>
        }
      >
        {preview && (
          <div className="flex flex-col gap-4 text-sm">
            <ErrorCard error={error} onDismiss={() => setError(null)} />

            <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <Badge variant="outline">{preview.label}</Badge>
              <span>exported {timeAgo(preview.created_ms)}</span>
              {preview.encrypted && (
                <Badge variant="warning">
                  <Lock className="size-3" /> encrypted
                </Badge>
              )}
            </div>

            {preview.problems.length > 0 && (
              <div className="rounded-lg border border-warning/40 bg-warning/10 p-3 text-xs">
                {preview.problems.map((p) => (
                  <p key={p}>⚠ {p}</p>
                ))}
              </div>
            )}

            {needsPassword && (
              <Field
                label="Bundle password"
                hint="The password this bundle was exported with. It is used once and never stored."
              >
                <Input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="the export password"
                  autoComplete="off"
                />
              </Field>
            )}

            <div className="flex flex-col gap-2">
              {preview.sites.map((s) => (
                <div key={s.project_name} className="rounded-lg border border-border p-3">
                  <p className="font-medium">{s.project_name}</p>
                  <p className="break-words text-xs text-muted-foreground">{s.hostnames.join(', ')}</p>
                  {s.summary.length > 0 && (
                    <ul className="mt-1.5 text-xs text-muted-foreground">
                      {s.summary.map((line) => (
                        <li key={line}>• {line}</li>
                      ))}
                    </ul>
                  )}
                  {s.existing.length > 0 ? (
                    <p className="mt-1.5 text-xs text-warning">
                      {s.existing.length} of {s.hostnames.length} already exist: {s.existing.join(', ')}
                      {mode === 'rename' && ` → they would be created as ${s.suggested.join(', ')}`}
                    </p>
                  ) : (
                    <p className="mt-1.5 text-xs text-success">
                      No name clashes — it is created as a new project.
                    </p>
                  )}
                </div>
              ))}
            </div>

            {colliding > 0 && (
              <Field
                label={`${colliding} site${colliding === 1 ? '' : 's'} already exist`}
                hint="Choose what to do with them."
              >
                <div className="flex flex-col gap-2">
                  <Choice
                    checked={mode === 'rename'}
                    onChange={() => setMode('rename')}
                    title="Create as new"
                    hint="Imports under free names instead, adjusting the database and ports so both copies run. Nothing existing is touched."
                  />
                  <Choice
                    checked={mode === 'update'}
                    onChange={() => setMode('update')}
                    title="Update the existing sites"
                    icon={<ShieldCheck className="size-3.5" />}
                    hint="Puts the bundle's configuration on them. A snapshot of the current state is taken first, so this can be undone."
                  />
                </div>
              </Field>
            )}

            {mode === 'rename' && (
              <Field
                label="Name for the new copy"
                hint="Left blank, the bundle's own name is used, with a number added when it is taken."
              >
                <Input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={preview.sites[0]?.project_name ?? ''}
                />
              </Field>
            )}
          </div>
        )}
      </Dialog>
    </>
  )
}

function Choice({
  checked,
  onChange,
  title,
  hint,
  icon,
}: {
  checked: boolean
  onChange: () => void
  title: string
  hint: string
  icon?: React.ReactNode
}) {
  return (
    <label className={checked ? 'flex cursor-pointer items-start gap-2.5 rounded-md border border-primary bg-accent/40 p-3' : 'flex cursor-pointer items-start gap-2.5 rounded-md border border-border p-3'}>
      <input type="radio" name="conflict" className="mt-1" checked={checked} onChange={onChange} />
      <span className="min-w-0">
        <span className="flex items-center gap-1.5 text-sm">
          {icon}
          {title}
        </span>
        <span className="block text-xs text-muted-foreground">{hint}</span>
      </span>
    </label>
  )
}
