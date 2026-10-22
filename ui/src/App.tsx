import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { AiHost } from '@/components/ai/AiHost'
import { CommandPalette } from '@/components/CommandPalette'
import { ConfirmHost } from '@/components/ConfirmHost'
import { DoctorDialog } from '@/components/DoctorDialog'
import { type Page, Sidebar } from '@/components/layout/Sidebar'
import { Titlebar } from '@/components/layout/Titlebar'
import { useWindowState } from '@/lib/hooks'
import { cn } from '@/lib/utils'
import { CommandsPage } from '@/pages/Commands'
import { AskAiPage } from '@/pages/AskAi'
import { DashboardPage } from '@/pages/Dashboard'
import { DatabasesPage } from '@/pages/Databases'
import { WebServerPage } from '@/pages/WebServer'
import { LogsPage } from '@/pages/Logs'
import { ProcessesPage } from '@/pages/Processes'
import { TasksPage } from '@/pages/Tasks'
import { PluginsPage } from '@/pages/Plugins'
import { ProfilesPage } from '@/pages/Profiles'
import { SitesPage } from '@/pages/Sites'
import { QuickAppsPage } from '@/pages/QuickApps'
import { RuntimesPage } from '@/pages/Runtimes'
import { ServicesPage } from '@/pages/Services'
import { SettingsPage } from '@/pages/Settings'
import { TunnelsPage } from '@/pages/Tunnels'

const VALID_PAGES: readonly Page[] = [
  'dashboard',
  'askai',
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
  const openDoctor = useCallback(() => setDoctor(true), [])

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
    (source: string) => {
      setLogSource(source)
      try {
        sessionStorage.setItem('ols:log-source', source)
      } catch {
        // ignore
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

  return (
    <div
      className={cn(
        'flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground',
        isWindowed ? 'rounded-xl' : 'rounded-none',
      )}
    >
      <Titlebar />
      <div className="flex min-h-0 flex-1">
        <Sidebar page={page} onNavigate={navigate} />
        <ConfirmHost />
        <AiHost onNavigate={navigate} />
        <CommandPalette onNavigate={navigate} onDoctor={openDoctor} />
        <DoctorDialog open={doctor} onClose={() => setDoctor(false)} />
        <main className="min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-6">
          {page === 'dashboard' && <DashboardPage onNavigate={navigate} onOpenLogs={openLogs} />}
          {page === 'askai' && <AskAiPage onNavigate={navigate} />}
          {page === 'sites' && <SitesPage onNavigate={navigate} />}
          {page === 'quickapps' && <QuickAppsPage onNavigate={navigate} />}
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
        </main>
      </div>
    </div>
  )
}
