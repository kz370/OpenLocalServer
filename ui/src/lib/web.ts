import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import {
  type ApplyReport,
  type CaInfo,
  type CatalogEntry,
  type CertInfo,
  type CoreCommand,
  type DomainSummary,
  type Project,
  type WebConfig,
  type WebStatus,
  runCommand,
} from '@/core'
import { useAction, usePoll } from '@/lib/hooks'

/** What a bulk site edit did: how many sites changed, and one line per site it refused. */
export interface BulkOutcome {
  changed: number
  skipped: string[]
}

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
  const [reports, setReports] = useState<ApplyReport[]>([])
  const [driftOpen, setDriftOpen] = useState(false)
  /** Hostnames and project ids whose row is mid-removal, so the page can show it. */
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set())
  const [applying, setApplying] = useState(false)
  const action = useAction()

  const markRemoving = (key: string, on = true) =>
    setRemoving((current) => {
      const next = new Set(current)
      if (on) next.add(key)
      else next.delete(key)
      return next
    })

  async function applyTracked() {
    setApplying(true)
    try {
      await apply()
    } finally {
      setApplying(false)
    }
  }

  async function refresh(updateConfig = false) {
    const [s, c, d, k, a] = await Promise.all([
      runCommand({ type: 'get_web_status' }),
      runCommand({ type: 'get_web_config' }),
      runCommand({ type: 'list_domains' }),
      runCommand({ type: 'list_certificates' }),
      runCommand({ type: 'get_ca_info' }),
    ])
    if (s.type === 'web_status') setStatus(s.status)
    if (c.type === 'web_config') setCfg((prev) => (updateConfig || prev === null ? c.config : prev))
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
      void refreshProjects().catch(() => undefined)
      void refresh().catch(() => undefined)
    }).then((dispose) => {
      if (alive) unlisten = dispose
      else dispose()
    })
    void refreshProjects().catch(() => undefined)
    // Laragon-style: new folders in your projects folder show up as <name>.test.
    runCommand({ type: 'sync_auto_domains' })
      .then((r) => {
        if (r.type === 'count' && r.count > 0) void refresh()
      })
      .catch(() => undefined)
    // The catalog and the unmanaged PHP versions only decorate other pages, so a failed read
    // leaves them empty. It still has to be caught: an unhandled rejection here is silent.
    runCommand({ type: 'list_runtime_catalog' })
      .then((r) => r.type === 'runtime_catalog' && setCatalog(r.entries))
      .catch(() => undefined)
    runCommand({ type: 'list_custom_installs' })
      .then((r) => r.type === 'custom_installs' && setCustomPhp(r.entries.filter((c) => c.id === 'php' && c.label).map((c) => c.label)))
      .catch(() => undefined)
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])

  async function apply(overwrite: string[] = []) {
    const res = await runCommand({ type: 'apply_web', overwrite })
    if (res.type === 'applied') {
      setReports(res.reports)
      if (res.reports.some((r) => r.drifted.length > 0)) setDriftOpen(true)
    }
    await refresh(true)
  }

  /**
   * Deleting a site re-renders every config file, runs the web server's own validator and
   * reloads it — seconds of work. The row leaves the list at once and the slow half runs
   * behind it, with `applying` telling the page to say so, instead of the row sitting
   * there looking untouched while the user waits.
   */
  /**
   * Deleting a site moves its folder into an `ols_excluded_sites` folder in the same directory,
   * revokes its certificate, re-renders config files and reloads the web server.
   */
  async function deleteSite(hostname: string) {
    if (removing.has(hostname)) return
    markRemoving(hostname)
    setDomains((list) => list.filter((d) => d.hostname !== hostname))
    try {
      await runCommand({ type: 'delete_site', hostname })
      await refreshProjects()
      await applyTracked()
    } catch (e) {
      await refresh().catch(() => undefined)
      await refreshProjects().catch(() => undefined)
      throw e
    } finally {
      markRemoving(hostname, false)
    }
  }

  /**
   * Hide/Exclude a site from OpenLocalServer: revokes cert and config, but files on disk
   * are completely untouched. Rescans ignore it until cleared in Settings > Hide/Exclude.
   */
  async function hideSite(hostname: string) {
    if (removing.has(hostname)) return
    markRemoving(hostname)
    setDomains((list) => list.filter((d) => d.hostname !== hostname))
    try {
      await runCommand({ type: 'remove_domain', hostname })
      await refreshProjects()
      await applyTracked()
    } catch (e) {
      await refresh().catch(() => undefined)
      await refreshProjects().catch(() => undefined)
      throw e
    } finally {
      markRemoving(hostname, false)
    }
  }

  /** Deletes a project by moving its folder into an `ols_excluded_sites` folder in the same directory. */
  async function deleteProject(id: string) {
    if (removing.has(id)) return
    markRemoving(id)
    setProjects((list) => list.filter((p) => p.id !== id))
    try {
      await runCommand({ type: 'delete_project', id })
    } catch (e) {
      await refreshProjects().catch(() => undefined)
      throw e
    } finally {
      markRemoving(id, false)
    }
  }

  /** Hide/Exclude a project from the list. Its files are never touched. */
  async function hideProjectFromList(id: string) {
    if (removing.has(id)) return
    markRemoving(id)
    setProjects((list) => list.filter((p) => p.id !== id))
    try {
      await runCommand({ type: 'remove_project', id })
    } catch (e) {
      await refreshProjects().catch(() => undefined)
      throw e
    } finally {
      markRemoving(id, false)
    }
  }

  /** Relocates a project to a new folder on disk and updates associated sites. */
  async function relocateProject(id: string, newPath: string) {
    try {
      const res = await runCommand({ type: 'relocate_project', id, path: newPath })
      await Promise.all([refresh(), refreshProjects()])
      await applyTracked()
      return res
    } catch (e) {
      await Promise.all([refresh().catch(() => undefined), refreshProjects().catch(() => undefined)])
      throw e
    }
  }

  /** Backwards compatibility alias for hideProjectFromList */
  const removeProjectFromList = hideProjectFromList

  /**
   * Bulk site edits. Each one is a single backend call and a single apply.
   */
  async function runBulk(
    keys: readonly string[],
    command: CoreCommand,
    optimistic: (list: DomainSummary[]) => DomainSummary[],
  ): Promise<BulkOutcome> {
    if (keys.length === 0) return { changed: 0, skipped: [] }
    keys.forEach((key) => markRemoving(key))
    setDomains(optimistic)
    try {
      const res = await runCommand(command)
      const outcome: BulkOutcome = res.type === 'bulk' ? { changed: res.changed, skipped: res.skipped } : { changed: 0, skipped: [] }
      await refresh().catch(() => undefined)
      await refreshProjects().catch(() => undefined)
      await applyTracked()
      return outcome
    } catch (e) {
      await refresh().catch(() => undefined)
      await refreshProjects().catch(() => undefined)
      throw e
    } finally {
      keys.forEach((key) => markRemoving(key, false))
    }
  }

  function setDomainsEnabled(hostnames: readonly string[], enabled: boolean): Promise<BulkOutcome> {
    return runBulk(hostnames, { type: 'bulk_set_domains_enabled', hostnames: [...hostnames], enabled }, (list) =>
      list.map((d) => (hostnames.includes(d.hostname) ? { ...d, enabled } : d)),
    )
  }

  function setDomainsServer(hostnames: readonly string[], server: string | null): Promise<BulkOutcome> {
    return runBulk(
      hostnames,
      { type: 'bulk_set_domain_server', hostnames: [...hostnames], server },
      (list) => {
        const resolved = server ?? cfg?.default_server ?? ''
        return list.map((d) => (hostnames.includes(d.hostname) ? { ...d, server: resolved } : d))
      },
    )
  }

  function deleteSites(hostnames: readonly string[]): Promise<BulkOutcome> {
    return runBulk(hostnames, { type: 'bulk_delete_sites', hostnames: [...hostnames] }, (list) =>
      list.filter((d) => !hostnames.includes(d.hostname)),
    )
  }

  function hideSites(hostnames: readonly string[]): Promise<BulkOutcome> {
    return runBulk(hostnames, { type: 'bulk_remove_domains', hostnames: [...hostnames] }, (list) =>
      list.filter((d) => !hostnames.includes(d.hostname)),
    )
  }

  const installedPhp = [...new Set([...catalog.filter((c) => c.id === 'php' && c.installed).map((c) => c.version), ...customPhp])]
  /** Hostnames whose generated config was edited by hand, across every server. */
  const driftedHosts = reports.flatMap((r) => r.drifted)

  return {
    ...action,
    status,
    cfg,
    domains,
    certs,
    ca,
    projects,
    installedPhp,
    reports,
    setReports,
    driftedHosts,
    driftOpen,
    setDriftOpen,
    removing,
    applying,
    deleteSite,
    deleteSites,
    deleteProject,
    relocateProject,
    hideSite,
    hideSites,
    hideProjectFromList,
    removeProjectFromList,
    setDomainsEnabled,
    setDomainsServer,
    refresh: () => refresh(),
    /** Re-reads the stored config, so a settings save clears the drafts that produced it. */
    refreshConfig: () => refresh(true),
    refreshProjects,
    apply,
  }
}

export type Web = ReturnType<typeof useWeb>
