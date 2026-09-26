import { open } from '@tauri-apps/plugin-dialog'
import { FileCode2, FolderSearch, Rocket } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'

import { TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type Domain, type Project, type QuickEntryView, runCommand } from '@/core'

const emptyBlocks = { headers: [], redirects: [], mappings: [], upstreams: [], includes: [] }

export function splitArgs(line: string): string[] {
  const out: string[] = []
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(line))) out.push(m[1] ?? m[2] ?? m[3])
  return out
}

export function newDomain(): Domain {
  return {
    hostname: '',
    project_id: null,
    root: '',
    kind: { type: 'static' },
    https: true,
    redirect_https: true,
    wildcard: false,
    enabled: true,
    ownership: 'managed',
    app: null,
    blocks: emptyBlocks,
    generated_hashes: {},
  }
}

/** The "Add site" dialog. Editing an existing site happens in the site dialog's Settings tab. */
export function DomainDialog({
  domain,
  onClose,
  ...rest
}: {
  projects: Project[]
  onQuickApp: (id: string) => void
  domain: Domain | null
  installedPhp: string[]
  onClose: () => void
  onSave: (d: Domain) => void
  busy: boolean
}) {
  if (!domain) return null
  return (
    <Dialog open onClose={onClose} title="Add site" description="Changes are applied to the web server as soon as you save.">
      <DomainSettings
        {...rest}
        domain={domain}
        isNew
        actions={(submit, busy) => (
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button disabled={busy} onClick={submit}>
              Add and apply
            </Button>
          </>
        )}
      />
    </Dialog>
  )
}

