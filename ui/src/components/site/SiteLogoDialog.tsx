import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type DomainSummary, type Project } from '@/core'
import { SiteLogoPanel } from './SiteLogoPanel'

export interface SiteLogoTarget {
  key: string
  site: DomainSummary | null
  project: Project | null
}

export function SiteLogoDialog({
  target,
  onClose,
  onUpdated,
}: {
  target: SiteLogoTarget | null
  onClose: () => void
  onUpdated: () => Promise<void> | void
}) {
  if (!target) return null

  const { site: d, project: p } = target
  const hostname = d?.hostname ?? null
  const folder = d?.folder ?? p?.path ?? ''
  const displayName = d?.hostname ?? p?.name ?? 'Site'
  const currentIcon = d?.icon ?? null
  const currentImage = d?.icon_image ?? p?.icon_image ?? null

  return (
    <Dialog
      open
      onClose={onClose}
      title="Site Logo & Icon"
      description={`Configure the logo or icon for ${displayName}`}
      size="form"
      footer={
        <Button size="sm" variant="secondary" onClick={onClose}>
          Done
        </Button>
      }
    >
      <SiteLogoPanel
        hostname={hostname}
        folder={folder}
        displayName={displayName}
        initialIcon={currentIcon}
        initialImage={currentImage}
        onUpdated={onUpdated}
      />
    </Dialog>
  )
}
