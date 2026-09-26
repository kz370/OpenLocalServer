import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'

import { AiHost } from '@/components/ai/AiHost'
import { CommandPalette } from '@/components/CommandPalette'
import { ConfirmHost } from '@/components/ConfirmHost'
import { DoctorDialog } from '@/components/DoctorDialog'
import { type Page, Sidebar } from '@/components/layout/Sidebar'
import { CommandsPage } from '@/pages/Commands'
import { ConfigPage } from '@/pages/Config'
import { DashboardPage } from '@/pages/Dashboard'
import { DatabasesPage } from '@/pages/Databases'
import { WebServerPage } from '@/pages/WebServer'
import { LogsPage } from '@/pages/Logs'
import { ProcessesPage } from '@/pages/Processes'
import { PluginsPage } from '@/pages/Plugins'
import { ProfilesPage } from '@/pages/Profiles'
import { SitesPage } from '@/pages/Sites'
import { QuickAppsPage } from '@/pages/QuickApps'
import { RuntimesPage } from '@/pages/Runtimes'
import { ServicesPage } from '@/pages/Services'
import { SettingsPage } from '@/pages/Settings'
import { TunnelsPage } from '@/pages/Tunnels'

export default function App() {
  const [page, setPage] = useState<Page>('dashboard')
  const [doctor, setDoctor] = useState(false)
  const openDoctor = useCallback(() => setDoctor(true), [])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<string>('ols:navigate', (event) => {
      if (event.payload === 'diagnostics') {
        setDoctor(true)
        return
      }
      const route = event.payload === 'quick-apps' ? 'quickapps' : event.payload === 'terminal' ? 'processes' : event.payload
      if (['dashboard', 'sites', 'quickapps', 'commands', 'webserver', 'config', 'tunnels', 'databases', 'services', 'runtimes', 'profiles', 'plugins', 'logs', 'processes', 'settings'].includes(route)) {
        setPage(route as Page)
      }
    }).then((cleanup) => { unlisten = cleanup })
    return () => unlisten?.()
  }, [])

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar page={page} onNavigate={setPage} />
      <ConfirmHost />
      <AiHost onNavigate={setPage} />
      <CommandPalette onNavigate={setPage} onDoctor={openDoctor} />
      <DoctorDialog open={doctor} onClose={() => setDoctor(false)} />
      <main className="min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-6">
        {page === 'dashboard' && <DashboardPage onNavigate={setPage} />}
        {page === 'sites' && <SitesPage onNavigate={setPage} />}
        {page === 'quickapps' && <QuickAppsPage onNavigate={setPage} />}
        {page === 'commands' && <CommandsPage />}
        {page === 'webserver' && <WebServerPage />}
        {page === 'config' && <ConfigPage />}
        {page === 'tunnels' && <TunnelsPage />}
        {page === 'databases' && <DatabasesPage />}
        {page === 'services' && <ServicesPage />}
        {page === 'runtimes' && <RuntimesPage />}
        {page === 'profiles' && <ProfilesPage />}
        {page === 'plugins' && <PluginsPage />}
        {page === 'logs' && <LogsPage />}
        {page === 'processes' && <ProcessesPage />}
        {page === 'settings' && <SettingsPage />}
      </main>
    </div>
  )
}
