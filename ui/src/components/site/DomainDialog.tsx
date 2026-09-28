import { open } from '@tauri-apps/plugin-dialog'
import { ArrowLeftRight, Braces, Check, CircleAlert, FileCode2, FileText, FolderSearch, Rocket } from 'lucide-react'
import { type ReactNode, useEffect, useRef, useState } from 'react'

import { TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, FormSection, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { buildSitePath, domainToFolderName, getDefaultSitesDir, getDefaultTld, slugify } from '@/lib/sites'
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
    server: null,
  }
}

/** The "Add site" dialog. Editing an existing site happens in the site dialog's Settings tab. */
export function DomainDialog({
  domain,
  onClose,
  onSave,
  busy,
  projects,
  onQuickApp,
  installedPhp,
  defaultParent,
}: {
  projects: Project[]
  onQuickApp: (id: string) => void
  domain: Domain | null
  installedPhp: string[]
  defaultParent?: string
  onClose: () => void
  onSave: (d: Domain) => void
  busy: boolean
}) {
  if (!domain) return null
  return (
    <AddSiteDialogBody
      projects={projects}
      onQuickApp={onQuickApp}
      domain={domain}
      defaultParent={defaultParent}
      installedPhp={installedPhp}
      onClose={onClose}
      onSave={onSave}
      busy={busy}
    />
  )
}

function AddSiteDialogBody({
  projects,
  onQuickApp,
  domain,
  defaultParent,
  installedPhp,
  onSave,
  busy,
  onClose,
}: {
  projects: Project[]
  onQuickApp?: (id: string) => void
  domain: Domain
  defaultParent?: string
  installedPhp: string[]
  onSave: (d: Domain) => void
  busy: boolean
  onClose: () => void
}) {
  const submitRef = useRef<(() => void) | null>(null)
  return (
    <Dialog
      open
      size="form"
      onClose={onClose}
      title="Add site"
      description="Changes are applied to the web server as soon as you save."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={() => submitRef.current?.()}>
            Add and apply
          </Button>
        </>
      }
    >
      <DomainSettings
        projects={projects}
        onQuickApp={onQuickApp}
        domain={domain}
        isNew
        defaultParent={defaultParent}
        installedPhp={installedPhp}
        onSave={onSave}
        busy={busy}
        submitRef={submitRef}
      />
    </Dialog>
  )
}

