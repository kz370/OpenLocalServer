import { useState } from 'react'

import { type Page, Sidebar } from '@/components/layout/Sidebar'
import { DashboardPage } from '@/pages/Dashboard'
import { ProcessesPage } from '@/pages/Processes'
import { ProjectsPage } from '@/pages/Projects'
import { RuntimesPage } from '@/pages/Runtimes'

export default function App() {
  const [page, setPage] = useState<Page>('dashboard')

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar page={page} onNavigate={setPage} />
      <main className="flex-1 overflow-y-auto p-6">
        {page === 'dashboard' && <DashboardPage />}
        {page === 'projects' && <ProjectsPage />}
        {page === 'processes' && <ProcessesPage />}
        {page === 'runtimes' && <RuntimesPage />}
      </main>
    </div>
  )
}
