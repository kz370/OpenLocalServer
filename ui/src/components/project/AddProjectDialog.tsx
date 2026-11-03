import { AlertCircle, FolderPlus } from 'lucide-react'
import { useEffect, useState } from 'react'

import { ProjectTypePicker } from '@/components/project/ProjectTypePicker'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  type CustomProjectType,
  type ProjectTypePreset,
  runCommand,
} from '@/core'
import { notify } from '@/lib/notify'

export interface AddProjectItem {
  path: string
  suggestedName?: string
  detectedType?: string | null
}

interface AddProjectDialogProps {
  items: AddProjectItem[]
  open: boolean
  presets: ProjectTypePreset[]
  customTypes: CustomProjectType[]
  onCustomTypesChanged?: () => Promise<void> | void
  onClose: () => void
  onAdded: () => Promise<void> | void
}

export function AddProjectDialog({
  items,
  open,
  presets,
  customTypes,
  onCustomTypesChanged,
  onClose,
  onAdded,
}: AddProjectDialogProps) {
  const [currentIndex, setCurrentIndex] = useState(0)
  const currentItem = items[currentIndex] ?? null

  const [name, setName] = useState('')
  const [projectType, setProjectType] = useState<string | null>(null)
  const [autoSite, setAutoSite] = useState(false)
  const [loading, setLoading] = useState(false)
  const [inspecting, setInspecting] = useState(false)
  const [error, setError] = useState<{ problem: string; cause: string; fix?: string | null } | null>(null)

  useEffect(() => {
    if (!open || !currentItem) {
      setCurrentIndex(0)
      setName('')
      setProjectType(null)
      setAutoSite(false)
      setError(null)
      return
    }

    const defaultName = currentItem.suggestedName || currentItem.path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || 'Project'
    setName(defaultName)
    setProjectType(currentItem.detectedType ?? null)
    setAutoSite(false)
    setError(null)
    setInspecting(true)

    // Inspect folder in backend for duplicate checks and validation
    runCommand({ type: 'inspect_project_folder', path: currentItem.path })
      .then((res) => {
        if (res.type === 'project_folder_inspection') {
          const { inspection } = res
          if (!inspection.exists) {
            setError({
              problem: "That folder can't be added as a project.",
              cause: `"${inspection.path}" does not exist on disk.`,
              fix: 'Check the path and try again.',
            })
          } else if (!inspection.is_dir) {
            setError({
              problem: "That file can't be added as a project.",
              cause: `"${inspection.path}" is a file, not a directory.`,
              fix: 'Select the containing folder to add it as a project.',
            })
          } else if (inspection.duplicate_of) {
            setError({
              problem: 'This folder is already a project.',
              cause: `"${inspection.path}" is already registered as "${inspection.duplicate_of.name}".`,
              fix: 'Use the button below to keep the existing project without an automatic site, or turn the site option on to request one. Any existing site will remain.',
            })
          } else {
            if (inspection.suggested_name && !currentItem.suggestedName) {
              setName(inspection.suggested_name)
            }
            if (inspection.detected_type && !currentItem.detectedType) {
              setProjectType(inspection.detected_type)
            }
          }
        }
      })
      .catch((e) => {
        setError({
          problem: 'Failed to inspect folder.',
          cause: e instanceof Error ? e.message : String(e),
        })
      })
      .finally(() => {
        setInspecting(false)
      })
  }, [open, currentItem, currentIndex])

  if (!currentItem) return null

  const isQueue = items.length > 1
  const queueLabel = isQueue ? ` (${currentIndex + 1} of ${items.length})` : ''

  const handleAdd = async () => {
    const trimmedName = name.trim()
    if (!trimmedName) {
      setError({
        problem: 'Project name is required.',
        cause: 'The name cannot be empty.',
        fix: 'Enter a name for the project.',
      })
      return
    }

    setLoading(true)
    setError(null)

    try {
      const res = await runCommand({
        type: 'add_project',
        path: currentItem.path,
        name: trimmedName,
        project_type: projectType,
        auto_site: autoSite,
      })

      if (res.type === 'project') {
        if (error?.problem === 'This folder is already a project.') {
          notify.success(
            autoSite
              ? `Updated "${res.project.name}" and requested a site`
              : `Kept "${res.project.name}" without an automatic site`,
          )
        } else {
          notify.success(`Added project "${res.project.name}"`)
        }
        await onAdded()

        // Check if there are more items in queue
        if (currentIndex + 1 < items.length) {
          setCurrentIndex((prev) => prev + 1)
        } else {
          onClose()
        }
      }
    } catch (e: any) {
      if (e && typeof e === 'object' && 'problem' in e) {
        setError({
          problem: e.problem,
          cause: e.cause || String(e),
          fix: e.fix,
        })
      } else {
        setError({
          problem: 'Failed to add project.',
          cause: e instanceof Error ? e.message : String(e),
          fix: 'Ensure the folder is accessible and not already added.',
        })
      }
    } finally {
      setLoading(false)
    }
  }

  const handleSkipOrCancel = () => {
    if (isQueue && currentIndex + 1 < items.length) {
      setCurrentIndex((prev) => prev + 1)
    } else {
      onClose()
    }
  }

  const isDuplicate = error?.problem === 'This folder is already a project.'

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={`Add Project${queueLabel}`}
      size="form"
      icon={<FolderPlus className="size-5 text-primary" />}
      footer={
        <div className="flex w-full items-center justify-between">
          <span className="text-xs text-muted-foreground truncate max-w-xs">{currentItem.path}</span>
          <div className="flex gap-2">
            <Button variant="outline" onClick={handleSkipOrCancel} disabled={loading}>
              {isQueue && currentIndex + 1 < items.length ? 'Skip' : 'Cancel'}
            </Button>
            <Button
              onClick={() => void handleAdd()}
              disabled={loading || inspecting || Boolean(error && !isDuplicate)}
            >
              {loading
                ? autoSite
                  ? 'Adding project and site…'
                  : 'Adding…'
                : isDuplicate
                  ? autoSite
                    ? 'Create Site'
                    : 'Keep Project'
                  : isQueue && currentIndex + 1 < items.length
                    ? 'Add & Next'
                    : 'Add Project'}
            </Button>
          </div>
        </div>
      }
    >
      <div className="flex flex-col gap-4 py-1">
        {error && (
          <div className="rounded-lg border border-destructive/30 bg-destructive/10 p-3 text-xs text-destructive flex flex-col gap-1">
            <div className="flex items-center gap-1.5 font-semibold">
              <AlertCircle className="size-4 shrink-0" />
              <span>{error.problem}</span>
            </div>
            <p className="opacity-90">{error.cause}</p>
            {error.fix && <p className="font-medium text-destructive/80 mt-1">Fix: {error.fix}</p>}
          </div>
        )}

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">Project Name</label>
          <Input
            value={name}
            onChange={(e) => {
              setName(e.target.value)
              if (error && error.problem === 'Project name is required.') {
                setError(null)
              }
            }}
            placeholder="My Project"
            disabled={loading}
          />
          <span className="text-[11px] text-muted-foreground">
            Display name shown in project lists and menus.
          </span>
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">Project Directory</label>
          <Input
            value={currentItem.path}
            readOnly
            disabled
            className="bg-muted/50 font-mono text-xs cursor-default text-muted-foreground"
          />
          <span className="text-[11px] text-muted-foreground">
            Folder location on disk.
          </span>
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-semibold text-foreground">Project Type</label>
          <ProjectTypePicker
            value={projectType}
            onChange={setProjectType}
            presets={presets}
            customTypes={customTypes}
            onCustomTypesChanged={onCustomTypesChanged}
            disabled={loading}
          />
          <span className="text-[11px] text-muted-foreground">
            Determines the badge and tech icon shown in the Projects tab.
          </span>
        </div>

        <div className="flex items-center justify-between gap-4 rounded-lg border border-border px-3 py-2.5">
          <div className="flex flex-col gap-1">
            <span className="text-xs font-semibold text-foreground">Create a site automatically</span>
            <span className="text-[11px] text-muted-foreground">
              Off by default. Creates a local site when this project can be served directly.
            </span>
          </div>
          <Switch
            checked={autoSite}
            onChange={setAutoSite}
            disabled={loading}
            label="Create a site automatically"
          />
        </div>
      </div>
    </Dialog>
  )
}