/** A site's domain, folder, type and HTTPS settings. `actions` renders the buttons under the form (used by the Settings tab); the Add site modal renders them in the dialog footer instead. */
export function DomainSettings({
  projects,
  onQuickApp,
  domain,
  isNew,
  defaultParent,
  installedPhp,
  onSave,
  busy,
  actions,
  submitRef,
}: {
  projects: Project[]
  onQuickApp?: (id: string) => void
  domain: Domain
  isNew: boolean
  defaultParent?: string
  installedPhp: string[]
  onSave: (d: Domain) => void
  busy: boolean
  actions?: (submit: () => void, busy: boolean) => ReactNode
  submitRef?: { current: (() => void) | null }
}) {
  const [d, setD] = useState<Domain>(domain)
  const [projectName, setProjectName] = useState(() => {
    if (domain.project_id) {
      const p = projects.find((x) => x.id === domain.project_id)
      return p ? p.name : ''
    }
    return ''
  })
  const [rootTouched, setRootTouched] = useState(false)
  const [domainTouched, setDomainTouched] = useState(false)
  /** Set once the user types a project name, so a later projects-changed event leaves it alone. */
  const projectNameTouched = useRef(false)
  const [parentDir, setParentDir] = useState(defaultParent || '')
  const kindType = d.kind.type
  const [appLine, setAppLine] = useState('')
  const [defaultTld, setDefaultTld] = useState('local')
  const [template, setTemplate] = useState(`{project}.local`)
  const [showApps, setShowApps] = useState(false)
  const [quickApps, setQuickApps] = useState<QuickEntryView[]>([])
  const [tunnels, setTunnels] = useState<TunnelStatus[]>([])
  const [webServers, setWebServers] = useState<{ id: string; name: string; http: number; https: number; default: boolean }[]>([])
  const [err, setErr] = useState<string | null>(null)

  // Re-seed the form only when the domain being edited actually changes. Keying this
  // on `projects` too meant a project-folder watcher event (debounced, fires on any
  // change under the sites root) reset the form and discarded whatever had been picked
  // but not yet saved — picking a web server and then saving wrote the stale value
  // back, which is how a pin to one server kept reverting to "Default (automatic)".
  useEffect(() => {
    setD(domain)
    setErr(null)
    setShowApps(false)
    setRootTouched(false)
    setDomainTouched(false)
    projectNameTouched.current = false
    setAppLine(domain?.app ? [domain.app.executable, ...domain.app.args].join(' ') : '')
    if (!domain.project_id) setProjectName('')
  }, [domain])

  // The project list is its own stream and changes identity on every watcher event;
  // only the name derived from it follows along, and never over a name the user is
  // still typing.
  useEffect(() => {
    if (projectNameTouched.current) return
    if (!domain.project_id) return
    const p = projects.find((x) => x.id === domain.project_id)
    setProjectName(p ? p.name : '')
  }, [domain.project_id, projects])

  useEffect(() => {
    void getDefaultTld().then((tld) => {
      setDefaultTld(tld)
      const tpl = `{project}.${tld}`
      setTemplate(tpl)
      if (projectName.trim() && !domainTouched) {
        const slug = slugify(projectName.trim())
        if (slug) setD((cur) => ({ ...cur, hostname: tpl.replace('{project}', slug) }))
      }
    })
  }, [])

  useEffect(() => {
    if (!parentDir) void getDefaultSitesDir().then((p) => p && setParentDir(p))
  }, [parentDir])

  useEffect(() => {
    if (defaultParent) setParentDir(defaultParent)
  }, [defaultParent])

  useEffect(() => {
    if (isNew && !rootTouched && parentDir && !d.root) {
      const slug = projectName.trim() ? slugify(projectName.trim()) : ''
      const folder = slug || (d.hostname ? domainToFolderName(d.hostname) : '')
      if (folder) setD((cur) => ({ ...cur, root: buildSitePath(parentDir, folder) }))
    }
  }, [isNew, rootTouched, projectName, d.hostname, d.root, parentDir])

  useEffect(() => {
    if (!showApps) return
    void runCommand({ type: 'list_quick_apps' }).then((r) => r.type === 'quick_apps' && setQuickApps(r.apps))
  }, [showApps])

  useEffect(() => {
    void runCommand({ type: 'list_tunnels' }).then((r) => r.type === 'tunnels' && setTunnels(r.tunnels.map((t) => t)))
    void runCommand({ type: 'get_web_status' }).then((r) => {
      if (r.type !== 'web_status') return
      setWebServers(
        // The default first, so the "Default (…)" label names the right one.
        [...r.status.servers].sort((a, b) => Number(b.active) - Number(a.active)).map((s) => ({
          id: s.id,
          name: s.name,
          http: s.http_port,
          https: s.https_port,
          default: s.active,
        })),
      )
    })
  }, [])

  // The port the tunnel should point at is the one this site's own server binds.
  const pickedServer = webServers.find((s) => (d.server ?? webServers[0]?.id) === s.id)
  const httpPort = pickedServer?.http ?? 80

  const onProjectNameChange = (val: string) => {
    setProjectName(val)
    projectNameTouched.current = true
    const trimmed = val.trim()
    if (!trimmed) setDomainTouched(false)
    const slug = slugify(trimmed)

    // Auto-fill domain from template if domain hasn't been manually detached
    const newHost = !domainTouched ? (slug ? template.replace('{project}', slug) : '') : d.hostname

    // Auto-fill site folder from parentDir if root hasn't been manually touched
    let newRoot = d.root
    if (!rootTouched) {
      const folder = slug || (trimmed ? domainToFolderName(trimmed) : '')
      newRoot = folder && parentDir ? buildSitePath(parentDir, folder) : ''
    }

    // Check if there is an existing project matching this name or id
    const matched = projects.find(
      (p) => p.name.toLowerCase() === trimmed.toLowerCase() || p.id.toLowerCase() === trimmed.toLowerCase()
    )

    setD((cur) => ({
      ...cur,
      hostname: newHost,
      root: newRoot,
      project_id: matched ? matched.id : null,
    }))

    if (matched && !rootTouched) {
      void runCommand({ type: 'get_project_detail', id: matched.id }).then((detail) => {
        if (detail.type === 'project_detail') {
          let root = matched.path
          const fw = detail.detail.detection.framework
          const sub = detail.detail.detection.doc_root ?? (fw === 'laravel' || fw === 'symfony' ? 'public' : null)
          if (sub) root = `${matched.path}\\${sub}`
          let kind: Domain['kind'] | null = null
          if (fw === 'laravel' || fw === 'symfony' || fw === 'word_press' || fw === 'generic_php') kind = { type: 'php', version: null }
          else if (fw === 'node' || fw === 'fast_api' || fw === 'django' || fw === 'flask') kind = { type: 'proxy', upstream_port: 3000 }
          setD((cur) => ({
            ...cur,
            root,
            kind: kind ?? cur.kind,
          }))
        }
      })
    }
  }

  const onTemplateChange = (val: string) => {
    setTemplate(val)
    if (projectName.trim() && !domainTouched) {
      const slug = slugify(projectName.trim())
      if (slug) {
        setD((cur) => ({ ...cur, hostname: val.replace('{project}', slug) }))
      }
    }
  }

  // "Add site" from a project arrives with the project chosen: fill in its folder, type and name.
  useEffect(() => {
    if (isNew && domain.project_id && !domain.root) {
      const p = projects.find((x) => x.id === domain.project_id)
      if (p) onProjectNameChange(p.name)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [domain])

  const set = (patch: Partial<Domain>) => setD({ ...d, ...patch })

  const onDomainChange = (val: string) => {
    setDomainTouched(true)
    if (isNew && !d.project_id && !rootTouched) {
      const folder = domainToFolderName(val)
      const newRoot = folder && parentDir ? buildSitePath(parentDir, folder) : ''
      setD((cur) => ({ ...cur, hostname: val, root: newRoot }))
    } else {
      set({ hostname: val })
    }
  }

  const onRootChange = (val: string) => {
    setRootTouched(val.trim().length > 0)
    set({ root: val })
  }

  async function browseRoot() {
    const picked = await open({ directory: true, title: 'Document root' })
    if (picked && !Array.isArray(picked)) {
      setRootTouched(true)
      set({ root: picked })
    }
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
  useEffect(() => {
    if (submitRef) submitRef.current = submit
  })

  const kinds: { id: 'php' | 'proxy' | 'static'; title: string; hint: string; icon: ReactNode }[] = [
    { id: 'php', title: 'PHP', hint: 'Laravel, WordPress, plain PHP (FastCGI)', icon: <Braces className="size-5" /> },
    { id: 'proxy', title: 'Reverse proxy', hint: 'Dev server, Docker, another computer', icon: <ArrowLeftRight className="size-5" /> },
    { id: 'static', title: 'Static files', hint: 'HTML, CSS and JS served as they are', icon: <FileText className="size-5" /> },
  ]
  const host = d.hostname.trim().toLowerCase()
  const localTld = /\.(test|local|localhost|dev\.test)$/.test(host)

  return (
    <div className="flex flex-col gap-5">
      {err && (
        <p role="alert" className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-[13px] text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" /> {err}
        </p>
      )}

      {isNew && (
        <FormSection title="Source" hint="Enter a project name to automatically fill the domain and folder, or create a new app.">
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Project (optional)" hint="Folder and domain fill automatically">
              <Input
                value={projectName}
                onChange={(e) => onProjectNameChange(e.target.value)}
                placeholder="e.g. my-app, blog, shop"
              />
            </Field>
            <Field label="Name template (§48)">
              <Select value={template} onChange={(e) => onTemplateChange(e.target.value)}>
                <option>{`{project}.${defaultTld}`}</option>
                <option>{`api.{project}.${defaultTld}`}</option>
                <option>{`admin.{project}.${defaultTld}`}</option>
              </Select>
            </Field>
          </div>
          {onQuickApp && (
            <div className="flex flex-col gap-2">
              <Button variant="secondary" size="sm" className="self-start" onClick={() => setShowApps((v) => !v)}>
                <Rocket /> {showApps ? 'Hide Quick Apps' : 'Create from a Quick App'}
              </Button>
              {showApps && (
                <div className="grid max-h-60 gap-2 overflow-y-auto rounded-lg border border-border p-2 sm:grid-cols-2">
                  {quickApps.length === 0 && <p className="p-2 text-sm text-muted-foreground">No Quick Apps available.</p>}
                  {quickApps.map((a) => (
                    <button
                      key={a.id}
                      onClick={() => onQuickApp(a.id)}
                      className="flex min-h-14 items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors hover:bg-accent"
                    >
                      <TechTile id={a.category} className="size-8 rounded-md [&_svg]:size-4" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium">{a.name}</span>
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
          <Input value={d.hostname} onChange={(e) => onDomainChange(e.target.value)} placeholder="shop.test, myapp.local, api.company.dev…" />
        </Field>
        <Field
          label="Site folder"
          hint={
            d.root
              ? `Will serve files from ${d.root}. Edit folder name or Browse different location.`
              : parentDir
                ? `Defaults to ${parentDir}\\<domain>. Edit folder name or Browse different location.`
                : 'Folder containing index.html, index.php or public assets.'
          }
        >
          <div className="flex gap-2">
            <Input
              value={d.root}
              onChange={(e) => onRootChange(e.target.value)}
              placeholder={parentDir ? buildSitePath(parentDir, 'shop') : 'C:\\Sites\\shop\\public'}
              className="min-w-0 flex-1"
            />
            <Button variant="secondary" onClick={browseRoot} className="shrink-0">
              <FolderSearch /> Browse
            </Button>
          </div>
        </Field>
        <Field
          label="Web server"
          hint={
            pickedServer
              ? pickedServer.default
                ? `Served by the default server, ${pickedServer.name}, on HTTP ${pickedServer.http} / HTTPS ${pickedServer.https}.`
                : `Pinned to ${pickedServer.name} on HTTP ${pickedServer.http} / HTTPS ${pickedServer.https}. This site is only rendered on that one server.`
              : 'The site is served by the default web server.'
          }
        >
          <div className="grid gap-2 sm:grid-cols-4" role="radiogroup" aria-label="Web server for this site">
            {webServers.map((s) => {
              // The default server's tile means "no override"; the rest pin this site.
              const isDefaultChoice = s.default
              const active = isDefaultChoice ? d.server === null || d.server === undefined : d.server === s.id
              return (
                <button
                  key={s.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => set({ server: isDefaultChoice ? null : s.id })}
                  className={`relative flex items-center gap-2 rounded-xl border p-2.5 text-left transition-all ${
                    active ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:border-foreground/20 hover:bg-accent/50'
                  }`}
                >
                  <TechTile id={s.id} className="size-7 rounded-md [&_svg]:size-4" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium">{isDefaultChoice ? 'Default' : s.name}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {isDefaultChoice ? s.name : `${s.http} / ${s.https}`}
                    </span>
                  </span>
                  {active && (
                    <span className="flex size-4 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground">
                      <Check className="size-2.5" />
                    </span>
                  )}
                </button>
              )
            })}
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
        <div className="grid gap-2.5 sm:grid-cols-3" role="radiogroup" aria-label="What the site serves">
          {kinds.map((k) => (
            <button
              key={k.id}
              type="button"
              role="radio"
              aria-checked={kindType === k.id}
              onClick={() => set({ kind: k.id === 'php' ? { type: 'php', version: null } : k.id === 'proxy' ? { type: 'proxy', upstream_port: 3000 } : { type: 'static' } })}
              className={cn(
                'flex min-h-28 flex-col items-start gap-1.5 rounded-lg border p-3 text-left transition-colors',
                kindType === k.id ? 'border-primary bg-accent text-accent-foreground ring-1 ring-primary/30' : 'border-border hover:bg-accent/40',
              )}
            >
              {k.icon}
              <span className="text-[13px] font-medium">{k.title}</span>
              <span className="text-xs leading-relaxed text-muted-foreground">{k.hint}</span>
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
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_7rem]">
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
        <div className="grid gap-2.5 sm:grid-cols-3">
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
      {actions && <div className="flex justify-end gap-2 border-t border-border pt-3">{actions(submit, busy)}</div>}
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
