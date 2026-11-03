import { lazy, Suspense, useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { runCommand } from '@/core'

import { CommandPalette } from '@/components/CommandPalette'
import { ConfirmHost } from '@/components/ConfirmHost'
import { NotificationHost } from '@/components/NotificationHost'
import { TooltipProvider } from '@/components/ui/tooltip'
import { DoctorDialog } from '@/components/DoctorDialog'
import { GlobalAuroraBackground } from '@/components/layout/GlobalAuroraBackground'
import { type Page, Sidebar } from '@/components/layout/Sidebar'
import { Titlebar } from '@/components/layout/Titlebar'
import { ExitingOverlay } from '@/components/layout/ExitingOverlay'
import { useWindowState } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { initDiagnosticsNotificationMonitor } from '@/lib/diagnosticsNotify'
import { initializeServiceStatusTracking } from '@/lib/serviceStatus'
const CommandsPage = lazy(() => import('@/pages/Commands').then((module) => ({ default: module.CommandsPage })))
const DashboardPage = lazy(() => import('@/pages/Dashboard').then((module) => ({ default: module.DashboardPage })))
const DatabasesPage = lazy(() => import('@/pages/Databases').then((module) => ({ default: module.DatabasesPage })))
const WebServerPage = lazy(() => import('@/pages/WebServer').then((module) => ({ default: module.WebServerPage })))
const LogsPage = lazy(() => import('@/pages/Logs').then((module) => ({ default: module.LogsPage })))
const ProcessesPage = lazy(() => import('@/pages/Processes').then((module) => ({ default: module.ProcessesPage })))
const TasksPage = lazy(() => import('@/pages/Tasks').then((module) => ({ default: module.TasksPage })))
const PluginsPage = lazy(() => import('@/pages/Plugins').then((module) => ({ default: module.PluginsPage })))
const ProfilesPage = lazy(() => import('@/pages/Profiles').then((module) => ({ default: module.ProfilesPage })))
const ProjectsPage = lazy(() => import('@/pages/Projects').then((module) => ({ default: module.ProjectsPage })))
const SitesPage = lazy(() => import('@/pages/Sites').then((module) => ({ default: module.SitesPage })))
const QuickAppsPage = lazy(() => import('@/pages/QuickApps').then((module) => ({ default: module.QuickAppsPage })))
const RuntimesPage = lazy(() => import('@/pages/Runtimes').then((module) => ({ default: module.RuntimesPage })))
const ServicesPage = lazy(() => import('@/pages/Services').then((module) => ({ default: module.ServicesPage })))
const SettingsPage = lazy(() => import('@/pages/Settings').then((module) => ({ default: module.SettingsPage })))
const TunnelsPage = lazy(() => import('@/pages/Tunnels').then((module) => ({ default: module.TunnelsPage })))

const VALID_PAGES: readonly Page[] = [
  'dashboard',
  'projects',
  'sites',
  'quickapps',
  'commands',
  'webserver',
  'config',
  'tunnels',
  'profiles',
  'plugins',
  'databases',
  'services',
  'runtimes',
  'logs',
  'processes',
  'tasks',
  'settings',
] as const

function parseHashPage(): Page | null {
  if (typeof window === 'undefined') return null
  const hash = window.location.hash.replace(/^#[/]?/, '').trim()
  if (VALID_PAGES.includes(hash as Page)) {
    return hash as Page
  }
  return null
}

function getStoredPage(): Page | null {
  if (typeof window === 'undefined') return null
  try {
    const stored = sessionStorage.getItem('ols:active-page')
    if (stored && VALID_PAGES.includes(stored as Page)) {
      return stored as Page
    }
  } catch {
    // ignore
  }
  return null
}

function getInitialPage(): Page {
  return parseHashPage() || getStoredPage() || 'dashboard'
}

export default function App() {
  const { isWindowed } = useWindowState()
  const initialPage = getInitialPage()
  const [page, setPage] = useState<Page>(initialPage === 'config' ? 'webserver' : initialPage)
  const [webServerTab, setWebServerTab] = useState<'server' | 'config' | 'certs'>(() => {
    if (initialPage === 'config') return 'config'
    try {
      const stored = sessionStorage.getItem('ols:webserver-tab')
      if (stored === 'server' || stored === 'config' || stored === 'certs') return stored
    } catch {
      // ignore
    }
    return 'server'
  })
  const [logSource, setLogSource] = useState<string | null>(() => {
    try {
      return sessionStorage.getItem('ols:log-source')
    } catch {
      // ignore
    }
    return null
  })
  const [doctor, setDoctor] = useState(false)
  const [trayQuickAppId, setTrayQuickAppId] = useState<string | null>(null)
  const openDoctor = useCallback(() => setDoctor(true), [])
  const clearTrayQuickAppId = useCallback(() => setTrayQuickAppId(null), [])

  useEffect(() => {
    initDiagnosticsNotificationMonitor()
    initializeServiceStatusTracking()
  }, [])

  const changePage = useCallback((target: Page) => {
    setPage(target)
    try {
      sessionStorage.setItem('ols:active-page', target)
      window.history.replaceState(null, '', `#/${target}`)
    } catch {
      // ignore
    }
  }, [])

  const navigate = useCallback(
    (target: Page) => {
      if (target === 'config') {
        setWebServerTab('config')
        try {
          sessionStorage.setItem('ols:webserver-tab', 'config')
        } catch {
          // ignore
        }
        changePage('webserver')
        return
      }
      if (target === 'webserver') {
        setWebServerTab('server')
        try {
          sessionStorage.setItem('ols:webserver-tab', 'server')
        } catch {
          // ignore
        }
      }
      changePage(target)
    },
    [changePage],
  )

  /** Logs opened for one service: switch page and preselect that service's source. */
  const openLogs = useCallback(
    (source?: string | null) => {
      if (source) {
        setLogSource(source)
        try {
          sessionStorage.setItem('ols:log-source', source)
        } catch {
          // ignore
        }
      }
      changePage('logs')
    },
    [changePage],
  )

  // Ensure initial hash reflects active page
  useEffect(() => {
    if (!window.location.hash) {
      window.history.replaceState(null, '', `#/${page}`)
    }
  }, [page])

  // Sync hash changes (e.g. back/forward navigation)
  useEffect(() => {
    const onHashChange = () => {
      const target = parseHashPage()
      if (target && target !== page) {
        navigate(target)
      }
    }
    window.addEventListener('hashchange', onHashChange)
    return () => window.removeEventListener('hashchange', onHashChange)
  }, [navigate, page])

  // Ensure current state is saved before any refresh/unload
  useEffect(() => {
    const saveState = () => {
      try {
        sessionStorage.setItem('ols:active-page', page)
        if (page === 'webserver') {
          sessionStorage.setItem('ols:webserver-tab', webServerTab)
        }
        if (logSource) {
          sessionStorage.setItem('ols:log-source', logSource)
        }
      } catch {
        // ignore
      }
    }
    window.addEventListener('beforeunload', saveState)
    return () => window.removeEventListener('beforeunload', saveState)
  }, [page, webServerTab, logSource])

  // F5 key listener: stays on current page by persisting state and reloading
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'F5') {
        e.preventDefault()
        try {
          sessionStorage.setItem('ols:active-page', page)
          if (page === 'webserver') {
            sessionStorage.setItem('ols:webserver-tab', webServerTab)
          }
          if (logSource) {
            sessionStorage.setItem('ols:log-source', logSource)
          }
        } catch {
          // ignore
        }
        window.location.reload()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [page, webServerTab, logSource])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<string>('ols:navigate', (event) => {
      const quickAppId = event.payload.startsWith('quick-app:') ? event.payload.slice('quick-app:'.length) : ''
      if (quickAppId) {
        setTrayQuickAppId(quickAppId)
        navigate('quickapps')
        return
      }
      if (event.payload === 'diagnostics') {
        setDoctor(true)
        return
      }
      const route = event.payload === 'quick-apps' ? 'quickapps' : event.payload === 'terminal' ? 'processes' : event.payload
      if (VALID_PAGES.includes(route as Page)) {
        navigate(route as Page)
      }
    }).then((cleanup) => {
      unlisten = cleanup
    })
    return () => unlisten?.()
  }, [navigate])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<string>('ols:add-project', (event) => {
      const path = event.payload
      if (path) {
        navigate('projects')
        window.dispatchEvent(new CustomEvent('ols:trigger-add-project', { detail: path }))
      }
    }).then((cleanup) => {
      unlisten = cleanup
    })
    return () => unlisten?.()
  }, [navigate])

  useEffect(() => {
    runCommand({ type: 'take_pending_add_project' })
      .then((res) => {
        if (res.type === 'pending_add_project' && res.path) {
          navigate('projects')
          window.dispatchEvent(new CustomEvent('ols:trigger-add-project', { detail: res.path }))
        }
      })
      .catch(() => undefined)
  }, [navigate])

  return (
    <div
      className={cn(
        'relative isolate flex h-screen w-screen flex-col overflow-hidden text-foreground',
        isWindowed ? 'rounded-xl' : 'rounded-none',
      )}
    >
      <TooltipProvider>
        <GlobalAuroraBackground />
        <Titlebar onNavigateLogs={openLogs} />
        <ExitingOverlay />
        <div className="relative z-10 flex min-h-0 flex-1">
          <Sidebar page={page} onNavigate={navigate} />
          <ConfirmHost />
          <NotificationHost onNavigateLogs={openLogs} />
          <CommandPalette onNavigate={navigate} onDoctor={openDoctor} />
          <DoctorDialog open={doctor} onClose={() => setDoctor(false)} />
          <main className="min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-6">
            <Suspense fallback={<div className="text-sm text-muted-foreground" role="status">Loading page…</div>}>
              {page === 'dashboard' && <DashboardPage onNavigate={navigate} onOpenLogs={openLogs} />}
              {page === 'projects' && <ProjectsPage onNavigate={navigate} />}
              {page === 'sites' && <SitesPage onNavigate={navigate} />}
              {page === 'quickapps' && (
                <QuickAppsPage
                  onNavigate={navigate}
                  openAppId={trayQuickAppId}
                  onOpenAppHandled={clearTrayQuickAppId}
                />
              )}
              {page === 'commands' && <CommandsPage />}
              {page === 'webserver' && (
                <WebServerPage
                  initialTab={webServerTab}
                  onTabChange={(tab) => {
                    setWebServerTab(tab)
                    try {
                      sessionStorage.setItem('ols:webserver-tab', tab)
                    } catch {
                      // ignore
                    }
                  }}
                />
              )}
              {page === 'tunnels' && <TunnelsPage />}
              {page === 'databases' && <DatabasesPage />}
              {page === 'services' && <ServicesPage onOpenLogs={openLogs} />}
              {page === 'runtimes' && <RuntimesPage />}
              {page === 'profiles' && <ProfilesPage />}
              {page === 'plugins' && <PluginsPage />}
              {page === 'logs' && (
                <LogsPage
                  initialSource={logSource}
                  onSourceChange={(src) => {
                    setLogSource(src)
                    try {
                      sessionStorage.setItem('ols:log-source', src)
                    } catch {
                      // ignore
                    }
                  }}
                />
              )}
              {page === 'processes' && <ProcessesPage />}
              {page === 'tasks' && <TasksPage />}
              {page === 'settings' && <SettingsPage />}
            </Suspense>
          </main>
        </div>
      </TooltipProvider>
    </div>
  )
}
