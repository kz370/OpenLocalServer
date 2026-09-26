import { open } from '@tauri-apps/plugin-dialog'
import { ArrowLeftRight, Braces, FileCode2, FileText, FolderSearch, Rocket } from 'lucide-react'
import { type ReactNode, useEffect, useState } from 'react'

import { TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { type Domain, type Project, type QuickEntryView, type TunnelStatus, runCommand } from '@/core'

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
    <Dialog open wide onClose={onClose} title="Add site" description="Changes are applied to the web server as soon as you save.">
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
  const kindType = d.kind.type
  const [appLine, setAppLine] = useState('')
  const [template, setTemplate] = useState('{project}.test')
  const [showApps, setShowApps] = useState(false)
  const [quickApps, setQuickApps] = useState<QuickEntryView[]>([])
  const [tunnels, setTunnels] = useState<TunnelStatus[]>([])
  const [httpPort, setHttpPort] = useState(80)
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
  useEffect(() => {
    void runCommand({ type: 'list_tunnels' }).then((r) => r.type === 'tunnels' && setTunnels(r.tunnels.map((t) => t)))
    void runCommand({ type: 'get_web_config' }).then((r) => r.type === 'web_config' && setHttpPort(r.config.http_port))
  }, [])
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
    if (out.public_domain && !out.tunnel_id) return setErr('Choose a saved named Cloudflare tunnel for the public domain')
    onSave(out)
  }

  const kinds: { id: 'php' | 'proxy' | 'static'; title: string; hint: string; icon: ReactNode }[] = [
    { id: 'php', title: 'PHP', hint: 'Laravel, WordPress, plain PHP (FastCGI)', icon: <Braces className="size-5" /> },
    { id: 'proxy', title: 'Reverse proxy', hint: 'Dev server, Docker, another computer', icon: <ArrowLeftRight className="size-5" /> },
    { id: 'static', title: 'Static files', hint: 'HTML, CSS and JS served as they are', icon: <FileText className="size-5" /> },
  ]
  const host = d.hostname.trim().toLowerCase()
  const localTld = /\.(test|local|localhost|dev\.test)$/.test(host)

  return (
    <div className="flex flex-col gap-6">
      {err && <p className="text-sm text-destructive">{err}</p>}

      {isNew && (
        <FormSection title="Source" hint="Start from a project you already have, or create a new app.">
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
          {onQuickApp && (
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
        </FormSection>
      )}

      <FormSection title="Address">
        <Field
          label="Domain"
          hint={
            host
              ? `Opens at ${d.https ? 'https' : 'http'}://${host}${localTld ? '' : ' — use .test, .local or .localhost so it never clashes with a real website'}`
              : 'Subdomains work too: api.shop.test routes independently of shop.test'
          }
        >
          <Input value={d.hostname} onChange={(e) => set({ hostname: e.target.value })} placeholder="shop.test, myapp.local, api.company.dev…" />
        </Field>
        <Field label="Site folder">
          <div className="flex gap-2">
            <Input value={d.root} onChange={(e) => set({ root: e.target.value })} placeholder="C:\Sites\shop\public" className="min-w-0 flex-1" />
            <Button variant="secondary" onClick={browseRoot}>
              <FolderSearch /> Browse
            </Button>
          </div>
        </Field>
      </FormSection>

      <FormSection title="Public domain" hint={`In the Cloudflare dashboard, route the public hostname to http://localhost:${httpPort} and set HTTP Host Header to the public hostname. TLS ends at Cloudflare; the local origin stays HTTP.`}>
        <Field label="Hostname">
          <Input value={d.public_domain ?? ''} onChange={(e) => set(e.target.value.trim() ? { public_domain: e.target.value.trim() } : { public_domain: null, tunnel_id: null })} placeholder="dev.example.com" />
        </Field>
        {d.public_domain && <Field label="Named Cloudflare tunnel" hint={tunnels.some((t) => t.config.provider === 'cloudflare' && !!t.config.public_hostname) ? `Select a saved tunnel; it reconnects after exit. For Laravel, set APP_URL=https://${d.public_domain} and trust Cloudflare's proxy headers.` : 'Create a named tunnel in the Cloudflare dashboard, save its token in Tunnels → Providers, then add it on the Tunnels page.'}>
          <Select value={d.tunnel_id ?? ''} onChange={(e) => set({ tunnel_id: e.target.value || null })}>
            <option value="">Choose a tunnel…</option>
            {tunnels.filter((t) => t.config.provider === 'cloudflare' && !!t.config.public_hostname).map((t) => <option key={t.config.id} value={t.config.id}>{t.config.name} · {t.config.public_hostname}</option>)}
          </Select>
        </Field>}
      </FormSection>

      <FormSection title="Serves">
        <div className="grid gap-3 sm:grid-cols-3" role="radiogroup" aria-label="What the site serves">
          {kinds.map((k) => (
            <button
              key={k.id}
              type="button"
              role="radio"
              aria-checked={kindType === k.id}
              onClick={() => set({ kind: k.id === 'php' ? { type: 'php', version: null } : k.id === 'proxy' ? { type: 'proxy', upstream_port: 3000 } : { type: 'static' } })}
              className={cn(
                'flex flex-col items-start gap-1.5 rounded-lg border p-3 text-left transition-colors',
                kindType === k.id ? 'border-primary bg-accent text-accent-foreground' : 'border-border hover:bg-accent/40',
              )}
            >
              {k.icon}
              <span className="text-sm font-medium">{k.title}</span>
              <span className="text-xs text-muted-foreground">{k.hint}</span>
            </button>
          ))}
        </div>
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
      </FormSection>

      <FormSection title="Options">
        <div className="grid gap-3 md:grid-cols-3">
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.https} onChange={(v) => set({ https: v, redirect_https: v ? d.redirect_https : false })} label="HTTPS" hint="A certificate from the local CA is created for this domain." />
          </div>
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.redirect_https} disabled={!d.https} onChange={(v) => set({ redirect_https: v })} label="Redirect to HTTPS" hint="Turn off for projects that need plain HTTP." />
          </div>
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.wildcard} onChange={(v) => set({ wildcard: v })} label="Wildcard" hint={`Answers *.${host || 'domain'}; needs a one-time Windows DNS rule (you'll be asked to approve it).`} />
          </div>
        </div>
      </FormSection>

      {!isNew && (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <FileCode2 className="size-3.5" /> Config ownership: <b>{d.ownership}</b>. Change it on the Web server config tab.
        </p>
      )}
      <div className="flex justify-end gap-2 border-t border-border pt-3">{actions(submit, busy)}</div>
    </div>
  )
}

/** A titled group of fields inside a dialog form. */
function FormSection({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <div>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{title}</h3>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </section>
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