/** A site's domain, folder, type and HTTPS settings. `actions` renders the buttons under the form. */
export function DomainSettings({
  projects,
  onQuickApp,
  domain,
  isNew,
  installedPhp,
  onSave,
  busy,
  actions,
}: {
  projects: Project[]
  onQuickApp?: (id: string) => void
  domain: Domain
  isNew: boolean
  installedPhp: string[]
  onSave: (d: Domain) => void
  busy: boolean
  actions: (submit: () => void, busy: boolean) => ReactNode
}) {
  const [d, setD] = useState<Domain>(domain)
  const [appLine, setAppLine] = useState('')
  const [template, setTemplate] = useState('{project}.test')
  const [showApps, setShowApps] = useState(false)
  const [quickApps, setQuickApps] = useState<QuickEntryView[]>([])
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    setD(domain)
    setErr(null)
    setShowApps(false)
    setAppLine(domain?.app ? [domain.app.executable, ...domain.app.args].join(' ') : '')
  }, [domain])
  useEffect(() => {
    if (!showApps) return
    void runCommand({ type: 'list_quick_apps' }).then((r) => r.type === 'quick_apps' && setQuickApps(r.apps))
  }, [showApps])
  async function pickProject(id: string) {
    const p = projects.find((x) => x.id === id)
    if (!p) return setD((cur) => ({ ...cur, project_id: null }))
    let root = p.path
    const detail = await runCommand({ type: 'get_project_detail', id })
    let kind: Domain['kind'] | null = null
    if (detail.type === 'project_detail') {
      const fw = detail.detail.detection.framework
      const sub = detail.detail.detection.doc_root ?? (fw === 'laravel' || fw === 'symfony' ? 'public' : null)
      if (sub) root = `${p.path}\\${sub}`
      if (fw === 'laravel' || fw === 'symfony' || fw === 'word_press' || fw === 'generic_php') kind = { type: 'php', version: null }
      else if (fw === 'node' || fw === 'fast_api' || fw === 'django' || fw === 'flask') kind = { type: 'proxy', upstream_port: 3000 }
    }
    const suggested = await runCommand({ type: 'suggest_domain', project_id: id, template })
    setD((cur) => ({ ...cur, project_id: id, root, kind: kind ?? cur.kind, hostname: cur.hostname || (suggested.type === 'text' ? suggested.text : '') }))
  }

  // "Add site" from a project arrives with the project chosen: fill in its folder, type and name.
  useEffect(() => {
    if (isNew && domain.project_id && !domain.root) void pickProject(domain.project_id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [domain])

  const set = (patch: Partial<Domain>) => setD({ ...d, ...patch })
  const kindType = d.kind.type

  async function browseRoot() {
    const picked = await open({ directory: true, title: 'Document root' })
    if (picked && !Array.isArray(picked)) set({ root: picked })
  }

  function submit() {
    const out = { ...d, hostname: d.hostname.trim().toLowerCase() }
    if (out.kind.type === 'proxy' && appLine.trim()) {
      const [executable, ...args] = splitArgs(appLine)
      out.app = { executable, args, cwd: out.app?.cwd || out.root, runtime: out.app?.runtime ?? (executable === 'node' || executable === 'npm' ? 'node' : null) }
    } else {
      out.app = null
    }
    if (!out.https) out.redirect_https = false
    if (!out.hostname) return setErr('Enter a domain like shop.test')
    if (!out.root) return setErr('Choose the site folder')
    onSave(out)
  }

  return (
    <div className="flex flex-col gap-4">
      {err && <p className="text-sm text-destructive">{err}</p>}
      {isNew && (
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Project (optional)">
            <Select value={d.project_id ?? ''} onChange={(e) => pickProject(e.target.value)}>
              <option value="">— none —</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Name template (§48)">
            <Select value={template} onChange={(e) => setTemplate(e.target.value)}>
              <option>{'{project}.test'}</option>
              <option>{'api.{project}.test'}</option>
              <option>{'admin.{project}.test'}</option>
            </Select>
          </Field>
        </div>
      )}
      {isNew && onQuickApp && (
        <div className="flex flex-col gap-2">
          <Button variant="secondary" className="self-start" onClick={() => setShowApps((v) => !v)}>
            <Rocket /> {showApps ? 'Hide Quick Apps' : 'Create from a Quick App'}
          </Button>
          {showApps && (
            <div className="grid max-h-56 gap-1.5 overflow-y-auto rounded-lg border border-border p-2 sm:grid-cols-2">
              {quickApps.length === 0 && <p className="p-2 text-sm text-muted-foreground">No Quick Apps available.</p>}
              {quickApps.map((a) => (
                <button
                  key={a.id}
                  onClick={() => onQuickApp(a.id)}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-accent"
                >
                  <TechTile id={a.category} />
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{a.name}</span>
                    <span className="block truncate text-xs text-muted-foreground">{a.description}</span>
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      <Field label="Domain" hint="Subdomains work too: api.shop.test routes independently of shop.test">
        <Input value={d.hostname} onChange={(e) => set({ hostname: e.target.value })} placeholder="shop.test, myapp.local, api.company.dev…" />
      </Field>
      <div className="flex gap-2">
        <Field label="Site folder">
          <div className="flex gap-2">
            <Input value={d.root} onChange={(e) => set({ root: e.target.value })} placeholder="C:\Sites\shop\public" className="w-96" />
            <Button variant="secondary" onClick={browseRoot}>
              <FolderSearch /> Browse
            </Button>
          </div>
        </Field>
      </div>
      <Field label="Serves">
        <Select
          value={kindType}
          onChange={(e) => {
            const t = e.target.value
            set({ kind: t === 'php' ? { type: 'php', version: null } : t === 'proxy' ? { type: 'proxy', upstream_port: 3000 } : { type: 'static' } })
          }}
        >
          <option value="php">PHP (FastCGI)</option>
          <option value="proxy">Reverse proxy (dev server, Docker, another computer…)</option>
          <option value="static">Static files</option>
        </Select>
      </Field>
      {d.kind.type === 'php' && (
        <Field label="PHP version" hint="“Project default” follows the project's own resolved version">
          <Select value={d.kind.version ?? ''} onChange={(e) => set({ kind: { type: 'php', version: e.target.value || null } })}>
            <option value="">Project default / newest</option>
            {phpOptions(installedPhp, d.kind.version).map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
      )}
      {d.kind.type === 'proxy' && (
        <>
          <div className="grid gap-3 sm:grid-cols-[1fr_8rem]">
            <Field label="Forward to" hint="Blank = this computer. Or a Docker host, another PC's IP, a hostname.">
              <Input
                value={d.kind.upstream_host ?? ''}
                onChange={(e) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000, upstream_host: e.target.value.trim() || null } })}
                placeholder="127.0.0.1"
              />
            </Field>
            <Field label="Port">
              <Input
                type="number"
                value={d.kind.upstream_port}
                onChange={(e) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: Number(e.target.value) } })}
              />
            </Field>
          </div>
          <Toggle
            checked={!!d.kind.upstream_https}
            onChange={(v) => set({ kind: { ...d.kind, type: 'proxy', upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000, upstream_https: v } })}
            label="The target only speaks HTTPS"
            hint="Self-signed certificates on the target are accepted."
          />
          {!d.kind.upstream_host && (
            <Field label="Start command (optional)" hint="OpenLocalServer supervises it and passes PORT. Example: npm run dev">
              <Input value={appLine} onChange={(e) => setAppLine(e.target.value)} placeholder="npm run dev" />
            </Field>
          )}
        </>
      )}
      <div className="flex flex-col gap-2.5">
        <Toggle checked={d.https} onChange={(v) => set({ https: v, redirect_https: v ? d.redirect_https : false })} label="HTTPS" hint="A certificate from the local CA is created for this domain." />
        <Toggle checked={d.redirect_https} disabled={!d.https} onChange={(v) => set({ redirect_https: v })} label="Redirect HTTP to HTTPS" hint="Turn off for projects that need plain HTTP." />
        <Toggle checked={d.wildcard} onChange={(v) => set({ wildcard: v })} label={`Wildcard (*.${d.hostname || 'domain'})`} hint="Answers every subdomain; needs a one-time Windows DNS rule (you'll be asked to approve it)." />
      </div>
      {!isNew && (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <FileCode2 className="size-3.5" /> Config ownership: <b>{d.ownership}</b>. Change it on the Web server config tab.
        </p>
      )}
      <div className="flex justify-end gap-2 border-t border-border pt-3">{actions(submit, busy)}</div>
    </div>
  )
}

/**
 * One "PHP 8.3" choice per minor version (the server picks the newest 8.3.x), plus each
 * exact version when several share a minor, so a specific build can be pinned.
 */
function phpOptions(installed: string[], current: string | null): { value: string; label: string }[] {
  const key = (v: string) => v.split('.').map((p) => parseInt(p, 10) || 0)
  const desc = (a: string, b: string) => {
    const [ka, kb] = [key(a), key(b)]
    for (let i = 0; i < Math.max(ka.length, kb.length); i++) if ((kb[i] ?? 0) !== (ka[i] ?? 0)) return (kb[i] ?? 0) - (ka[i] ?? 0)
    return 0
  }
  const byMinor = new Map<string, string[]>()
  for (const v of [...installed].sort(desc)) {
    const minor = v.split('.').slice(0, 2).join('.')
    byMinor.set(minor, [...(byMinor.get(minor) ?? []), v])
  }
  const options: { value: string; label: string }[] = []
  for (const [minor, versions] of byMinor) {
    options.push({ value: minor, label: versions.length > 1 ? `PHP ${minor} (newest: ${versions[0]})` : `PHP ${minor} (${versions[0]})` })
    if (versions.length > 1) for (const v of versions) options.push({ value: v, label: `  PHP ${v} exactly` })
  }
  // A saved version that's no longer installed still shows, instead of silently blanking.
  if (current && !options.some((o) => o.value === current)) options.push({ value: current, label: `PHP ${current} (not installed)` })
  return options
}
