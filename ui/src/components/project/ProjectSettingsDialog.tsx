import { Image as ImageIcon, Save, Settings } from 'lucide-react'
import { useEffect, useState } from 'react'

import { SiteLogoPanel } from '@/components/site/SiteLogoPanel'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import { type Project, type ProjectTypePreset, runCommand } from '@/core'
import { notify } from '@/lib/notify'

interface ProjectSettingsDialogProps {
  project: Project | null
  open: boolean
  onClose: () => void
  onUpdated: () => Promise<void> | void
}

export function ProjectSettingsDialog({
  project,
  open,
  onClose,
  onUpdated,
}: ProjectSettingsDialogProps) {
  const [activeTab, setActiveTab] = useState<'general' | 'icon'>('general')
  const [name, setName] = useState('')
  const [projectTypeMode, setProjectTypeMode] = useState<string>('auto')
  const [customType, setCustomType] = useState<string>('')
  const [presets, setPresets] = useState<ProjectTypePreset[]>([])
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !project) return
    setName(project.name)
    setActiveTab('general')
    setError(null)

    // Load presets
    runCommand({ type: 'list_project_types' })
      .then((res) => {
        if (res.type === 'project_types') {
          setPresets(res.types)
        }
      })
      .catch(() => undefined)

    if (!project.project_type) {
      setProjectTypeMode('auto')
      setCustomType('')
    } else if (
      project.project_type.startsWith('custom:') ||
      !presets.some((p) => p.id === project.project_type)
    ) {
      setProjectTypeMode('custom')
      setCustomType(project.project_type.replace(/^custom:/, ''))
    } else {
      setProjectTypeMode(project.project_type)
      setCustomType('')
    }
  }, [open, project])

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
      const typeToSet =
        projectTypeMode === 'auto'
          ? null
          : projectTypeMode === 'custom'
            ? customType.trim() ? `custom:${customType.trim()}` : null
            : projectTypeMode

      await runCommand({
        type: 'update_project',
        id: project.id,
        name: trimmedName,
        project_type: typeToSet,
        clear_project_type: projectTypeMode === 'auto' || (projectTypeMode === 'custom' && !customType.trim()),
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
              <Select
                value={projectTypeMode}
                onChange={(e) => setProjectTypeMode(e.target.value)}
                className="w-full"
                searchable={true}
                searchPlaceholder="Search frameworks & languages..."
                disabled={saving}
              >
                <option value="auto">Auto-detect from project files (default)</option>
                <optgroup label="Preset Frameworks & Languages">
                  {presets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </optgroup>
                <option value="custom">Custom project type…</option>
              </Select>
              <span className="text-[11px] text-muted-foreground">
                Determines the badge and tech icon shown for this project.
              </span>
            </div>

            {projectTypeMode === 'custom' && (
              <div className="flex flex-col gap-1.5 pl-2 border-l-2 border-primary/40">
                <label className="text-xs font-medium text-foreground">Custom Type Name</label>
                <Input
                  value={customType}
                  onChange={(e) => setCustomType(e.target.value)}
                  placeholder="e.g. Rust, Go API, SvelteKit"
                  disabled={saving}
                />
              </div>
            )}
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
