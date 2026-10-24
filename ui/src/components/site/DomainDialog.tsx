import { open } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeftRight,
  ArrowRight,
  Check,
  CircleAlert,
  FileCode2,
  FileText,
  FolderSearch,
  Globe,
  Loader2,
  RefreshCw,
  Rocket,
  Terminal,
} from 'lucide-react'
import { type ReactNode, useEffect, useRef, useState } from 'react'

import { TechIcon, TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, FormSection, Select, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { buildSitePath, domainToFolderName, getDefaultSitesDir, getDefaultTld, slugify } from '@/lib/sites'
import { type Domain, type Project, type QuickEntryView, type SiteFolderInspection, type SystemInstall, runCommand } from '@/core'

export type SiteServeMode = 'php' | 'node' | 'python' | 'proxy' | 'static'

export function getInitialServeMode(domain: Domain): SiteServeMode {
  if (domain.kind.type === 'php') return 'php'
  if (domain.kind.type === 'static') return 'static'
  if (domain.kind.type === 'proxy') {
    if (domain.app?.runtime === 'node') return 'node'
    if (domain.app?.runtime === 'python') return 'python'
    const exe = domain.app?.executable?.toLowerCase() || ''
    if (['node', 'npm', 'pnpm', 'yarn', 'bun'].some((k) => exe.includes(k))) return 'node'
    if (['python', 'py', 'uvicorn', 'flask', 'django', 'waitress', 'streamlit'].some((k) => exe.includes(k))) return 'python'
    return 'proxy'
  }
  return 'static'
}

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
    path_prefix: null,
    ssl_provider: 'local_ca',
    custom_cert_path: null,
    custom_key_path: null,
    lets_encrypt_email: null,
    auto_broadcast: false,
    broadcast_provider: 'cloudflare',
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
  onOpenDomainsTab,
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
  onOpenDomainsTab?: () => void
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
  /** Set once the user types a localhost prefix, so a later project-name change leaves it alone. */
  const [prefixTouched, setPrefixTouched] = useState(false)
  /** Set once the user types a project name, so a later projects-changed event leaves it alone. */
  const projectNameTouched = useRef(false)
  const [parentDir, setParentDir] = useState(defaultParent || '')
  // The configured sites directory, kept separately from `parentDir` so the folder
  // placeholder can name the real default location even when no parent was passed in
  // and the lookup has not answered yet.
  const [defaultSitesDir, setDefaultSitesDir] = useState('')
  const [serveMode, setServeMode] = useState<SiteServeMode>(() => getInitialServeMode(domain))
  const modeTouched = useRef(false)
  const [inspection, setInspection] = useState<SiteFolderInspection | null>(null)
  const [inspecting, setInspecting] = useState(false)
  const [installedNodeVersions, setInstalledNodeVersions] = useState<string[]>([])
  const [installedPythonVersions, setInstalledPythonVersions] = useState<string[]>([])
  const [systemNode, setSystemNode] = useState<SystemInstall | null>(null)
  const [systemPython, setSystemPython] = useState<SystemInstall | null>(null)
  const [nodePkgManager, setNodePkgManager] = useState<'npm' | 'pnpm' | 'yarn' | 'bun'>('npm')
  const [pythonPkgManager, setPythonPkgManager] = useState<'pip' | 'uv' | 'poetry' | 'pipenv'>('pip')
  const [nodeVersion, setNodeVersion] = useState<string>(() => {
    if (domain?.app?.runtime?.startsWith('node:')) return domain.app.runtime.slice(5)
    return ''
  })
  const [pythonInterpreter, setPythonInterpreter] = useState<string>(() => {
    if (domain?.app?.runtime?.startsWith('python:')) return `version:${domain.app.runtime.slice(7)}`
    if (domain?.app?.executable && (domain.app.executable.includes('/Scripts/') || domain.app.executable.includes('\\Scripts\\'))) {
      return `venv:${domain.app.executable}`
    }
    return 'system'
  })
  const [nodeRunOptChoice, setNodeRunOptChoice] = useState<string>('')
  const [pythonRunOptChoice, setPythonRunOptChoice] = useState<string>('')
  const [portChecking, setPortChecking] = useState(false)
  const initialAppLine = domain?.app ? [domain.app.executable, ...domain.app.args].join(' ') : ''
  const isInitialPython = domain?.app?.runtime?.startsWith('python') || ['python', 'py', 'uvicorn', 'flask', 'django', 'waitress', 'streamlit'].some((k) => domain?.app?.executable?.toLowerCase().includes(k))
  const [nodeAppLine, setNodeAppLine] = useState(!isInitialPython ? (initialAppLine || 'npm run dev') : 'npm run dev')
  const [pythonAppLine, setPythonAppLine] = useState(isInitialPython ? (initialAppLine || 'python main.py') : 'python main.py')
  const [defaultTld, setDefaultTld] = useState('local')
  const [template, setTemplate] = useState(`{project}.local`)
  const [showApps, setShowApps] = useState(false)
  const [quickApps, setQuickApps] = useState<QuickEntryView[]>([])
  const [webServers, setWebServers] = useState<{ id: string; name: string; http: number; https: number; default: boolean }[]>([])
  const [err, setErr] = useState<string | null>(null)

  // Re-seed the form only when the domain being edited actually changes.
  useEffect(() => {
    setD(domain)
    setErr(null)
    setShowApps(false)
    setRootTouched(false)
    setDomainTouched(false)
    setPrefixTouched(!!domain.path_prefix)
    projectNameTouched.current = false
    setServeMode(getInitialServeMode(domain))
    modeTouched.current = false
    const line = domain?.app ? [domain.app.executable, ...domain.app.args].join(' ') : ''
    const isPy = domain?.app?.runtime?.startsWith('python') || ['python', 'py', 'uvicorn', 'flask', 'django', 'waitress', 'streamlit'].some((k) => domain?.app?.executable?.toLowerCase().includes(k))
    if (isPy) {
      setPythonAppLine(line || 'python main.py')
      if (domain?.app?.runtime?.startsWith('python:')) setPythonInterpreter(`version:${domain.app.runtime.slice(7)}`)
    } else {
      setNodeAppLine(line || 'npm run dev')
      if (domain?.app?.runtime?.startsWith('node:')) setNodeVersion(domain.app.runtime.slice(5))
    }
    if (!domain.project_id) setProjectName('')
  }, [domain])

  useEffect(() => {
    void runCommand({ type: 'list_runtime_catalog' }).then((r) => {
      if (r.type === 'runtime_catalog') {
        setInstalledNodeVersions(r.entries.filter((e) => e.id === 'node' && e.installed).map((e) => e.version))
        setInstalledPythonVersions(r.entries.filter((e) => e.id === 'python' && e.installed).map((e) => e.version))
        const pySys = r.entries.find((e) => e.id === 'python')?.system
        if (pySys) setSystemPython(pySys)
        const nodeSys = r.entries.find((e) => e.id === 'node')?.system
        if (nodeSys) setSystemNode(nodeSys)
      }
    })
  }, [])

  useEffect(() => {
    const rootPath = d.root?.trim()
    if (!rootPath || rootPath.length < 2) {
      setInspection(null)
      return
    }
    let cancelled = false
    setInspecting(true)
    const timer = setTimeout(() => {
      void runCommand({ type: 'inspect_site_folder', path: rootPath })
        .then((res) => {
          if (cancelled || res.type !== 'site_folder_inspection') return
          const info = res.inspection
          setInspection(info)
          setInspecting(false)

          if (isNew && !modeTouched.current) {
            const isPython =
              ['django', 'flask', 'fast_api', 'generic_python'].includes(info.framework) ||
              info.entry_files.some((f) => f.endsWith('.py')) ||
              !!info.detected_venv ||
              (info.available_venvs && info.available_venvs.length > 0) ||
              (info.requirements_files && info.requirements_files.length > 0)

            if (isPython) {
              setServeMode('python')
              if (info.detected_package_manager && ['pip', 'uv', 'poetry', 'pipenv'].includes(info.detected_package_manager)) {
                setPythonPkgManager(info.detected_package_manager as 'pip' | 'uv' | 'poetry' | 'pipenv')
              }
              const pyCmd = info.suggested_command && info.suggested_command.toLowerCase().includes('python')
                ? info.suggested_command
                : 'python main.py'
              setPythonAppLine(pyCmd)
              if (info.available_venvs && info.available_venvs.length > 0) {
                setPythonInterpreter(`venv:${info.available_venvs[0].python_exe}`)
              } else if (info.detected_venv) {
                setPythonInterpreter('venv')
              }
              if (info.suggested_port) {
                setD((cur) => ({
                  ...cur,
                  kind: { type: 'proxy', upstream_port: info.suggested_port, upstream_host: null, upstream_https: false },
                }))
              }
            } else if (info.framework === 'node' || info.package_scripts.length > 0) {
              setServeMode('node')
              if (info.detected_package_manager && ['npm', 'pnpm', 'yarn', 'bun'].includes(info.detected_package_manager)) {
                setNodePkgManager(info.detected_package_manager as 'npm' | 'pnpm' | 'yarn' | 'bun')
              }
              const nodeCmd = info.suggested_command && !info.suggested_command.toLowerCase().includes('python')
                ? info.suggested_command
                : (info.package_scripts.some((s) => s.name === 'dev') ? 'npm run dev' : 'npm start')
              setNodeAppLine(nodeCmd)
              if (info.suggested_port) {
                setD((cur) => ({
                  ...cur,
                  kind: { type: 'proxy', upstream_port: info.suggested_port, upstream_host: null, upstream_https: false },
                }))
              }
            } else if (['laravel', 'symfony', 'word_press', 'generic_php'].includes(info.framework)) {
              setServeMode('php')
            }
          } else {
            if (info.detected_package_manager) {
              if (['npm', 'pnpm', 'yarn', 'bun'].includes(info.detected_package_manager)) {
                setNodePkgManager(info.detected_package_manager as 'npm' | 'pnpm' | 'yarn' | 'bun')
              } else if (['pip', 'uv', 'poetry', 'pipenv'].includes(info.detected_package_manager)) {
                setPythonPkgManager(info.detected_package_manager as 'pip' | 'uv' | 'poetry' | 'pipenv')
              }
            }
            if (info.available_venvs && info.available_venvs.length > 0) {
              setPythonInterpreter(`venv:${info.available_venvs[0].python_exe}`)
            } else if (info.detected_venv) {
              setPythonInterpreter('venv')
            }
          }
        })
        .catch(() => {
          if (!cancelled) setInspecting(false)
        })
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [d.root, isNew])

  async function findNextFreePort(startPort: number) {
    setPortChecking(true)
    try {
      for (let p = startPort; p < startPort + 40; p++) {
        const res = await runCommand({ type: 'check_port', port: p })
        if (res.type === 'port' && res.status.status === 'free') {
          setD((cur) => ({
            ...cur,
            kind: {
              ...(cur.kind.type === 'proxy' ? cur.kind : { type: 'proxy', upstream_host: null, upstream_https: false }),
              type: 'proxy',
              upstream_port: p,
            },
          }))
          break
        }
      }
    } finally {
      setPortChecking(false)
    }
  }

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
    void getDefaultSitesDir().then((p) => {
      if (!p) return
      setDefaultSitesDir(p)
      setParentDir((cur) => cur || p)
    })
  }, [])

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

    // Auto-fill localhost/<prefix> from the same slug, until the user types one.
    // Capped at 40 and restricted to the characters `validate_path_prefix` accepts,
    // so the autofill can never produce a prefix the backend would then refuse.
    const newPrefix = !prefixTouched
      ? (slug && slug.length <= 40 ? slug : null)
      : (d.path_prefix ?? null)

    setD((cur) => ({
      ...cur,
      hostname: newHost,
      root: newRoot,
      path_prefix: newPrefix,
      project_id: matched ? matched.id : null,
    }))

    if (matched && !rootTouched) {
      // Best-effort autofill: the form stays usable if the read fails, so the failure is
      // swallowed on purpose — but it must be swallowed here, not left to reject unhandled.
      void runCommand({ type: 'get_project_detail', id: matched.id })
        .then((detail) => {
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
        .catch(() => undefined)
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

  const onSelectMode = (m: SiteServeMode) => {
    modeTouched.current = true
    setServeMode(m)
    if (m === 'php') {
      setD((cur) => ({ ...cur, kind: { type: 'php', version: cur.kind.type === 'php' ? cur.kind.version : null } }))
    } else if (m === 'node') {
      const port = d.kind.type === 'proxy' ? d.kind.upstream_port : (inspection?.suggested_port || 3000)
      setD((cur) => ({ ...cur, kind: { type: 'proxy', upstream_port: port, upstream_host: null, upstream_https: false } }))
    } else if (m === 'python') {
      const port = d.kind.type === 'proxy' ? d.kind.upstream_port : (inspection?.suggested_port || 8000)
      setD((cur) => ({ ...cur, kind: { type: 'proxy', upstream_port: port, upstream_host: null, upstream_https: false } }))
    } else if (m === 'proxy') {
      setD((cur) => ({
        ...cur,
        kind: {
          type: 'proxy',
          upstream_port: cur.kind.type === 'proxy' ? cur.kind.upstream_port : 3000,
          upstream_host: cur.kind.type === 'proxy' ? cur.kind.upstream_host : null,
          upstream_https: cur.kind.type === 'proxy' ? cur.kind.upstream_https : false,
        },
      }))
    } else if (m === 'static') {
      setD((cur) => ({ ...cur, kind: { type: 'static' } }))
    }
  }

  const onNodeRunOptChange = (opt: string) => {
    setNodeRunOptChoice(opt)
    if (!opt || opt === 'custom') return
    if (opt.startsWith('script:')) {
      const scriptName = opt.replace('script:', '')
      if (nodePkgManager === 'npm') {
        setNodeAppLine(scriptName === 'start' ? 'npm start' : `npm run ${scriptName}`)
      } else {
        setNodeAppLine(`${nodePkgManager} ${scriptName}`)
      }
    } else if (opt === 'preset:dev') {
      setNodeAppLine(nodePkgManager === 'npm' ? 'npm run dev' : `${nodePkgManager} dev`)
    } else if (opt === 'preset:start') {
      setNodeAppLine(nodePkgManager === 'npm' ? 'npm start' : `${nodePkgManager} start`)
    } else if (opt === 'preset:server_js') {
      setNodeAppLine('node server.js')
    } else if (opt === 'preset:index_js') {
      setNodeAppLine('node index.js')
    } else if (opt === 'preset:pnpm_dev') {
      setNodePkgManager('pnpm')
      setNodeAppLine('pnpm dev')
    } else if (opt === 'preset:yarn_dev') {
      setNodePkgManager('yarn')
      setNodeAppLine('yarn dev')
    } else if (opt === 'preset:bun_dev') {
      setNodePkgManager('bun')
      setNodeAppLine('bun run dev')
    }
  }

  const onNodePkgManagerChange = (pm: 'npm' | 'pnpm' | 'yarn' | 'bun') => {
    setNodePkgManager(pm)
    setNodeAppLine((cur) => {
      if (cur.startsWith('npm run ')) return cur.replace('npm run ', `${pm} `)
      if (cur.startsWith('pnpm ')) return cur.replace('pnpm ', `${pm} `)
      if (cur.startsWith('yarn ')) return cur.replace('yarn ', `${pm} `)
      if (cur.startsWith('bun run ')) return cur.replace('bun run ', `${pm} `)
      if (cur === 'npm start') return `${pm} start`
      return cur
    })
  }

  const onPythonRunOptChange = (opt: string) => {
    setPythonRunOptChoice(opt)
    if (!opt || opt === 'custom') return
    if (opt === 'preset:main_py') {
      setPythonAppLine('python main.py')
    } else if (opt === 'preset:app_py') {
      setPythonAppLine('python app.py')
    } else if (opt === 'preset:server_py') {
      setPythonAppLine('python server.py')
    } else if (opt === 'preset:fastapi') {
      setPythonAppLine('python -m uvicorn main:app --reload')
    } else if (opt === 'preset:flask') {
      setPythonAppLine('python -m flask run')
    } else if (opt === 'preset:django') {
      setPythonAppLine('python manage.py runserver')
    } else if (opt === 'preset:streamlit') {
      setPythonAppLine('streamlit run app.py')
    }
  }

  function submit() {
    const out = { ...d, hostname: d.hostname.trim().toLowerCase() }
    if (serveMode === 'php') {
      out.kind = d.kind.type === 'php' ? d.kind : { type: 'php', version: null }
      out.app = null
    } else if (serveMode === 'static') {
      out.kind = { type: 'static' }
      out.app = null
    } else if (serveMode === 'node') {
      const port = d.kind.type === 'proxy' && d.kind.upstream_port ? d.kind.upstream_port : 3000
      out.kind = { type: 'proxy', upstream_port: port, upstream_host: null, upstream_https: false }
      if (nodeAppLine.trim()) {
        const [executable, ...args] = splitArgs(nodeAppLine)
        out.app = {
          executable,
          args,
          cwd: out.app?.cwd || out.root,
          runtime: nodeVersion ? `node:${nodeVersion}` : 'node',
        }
      } else {
        out.app = null
      }
    } else if (serveMode === 'python') {
      const port = d.kind.type === 'proxy' && d.kind.upstream_port ? d.kind.upstream_port : 8000
      out.kind = { type: 'proxy', upstream_port: port, upstream_host: null, upstream_https: false }
      if (pythonAppLine.trim()) {
        const [executable, ...args] = splitArgs(pythonAppLine)
        let finalExe = executable
        let runtimeField = 'python'
        if (pythonInterpreter.startsWith('version:')) {
          runtimeField = `python:${pythonInterpreter.slice(8)}`
        } else if (pythonInterpreter.startsWith('venv:') || pythonInterpreter === 'venv') {
          const venvPath = pythonInterpreter.startsWith('venv:')
            ? pythonInterpreter.slice(5)
            : inspection?.detected_venv
          if (venvPath && (executable.toLowerCase() === 'python' || executable.toLowerCase() === 'python.exe')) {
            finalExe = venvPath
          }
        }
        out.app = {
          executable: finalExe,
          args,
          cwd: out.app?.cwd || out.root,
          runtime: runtimeField,
        }
      } else {
        out.app = null
      }
    } else if (serveMode === 'proxy') {
      const port = d.kind.type === 'proxy' && d.kind.upstream_port ? d.kind.upstream_port : 3000
      const host = d.kind.type === 'proxy' ? d.kind.upstream_host : null
      const https = d.kind.type === 'proxy' ? d.kind.upstream_https : false
      out.kind = { type: 'proxy', upstream_port: port, upstream_host: host, upstream_https: https }
      if (!host && nodeAppLine.trim()) {
        const [executable, ...args] = splitArgs(nodeAppLine)
        out.app = {
          executable,
          args,
          cwd: out.app?.cwd || out.root,
          runtime: out.app?.runtime ?? null,
        }
      } else {
        out.app = null
      }
    }

    if (!out.https) out.redirect_https = false
    if (!out.hostname) return setErr('Enter a domain like shop.test or mydomain.com')
    if (pathPrefixError) return setErr(pathPrefixError)
    if (!out.root) return setErr('Choose the site folder')
    if (out.public_domain && !out.tunnel_id) return setErr('Choose a saved named Cloudflare tunnel for the public domain')
    onSave(out)
  }
  useEffect(() => {
    if (submitRef) submitRef.current = submit
  })

  const kinds: { id: SiteServeMode; title: string; hint: string; icon: ReactNode }[] = [
    { id: 'php', title: 'PHP', hint: 'Laravel, WordPress, Symfony (FastCGI pool)', icon: <TechIcon id="php" className="size-4" /> },
    { id: 'node', title: 'Node.js', hint: 'Next.js, Vite, Express, Nest, Fastify dev server', icon: <TechIcon id="node" className="size-4" /> },
    { id: 'python', title: 'Python', hint: 'Python script (main.py), FastAPI (Uvicorn), Django, Flask', icon: <TechIcon id="python" className="size-4" /> },
    { id: 'proxy', title: 'Reverse proxy', hint: 'Forward traffic to Docker, another local port, or remote IP', icon: <ArrowLeftRight className="size-4" /> },
    { id: 'static', title: 'Static files', hint: 'Directly serve HTML, CSS, JS with zero background process', icon: <FileText className="size-4" /> },
  ]
  const host = d.hostname.trim().toLowerCase()
  const localTld = /\.(test|local|localhost|dev\.test)$/.test(host)
  // Mirrors validate_path_prefix in ols-core: the same value lands as a URL path in a
  // generated config, so it is one segment of safe characters and nothing that could
  // escape the prefix. Checked here so the reason shows next to the field, and again in
  // Rust because this one is only a convenience.
  const rawPrefix = (d.path_prefix ?? '').trim()
  const pathPrefixError =
    rawPrefix === '' || /^[a-z0-9_-]{1,40}$/.test(rawPrefix)
      ? undefined
      : rawPrefix.length > 40
        ? 'Keep it under 40 characters.'
        : rawPrefix !== rawPrefix.toLowerCase()
          ? 'Use lowercase letters, digits, "-" and "_" only.'
          : 'One path segment: no "/", spaces or punctuation.'

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
            <Field label="Project (optional)" hint="Folder, domain and localhost path fill automatically">
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
              ? localTld
                ? `Local development domain (${d.https ? 'https' : 'http'}://${host})`
                : "Real public domain! You can secure it with Let's Encrypt or custom SSL and broadcast it online."
              : 'Enter a local domain (shop.test, myapp.local) or a real public domain (example.com, api.site.org)'
          }
        >
          <Input value={d.hostname} onChange={(e) => onDomainChange(e.target.value)} placeholder="shop.test, myapp.local, api.company.dev, mysite.com…" />
        </Field>
        <Field
          label="Site folder"
          hint={
            d.root
              ? isNew && d.kind.type !== 'proxy'
                ? `Will serve files from ${d.root}. The folder and welcome page will be created automatically if they don't exist.`
                : `Will serve files from ${d.root}. Edit folder name or Browse different location.`
              : parentDir
                ? `Defaults to ${parentDir}\\<domain>. Edit folder name or Browse different location.`
                : 'Folder containing index.html, index.php or public assets.'
          }
        >
          <div className="flex gap-2">
            <Input
              value={d.root}
              onChange={(e) => onRootChange(e.target.value)}
              placeholder={(() => {
                // The placeholder is the path this dialog would actually write: the
                // default sites directory (or the parent the caller chose) plus a sample
                // folder. It used to fall back to a hardcoded `C:\Sites\shop\public`,
                // which named a location the app never uses.
                const base = parentDir || defaultSitesDir
                return base ? buildSitePath(base, 'shop') : ''
              })()}
              className="min-w-0 flex-1"
            />
            <Button variant="secondary" onClick={browseRoot} className="shrink-0">
              <FolderSearch /> Browse
            </Button>
          </div>
        </Field>
        <Field
          label="Also serve at localhost/…"
          hint={
            d.path_prefix
              ? `In addition to ${d.hostname || 'its domain'}, this site answers at http://localhost:${httpPort}/${d.path_prefix}/ — the same app, same PHP, same proxy target.`
              : 'Optional. One path segment, like "shop" for http://localhost/shop/. Fills from the project name until you type one. Leave empty for domain-only.'
          }
          error={pathPrefixError}
        >
          <div className="flex items-center gap-2">
            <span className="shrink-0 font-mono text-xs text-muted-foreground">localhost/{httpPort}/</span>
            <Input
              value={d.path_prefix ?? ''}
              onChange={(e) => {
                setPrefixTouched(true)
                set({ path_prefix: e.target.value })
              }}
              placeholder="shop"
              className="min-w-0 flex-1 font-mono"
            />
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



      <FormSection title="Serves">
        <div className="flex flex-col gap-2.5">
          <div className="flex p-1 bg-muted/60 rounded-xl border border-border/80 gap-1 select-none" role="tablist" aria-label="What the site serves">
            {kinds.map((k) => {
              const active = serveMode === k.id
              return (
                <button
                  key={k.id}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => onSelectMode(k.id)}
                  className={cn(
                    'flex-1 flex items-center justify-center gap-2 py-2 px-3 rounded-lg text-xs font-medium transition-all cursor-pointer',
                    active
                      ? 'bg-background text-foreground shadow-sm border border-border/70 ring-1 ring-primary/20 font-semibold'
                      : 'text-muted-foreground hover:text-foreground hover:bg-background/40',
                  )}
                >
                  <span className="shrink-0 flex items-center justify-center [&_svg]:size-4">{k.icon}</span>
                  <span>{k.title}</span>
                </button>
              )
            })}
          </div>

          <div className="flex items-center justify-between px-3.5 py-2 rounded-lg bg-accent/25 border border-border/60 text-xs">
            <div className="flex items-center gap-2 text-muted-foreground">
              <span className="font-semibold text-foreground">{kinds.find((k) => k.id === serveMode)?.title}:</span>
              <span>{kinds.find((k) => k.id === serveMode)?.hint}</span>
            </div>
            {inspection?.framework && inspection.framework !== 'unknown' && (
              <span className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 text-[11px] font-medium text-emerald-400 border border-emerald-500/20">
                Detected: {inspection.framework.replace(/_/g, ' ')}
              </span>
            )}
          </div>
        </div>

        {inspecting && (
          <div className="flex items-center gap-2 rounded-md border border-border/60 bg-muted/30 px-3 py-1.5 text-xs text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin text-primary" />
            <span>Inspecting project files and startup scripts…</span>
          </div>
        )}

        {serveMode === 'php' && (
          <Field label="PHP version" hint="“Project default” follows the project's own resolved version">
            <Select value={d.kind.type === 'php' ? (d.kind.version ?? '') : ''} onChange={(e) => set({ kind: { type: 'php', version: e.target.value || null } })}>
              <option value="">Project default / newest</option>
              {phpOptions(installedPhp, d.kind.type === 'php' ? d.kind.version : null).map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </Select>
          </Field>
        )}

        {serveMode === 'node' && (
          <div className="flex flex-col gap-3 rounded-lg border border-border/80 bg-accent/10 p-3.5">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field
                label="Run opt (Startup Mode)"
                hint={
                  inspection?.package_scripts?.length
                    ? `Found ${inspection.package_scripts.length} script(s) in package.json.`
                    : 'Choose a startup mode or pick [Custom Command].'
                }
              >
                <Select value={nodeRunOptChoice} onChange={(e) => onNodeRunOptChange(e.target.value)}>
                  <option value="">-- Choose startup script / preset --</option>
                  {inspection?.package_scripts && inspection.package_scripts.length > 0 && (
                    <optgroup label="Scripts in package.json">
                      {inspection.package_scripts.map((s) => (
                        <option key={s.name} value={`script:${s.name}`}>
                          {s.name} {s.description ? `[${s.description}]` : ''}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  <optgroup label="Common Presets">
                    <option value="preset:dev">npm run dev (Next.js / Vite / Nuxt)</option>
                    <option value="preset:start">npm start (Production)</option>
                    <option value="preset:server_js">node server.js</option>
                    <option value="preset:index_js">node index.js</option>
                    <option value="preset:pnpm_dev">pnpm dev</option>
                    <option value="preset:yarn_dev">yarn dev</option>
                    <option value="preset:bun_dev">bun run dev</option>
                  </optgroup>
                  <option value="custom">[Custom Command]</option>
                </Select>
              </Field>

              <Field label="Package Manager" hint="Please select the package manager for this project">
                <Select value={nodePkgManager} onChange={(e) => onNodePkgManagerChange(e.target.value as any)}>
                  <option value="npm">npm</option>
                  <option value="pnpm">pnpm</option>
                  <option value="yarn">yarn</option>
                  <option value="bun">bun</option>
                </Select>
              </Field>
            </div>

            <Field label="Start command" hint="OLS supervises this process and passes PORT. Example: npm run dev">
              <div className="flex items-center gap-2">
                <Terminal className="size-4 shrink-0 text-muted-foreground" />
                <Input
                  value={nodeAppLine}
                  onChange={(e) => {
                    setNodeAppLine(e.target.value)
                    setNodeRunOptChoice('custom')
                  }}
                  placeholder="npm run dev"
                  className="font-mono text-xs"
                />
              </div>
            </Field>

            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Port of the project" hint="OLS web server reverse-proxies domain traffic to this port">
                <div className="flex gap-2">
                  <Input
                    type="number"
                    value={d.kind.type === 'proxy' ? d.kind.upstream_port : 3000}
                    onChange={(e) =>
                      set({
                        kind: {
                          type: 'proxy',
                          upstream_port: Number(e.target.value) || 3000,
                          upstream_host: null,
                          upstream_https: false,
                        },
                      })
                    }
                    className="font-mono"
                  />
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={portChecking}
                    onClick={() => findNextFreePort(d.kind.type === 'proxy' ? d.kind.upstream_port : 3000)}
                    className="shrink-0 gap-1 text-xs"
                  >
                    <RefreshCw className={cn('size-3', portChecking && 'animate-spin')} />
                    Find free
                  </Button>
                </div>
              </Field>

              <Field
                label="Node Version"
                hint={
                  nodeVersion
                    ? `Using OLS managed Node.js v${nodeVersion}.`
                    : systemNode?.version
                      ? `Using system Node.js (${systemNode.version}) from PATH (${systemNode.path}).`
                      : 'Choose the Node runtime version'
                }
              >
                <Select
                  value={nodeVersion}
                  onChange={(e) => setNodeVersion(e.target.value)}
                >
                  <option value="">
                    System Node.js ({systemNode?.version ? `PATH - Node.js ${systemNode.version.replace(/^v/i, '')}` : 'PATH'})
                  </option>
                  {installedNodeVersions.map((v) => (
                    <option key={v} value={v}>
                      Node.js v{v} (OLS installed)
                    </option>
                  ))}
                </Select>
              </Field>
            </div>

            <Toggle
              checked={d.enabled}
              onChange={(v) => set({ enabled: v })}
              label="Follow the system to start the service"
              hint="Supervise and automatically start this Node.js server with OpenLocalServer."
            />
          </div>
        )}

        {serveMode === 'python' && (
          <div className="flex flex-col gap-3 rounded-lg border border-border/80 bg-accent/10 p-3.5">
            {inspection?.requirements_files && inspection.requirements_files.length > 0 && (
              <div className="flex items-center gap-1.5 rounded-md border border-border/60 bg-background/60 px-2.5 py-1.5 text-xs text-muted-foreground">
                <FileText className="size-3.5 shrink-0 text-primary" />
                <span>Detected dependency manifest:</span>
                <span className="font-mono font-medium text-foreground">{inspection.requirements_files.join(', ')}</span>
              </div>
            )}

            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Run opt (Startup Mode)" hint="Select standard Python script or framework">
                <Select value={pythonRunOptChoice} onChange={(e) => onPythonRunOptChange(e.target.value)}>
                  <option value="">-- Choose startup command --</option>
                  <optgroup label="Standard Python Scripts">
                    <option value="preset:main_py">python main.py</option>
                    <option value="preset:app_py">python app.py</option>
                    <option value="preset:server_py">python server.py</option>
                  </optgroup>
                  <optgroup label="Frameworks (FastAPI, Flask, Django)">
                    <option value="preset:fastapi">FastAPI: python -m uvicorn main:app --reload</option>
                    <option value="preset:django">Django: python manage.py runserver</option>
                    <option value="preset:flask">Flask: python -m flask run</option>
                    <option value="preset:streamlit">Streamlit: streamlit run app.py</option>
                  </optgroup>
                  <option value="custom">[Custom Command]</option>
                </Select>
              </Field>

              <Field label="Package Manager" hint="Virtual environment / dependency tool">
                <Select value={pythonPkgManager} onChange={(e) => setPythonPkgManager(e.target.value as any)}>
                  <option value="pip">pip (requirements.txt)</option>
                  <option value="uv">uv</option>
                  <option value="poetry">poetry</option>
                  <option value="pipenv">pipenv</option>
                </Select>
              </Field>
            </div>

            <Field label="Start command" hint="OLS supervises this process and passes PORT. Example: python main.py">
              <div className="flex items-center gap-2">
                <Terminal className="size-4 shrink-0 text-muted-foreground" />
                <Input
                  value={pythonAppLine}
                  onChange={(e) => {
                    setPythonAppLine(e.target.value)
                    setPythonRunOptChoice('custom')
                  }}
                  placeholder="python main.py"
                  className="font-mono text-xs"
                />
              </div>
            </Field>

            <div className="grid gap-3 sm:grid-cols-2">
              <Field label="Port of the project" hint="OLS web server reverse-proxies domain traffic to this port">
                <div className="flex gap-2">
                  <Input
                    type="number"
                    value={d.kind.type === 'proxy' ? d.kind.upstream_port : 8000}
                    onChange={(e) =>
                      set({
                        kind: {
                          type: 'proxy',
                          upstream_port: Number(e.target.value) || 8000,
                          upstream_host: null,
                          upstream_https: false,
                        },
                      })
                    }
                    className="font-mono"
                  />
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={portChecking}
                    onClick={() => findNextFreePort(d.kind.type === 'proxy' ? d.kind.upstream_port : 8000)}
                    className="shrink-0 gap-1 text-xs"
                  >
                    <RefreshCw className={cn('size-3', portChecking && 'animate-spin')} />
                    Find free
                  </Button>
                </div>
              </Field>

              <Field
                label="Python Interpreter / Environment"
                hint={
                  pythonInterpreter.startsWith('venv:') || pythonInterpreter === 'venv'
                    ? 'Using selected virtualenv. Its Scripts directory will be set first on PATH and VIRTUAL_ENV is set.'
                    : pythonInterpreter.startsWith('version:')
                      ? `Using OLS managed Python v${pythonInterpreter.slice(8)}.`
                      : systemPython?.version
                        ? `Using system Python (${systemPython.version.replace(/^Python\s*/i, '')}) from PATH (${systemPython.path}).`
                        : 'Using system Python from PATH.'
                }
              >
                <Select value={pythonInterpreter} onChange={(e) => setPythonInterpreter(e.target.value)}>
                  {inspection?.available_venvs && inspection.available_venvs.length > 0 && (
                    <optgroup label="Detected Virtual Environments">
                      {inspection.available_venvs.map((v) => (
                        <option key={v.path} value={`venv:${v.python_exe}`}>
                          {v.name} ({v.version ? `Python ${v.version}` : 'venv'}) - {v.path}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  <optgroup label="System / Global Python">
                    <option value="system">
                      System Python ({systemPython?.version ? `PATH - Python ${systemPython.version.replace(/^Python\s*/i, '')}` : 'PATH'})
                    </option>
                  </optgroup>
                  {installedPythonVersions.length > 0 && (
                    <optgroup label="OLS Managed Runtimes">
                      {installedPythonVersions.map((v) => (
                        <option key={v} value={`version:${v}`}>
                          Python v{v} (OLS installed)
                        </option>
                      ))}
                    </optgroup>
                  )}
                </Select>
              </Field>
            </div>

            <Toggle
              checked={d.enabled}
              onChange={(v) => set({ enabled: v })}
              label="Follow the system to start the service"
              hint="Supervise and automatically start this Python server with OpenLocalServer."
            />
          </div>
        )}

        {serveMode === 'proxy' && (
          <>
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_7rem]">
              <Field label="Forward to" hint="Blank = this computer. Or a Docker host, another PC's IP, a hostname.">
                <Input
                  value={d.kind.type === 'proxy' ? (d.kind.upstream_host ?? '') : ''}
                  onChange={(e) =>
                    set({
                      kind: {
                        type: 'proxy',
                        upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000,
                        upstream_host: e.target.value.trim() || null,
                        upstream_https: d.kind.type === 'proxy' ? d.kind.upstream_https : false,
                      },
                    })
                  }
                  placeholder="127.0.0.1"
                />
              </Field>
              <Field label="Port">
                <Input
                  type="number"
                  value={d.kind.type === 'proxy' ? d.kind.upstream_port : 3000}
                  onChange={(e) =>
                    set({
                      kind: {
                        type: 'proxy',
                        upstream_port: Number(e.target.value) || 3000,
                        upstream_host: d.kind.type === 'proxy' ? d.kind.upstream_host : null,
                        upstream_https: d.kind.type === 'proxy' ? d.kind.upstream_https : false,
                      },
                    })
                  }
                />
              </Field>
            </div>
            <Toggle
              checked={d.kind.type === 'proxy' ? !!d.kind.upstream_https : false}
              onChange={(v) =>
                set({
                  kind: {
                    type: 'proxy',
                    upstream_port: d.kind.type === 'proxy' ? d.kind.upstream_port : 3000,
                    upstream_host: d.kind.type === 'proxy' ? d.kind.upstream_host : null,
                    upstream_https: v,
                  },
                })
              }
              label="The target only speaks HTTPS"
              hint="Self-signed certificates on the target are accepted."
            />
            {!(d.kind.type === 'proxy' && d.kind.upstream_host) && (
              <Field label="Start command (optional)" hint="OLS supervises it and passes PORT. Example: npm run dev">
                <Input value={nodeAppLine} onChange={(e) => setNodeAppLine(e.target.value)} placeholder="npm run dev" />
              </Field>
            )}
          </>
        )}

        {serveMode === 'static' && (
          <p className="rounded-md border border-border/60 bg-muted/20 p-3 text-xs text-muted-foreground">
            Serves static HTML, CSS, JavaScript, and asset files directly from the site folder with zero background process overhead.
          </p>
        )}
      </FormSection>

      <FormSection title="HTTPS & TLS" hint="Configure local HTTPS encryption for this site.">
        <div className="grid gap-2.5 sm:grid-cols-3">
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.https} onChange={(v) => set({ https: v, redirect_https: v ? d.redirect_https : false })} label="HTTPS" hint="Encrypt connections with SSL/TLS." />
          </div>
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.redirect_https} disabled={!d.https} onChange={(v) => set({ redirect_https: v })} label="Redirect to HTTPS" hint="Turn off for projects that need plain HTTP." />
          </div>
          <div className="rounded-lg border border-border p-3">
            <Toggle checked={d.wildcard} onChange={(v) => set({ wildcard: v })} label="Wildcard" hint={`Answers *.${host || 'domain'}; needs a one-time Windows DNS rule.`} />
          </div>
        </div>

        <div
          className={cn(
            'mt-3 flex items-center justify-between rounded-lg border border-border/70 bg-accent/20 px-3.5 py-2.5 text-xs transition-colors',
            onOpenDomainsTab && 'hover:border-primary/50 hover:bg-accent/30 cursor-pointer'
          )}
          onClick={onOpenDomainsTab}
        >
          <div className="flex items-center gap-2 text-muted-foreground">
            <Globe className="size-4 text-sky-400 shrink-0" />
            <span>Need extra domain aliases, Dynamic DNS (Cloudflare, GoDaddy, Namecheap), Let's Encrypt, or Custom SSL files?</span>
          </div>
          {onOpenDomainsTab ? (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={(e) => {
                e.stopPropagation()
                onOpenDomainsTab()
              }}
              className="h-7 shrink-0 gap-1.5 px-2.5 text-xs font-medium cursor-pointer"
            >
              <span>See Domains & DNS tab</span>
              <ArrowRight className="size-3 text-muted-foreground" />
            </Button>
          ) : (
            <span className="font-medium text-foreground shrink-0 ml-3">See Domains & DNS tab</span>
          )}
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
