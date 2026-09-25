import { useState } from 'react'

import { ConfirmHost } from '@/components/ConfirmHost'
import { type Page, Sidebar } from '@/components/layout/Sidebar'
import { CommandsPage } from '@/pages/Commands'
import { ConfigPage } from '@/pages/Config'
import { DashboardPage } from '@/pages/Dashboard'
import { DatabasesPage } from '@/pages/Databases'
import { DomainsPage } from '@/pages/Domains'
import { LogsPage } from '@/pages/Logs'
import { ProcessesPage } from '@/pages/Processes'
import { ProjectsPage } from '@/pages/Projects'
import { QuickAppsPage } from '@/pages/QuickApps'
import { RuntimesPage } from '@/pages/Runtimes'
import { ServicesPage } from '@/pages/Services'
import { SettingsPage } from '@/pages/Settings'

export default function App() {
  const [page, setPage] = useState<Page>('dashboard')

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar page={page} onNavigate={setPage} />
      <ConfirmHost />
      <main className="flex-1 overflow-y-auto p-6">
        {page === 'dashboard' && <DashboardPage onNavigate={setPage} />}
        {page === 'projects' && <ProjectsPage />}
        {page === 'quickapps' && <QuickAppsPage onNavigate={setPage} />}
        {page === 'commands' && <CommandsPage />}
        {page === 'domains' && <DomainsPage />}
        {page === 'config' && <ConfigPage />}
        {page === 'databases' && <DatabasesPage />}
        {page === 'services' && <ServicesPage />}
        {page === 'runtimes' && <RuntimesPage />}
        {page === 'logs' && <LogsPage />}
        {page === 'processes' && <ProcessesPage />}
        {page === 'settings' && <SettingsPage />}
      </main>
    </div>
  )
}
