import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import {
  type ApplyReport,
  type CaInfo,
  type CatalogEntry,
  type CertInfo,
  type DomainSummary,
  type Project,
  type WebConfig,
  type WebStatus,
  runCommand,
} from '@/core'
import { useAction, usePoll } from '@/lib/hooks'

/**
 * The web server's state and the "apply" step every site change ends with. Shared by the
 * Projects page (which lists sites) and the Web server page (server, ports, certificates).
 */
export function useWeb() {
  const [status, setStatus] = useState<WebStatus | null>(null)
  const [cfg, setCfg] = useState<WebConfig | null>(null)
  const [domains, setDomains] = useState<DomainSummary[]>([])
  const [certs, setCerts] = useState<CertInfo[]>([])
  const [ca, setCa] = useState<CaInfo | null>(null)
  const [projects, setProjects] = useState<Project[]>([])
  const [catalog, setCatalog] = useState<CatalogEntry[]>([])
  const [customPhp, setCustomPhp] = useState<string[]>([])
  const [report, setReport] = useState<ApplyReport | null>(null)
  const [driftOpen, setDriftOpen] = useState(false)
  const action = useAction()

  async function refresh() {
    const [s, c, d, k, a] = await Promise.all([
      runCommand({ type: 'get_web_status' }),
      runCommand({ type: 'get_web_config' }),
      runCommand({ type: 'list_domains' }),
      runCommand({ type: 'list_certificates' }),
      runCommand({ type: 'get_ca_info' }),
    ])
    if (s.type === 'web_status') setStatus(s.status)
    if (c.type === 'web_config') setCfg((prev) => prev ?? c.config)
    if (d.type === 'domains') setDomains(d.domains)
    if (k.type === 'certificates') setCerts(k.certs)
    if (a.type === 'ca_info') setCa(a.info)
  }

  async function refreshProjects() {
    const r = await runCommand({ type: 'list_projects' })
    if (r.type === 'projects') setProjects(r.projects)
  }

  usePoll(() => refresh().catch(() => undefined), 4000)
  useEffect(() => {
    let alive = true
    let unlisten: (() => void) | undefined
    void listen('projects-changed', () => {
      if (!alive) return
      void refreshProjects()
      void refresh()
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    void refreshProjects()
    // Laragon-style: new folders in your projects folder show up as <name>.test.
    runCommand({ type: 'sync_auto_domains' })
      .then((r) => {
        if (r.type === 'count' && r.count > 0) void refresh()
      })
      .catch(() => undefined)
    runCommand({ type: 'list_runtime_catalog' }).then((r) => r.type === 'runtime_catalog' && setCatalog(r.entries))
    // PHP versions registered from elsewhere (Laragon, XAMPP, ...) serve sites too.
    runCommand({ type: 'list_custom_installs' }).then(
      (r) => r.type === 'custom_installs' && setCustomPhp(r.entries.filter((c) => c.id === 'php' && c.label).map((c) => c.label)),
    )
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])

  async function apply(overwrite: string[] = []) {
    const res = await runCommand({ type: 'apply_web', overwrite })
    if (res.type === 'applied') {
      setReport(res.report)
      if (res.report.drifted.length > 0) setDriftOpen(true)
    }
    await refresh()
  }

  const installedPhp = [...new Set([...catalog.filter((c) => c.id === 'php' && c.installed).map((c) => c.version), ...customPhp])]

  return {
    ...action,
    status,
    cfg,
    domains,
    certs,
    ca,
    projects,
    installedPhp,
    report,
    setReport,
    driftOpen,
    setDriftOpen,
    refresh,
    refreshProjects,
    apply,
  }
}

export type Web = ReturnType<typeof useWeb>
