import { ExternalLink, Play, RefreshCw } from 'lucide-react'
import type { ReactNode } from 'react'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { type DockerStatus, runCommand } from '@/core'
import { cn } from '@/lib/utils'

/** What the page shows: the engine check plus a UI-only "starting" while we wait after Start. */
export type DockerView = 'checking' | 'not_installed' | 'stopped' | 'starting' | 'error' | 'running'

export const DOCKER_INSTALL_URL = 'https://www.docker.com/products/docker-desktop/'

/**
 * The single gate for the whole Docker page. Running: renders the children (status line,
 * tabs, lists). Anything else: ONE centered card and no tabs, so an empty list can never
 * be mistaken for "you have nothing". The Compose editor opens from the card's link.
 */
export function DockerGate({
  view,
  status,
  startError,
  canStart,
  loading,
  onStart,
  onRetry,
  onRefresh,
  onDetails,
  onCompose,
  children,
}: {
  view: DockerView
  status: DockerStatus | null
  /** Why the last Start attempt failed (timeout, launch error), shown on the error state. */
  startError: string | null
  canStart: boolean
  loading: boolean
  onStart: () => void
  onRetry: () => void
  onRefresh: () => void
  onDetails: () => void
  onCompose: () => void
  children: ReactNode
}) {
  if (view === 'running') return <>{children}</>

  let dot = 'bg-destructive'
  let title = 'Docker is stopped'
  let text: ReactNode = 'Docker is installed but not running. Start it to manage containers, images, and volumes.'
  let actions: ReactNode = null

  const refreshBtn = (
    <Button size="icon" variant="ghost" className="size-8 cursor-pointer" aria-label="Refresh status" title="Refresh status" disabled={loading} onClick={onRefresh}>
      <RefreshCw className={cn('size-4', loading && 'animate-spin')} />
    </Button>
  )

  if (view === 'checking') {
    dot = ''
    title = 'Checking Docker…'
    text = null
  } else if (view === 'starting') {
    dot = ''
    title = 'Starting Docker…'
    text = 'This can take up to a minute.'
  } else if (view === 'not_installed') {
    dot = 'bg-muted-foreground'
    title = 'Docker is not installed'
    text = 'Install Docker Desktop to run containers. OLS never installs Docker for you.'
    actions = (
      <Button size="sm" className="cursor-pointer" onClick={() => void runCommand({ type: 'open_url', url: DOCKER_INSTALL_URL })}>
        Get Docker <ExternalLink className="size-3.5" />
      </Button>
    )
  } else if (view === 'error') {
    title = "Can't reach Docker"
    text = startError ?? status?.message ?? 'Docker did not answer.'
    actions = (
      <>
        <Button size="sm" className="cursor-pointer" disabled={loading} onClick={onRetry}>
          <RefreshCw className={cn('size-3.5', loading && 'animate-spin')} /> {loading ? 'Retrying…' : 'Retry'}
        </Button>
        <Button size="sm" variant="outline" className="cursor-pointer" onClick={onDetails}>
          View details
        </Button>
      </>
    )
  } else {
    actions = (
      <>
        {canStart ? (
          <Button size="sm" className="cursor-pointer" onClick={onStart}>
            <Play className="size-3.5" /> Start Docker
          </Button>
        ) : (
          <span className="text-xs text-muted-foreground">Docker Desktop wasn't found. Start the engine yourself; this page updates when it answers.</span>
        )}
        {refreshBtn}
      </>
    )
  }

  const busy = view === 'checking' || view === 'starting'
  return (
    <Card className="w-full">
      <CardContent className="flex flex-col items-center gap-3 py-10 text-center" role="status" aria-live="polite">
        {busy ? <Spinner className="size-6" /> : <span className={cn('size-3 rounded-full', dot)} aria-hidden />}
        <p className="text-lg font-semibold">{title}</p>
        {text && <p className="max-w-md text-sm text-muted-foreground">{text}</p>}
        {actions && <div className="flex flex-wrap items-center justify-center gap-2">{actions}</div>}
        {view !== 'checking' && (
          <>
            <hr className="my-1 w-full max-w-xs border-border" />
            <button type="button" className="cursor-pointer text-sm text-primary underline-offset-4 hover:underline" onClick={onCompose}>
              Edit compose files without Docker
            </button>
          </>
        )}
      </CardContent>
    </Card>
  )
}
