import { Image as ImageIcon, Save, Settings } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ProjectTypePicker } from '@/components/project/ProjectTypePicker'
import { SiteLogoPanel } from '@/components/site/SiteLogoPanel'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  type CustomProjectType,
  type Project,
  type ProjectTypePreset,
  runCommand,
} from '@/core'
import { notify } from '@/lib/notify'

interface ProjectSettingsDialogProps {
  project: Project | null
  open: boolean
  onClose: () => void
  onUpdated: () => Promise<void> | void
  presets?: ProjectTypePreset[]
  customTypes?: CustomProjectType[]
  onCustomTypesChanged?: () => Promise<void> | void
}

export function ProjectSettingsDialog({
  project,
  open,
  onClose,
  onUpdated,
  presets: initialPresets,
  customTypes: initialCustomTypes,
  onCustomTypesChanged,
}: ProjectSettingsDialogProps) {
  const [activeTab, setActiveTab] = useState<'general' | 'icon'>('general')
  const [name, setName] = useState('')
  const [projectType, setProjectType] = useState<string | null>(null)
  const [presets, setPresets] = useState<ProjectTypePreset[]>(initialPresets ?? [])
  const [customTypes, setCustomTypes] = useState<CustomProjectType[]>(initialCustomTypes ?? [])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (initialPresets) setPresets(initialPresets)
  }, [initialPresets])

  useEffect(() => {
    if (initialCustomTypes) setCustomTypes(initialCustomTypes)
  }, [initialCustomTypes])

  useEffect(() => {
    if (!open || !project) return
    setName(project.name)
    setProjectType(project.project_type ?? null)
    setActiveTab('general')
    setError(null)

    // Load presets if not provided
    if (!initialPresets) {
      runCommand({ type: 'list_project_types' })
        .then((res) => {
          if (res.type === 'project_types') {
            setPresets(res.types)
          }
        })
        .catch(() => undefined)
    }

    // Load custom types if not provided
    if (!initialCustomTypes) {
      runCommand({ type: 'list_custom_project_types' })
        .then((res) => {
          if (res.type === 'custom_project_types') {
            setCustomTypes(res.types)
          }
        })
        .catch(() => undefined)
    }
  }, [open, project, initialPresets, initialCustomTypes])

  if (!project) return null

  const handleSaveGeneral = async () => {
    if (!project) return
    const trimmedName = name.trim()
    if (!trimmedName) {
      setError('Project name cannot be empty')
      return
    }

    setSaving(true)
    setError(null)
    try {
      const isAuto = !projectType || projectType === 'auto'

      await runCommand({
        type: 'update_project',
        id: project.id,
        name: trimmedName,
        project_type: isAuto ? null : projectType,
        clear_project_type: isAuto,
      })

      notify.success(`Updated settings for "${trimmedName}"`)
      await onUpdated()
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  const handleRefreshCustomTypes = async () => {
    try {
      const res = await runCommand({ type: 'list_custom_project_types' })
      if (res.type === 'custom_project_types') {
        setCustomTypes(res.types)
      }
      await onCustomTypesChanged?.()
    } catch {}
  }

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={`Project Settings — ${project.name}`}
      size="form"
      icon={<Settings className="size-5 text-primary" />}
      footer={
        activeTab === 'general' ? (
          <div className="flex w-full items-center justify-between">
            <span className="text-xs text-muted-foreground truncate max-w-xs">{project.path}</span>
            <div className="flex gap-2">
              <Button variant="outline" onClick={onClose} disabled={saving}>
                Cancel
              </Button>
              <Button onClick={() => void handleSaveGeneral()} disabled={saving}>
                <Save /> {saving ? 'Saving…' : 'Save Changes'}
              </Button>
            </div>
          </div>
        ) : undefined
      }
    >
      <div className="flex flex-col gap-4">
        {/* Navigation Tabs */}
        <div className="flex border-b border-border text-sm">
          <button
            type="button"
            className={`flex items-center gap-2 border-b-2 px-4 py-2 font-medium transition-colors ${
              activeTab === 'general'
                ? 'border-primary text-primary font-semibold'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            onClick={() => setActiveTab('general')}
          >
            <Settings className="size-4" /> General
          </button>
          <button
            type="button"
            className={`flex items-center gap-2 border-b-2 px-4 py-2 font-medium transition-colors ${
              activeTab === 'icon'
                ? 'border-primary text-primary font-semibold'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
            onClick={() => setActiveTab('icon')}
          >
            <ImageIcon className="size-4" /> Icon & Logo
          </button>
        </div>

        {error && (
          <div className="rounded-md border border-destructive/20 bg-destructive/10 p-2.5 text-xs text-destructive">
            {error}
          </div>
        )}

        {/* Tab 1: General */}
        {activeTab === 'general' && (
          <div className="flex flex-col gap-4 py-2">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-semibold text-foreground">Project Name</label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="My Project"
                disabled={saving}
              />
              <span className="text-[11px] text-muted-foreground">
                Display name shown in project lists and menus.
              </span>
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-semibold text-foreground">Project Directory</label>
              <div className="flex items-center gap-2">
                <Input
                  value={project.path}
                  readOnly
                  disabled
                  className="bg-muted/50 font-mono text-xs cursor-default text-muted-foreground"
                />
              </div>
              <span className="text-[11px] text-muted-foreground">
                Folder path on disk. To move this project, use the "Locate" action on the Projects page.
              </span>
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-semibold text-foreground">Project Type</label>
              <ProjectTypePicker
                value={projectType}
                onChange={setProjectType}
                presets={presets}
                customTypes={customTypes}
                onCustomTypesChanged={handleRefreshCustomTypes}
                disabled={saving}
              />
              <span className="text-[11px] text-muted-foreground">
                Determines the badge and tech icon shown for this project.
              </span>
            </div>
          </div>
        )}

        {/* Tab 2: Icon & Logo */}
        {activeTab === 'icon' && (
          <div className="py-1">
            <SiteLogoPanel
              hostname={null}
              projectId={project.id}
              folder={project.path}
              displayName={project.name}
              initialIcon={project.icon}
              initialImage={project.icon_image}
              fallback="folder"
              onUpdated={async () => {
                await onUpdated()
              }}
            />
          </div>
        )}
      </div>
    </Dialog>
  )
}
