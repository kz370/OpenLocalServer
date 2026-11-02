import { Check, Edit2, Plus, Sliders, Trash2, X } from 'lucide-react'
import { useState } from 'react'

import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  type CustomProjectType,
  type Project,
  type ProjectTypePreset,
  runCommand,
} from '@/core'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'

interface ManageProjectTypesDialogProps {
  open: boolean
  onClose: () => void
  presets: ProjectTypePreset[]
  customTypes: CustomProjectType[]
  projects: Project[]
  onUpdated: () => Promise<void> | void
}

export function ManageProjectTypesDialog({
  open,
  onClose,
  presets,
  customTypes,
  projects,
  onUpdated,
}: ManageProjectTypesDialogProps) {
  const [newTypeName, setNewTypeName] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  if (!open) return null

  // Count projects using each custom type
  const getProjectCount = (c: CustomProjectType) => {
    const target = c.id.toLowerCase()
    const targetName = `custom:${c.name.toLowerCase()}`
    return projects.filter((p) => {
      if (!p.project_type) return false
      const pt = p.project_type.toLowerCase()
      return pt === target || pt === targetName
    }).length
  }

  const handleAddType = async () => {
    const trimmed = newTypeName.trim()
    if (!trimmed) {
      setError('Please enter a type name.')
      return
    }

    setBusy(true)
    setError(null)
    try {
      const res = await runCommand({ type: 'add_custom_project_type', name: trimmed })
      if (res.type === 'custom_project_types') {
        notify.success(`Added custom type "${trimmed}"`)
        setNewTypeName('')
        await onUpdated()
      }
    } catch (e: any) {
      setError(e?.problem || (e instanceof Error ? e.message : String(e)))
    } finally {
      setBusy(false)
    }
  }

  const handleStartEdit = (c: CustomProjectType) => {
    setEditingId(c.id)
    setEditName(c.name)
    setError(null)
  }

  const handleSaveEdit = async (c: CustomProjectType) => {
    const trimmed = editName.trim()
    if (!trimmed || trimmed === c.name) {
      setEditingId(null)
      return
    }

    setBusy(true)
    setError(null)
    try {
      const res = await runCommand({
        type: 'rename_custom_project_type',
        old_name: c.name,
        new_name: trimmed,
      })
      if (res.type === 'custom_project_types') {
        notify.success(`Renamed "${c.name}" to "${trimmed}"`)
        setEditingId(null)
        await onUpdated()
      }
    } catch (e: any) {
      setError(e?.problem || (e instanceof Error ? e.message : String(e)))
    } finally {
      setBusy(false)
    }
  }

  const handleDelete = async (c: CustomProjectType) => {
    const count = getProjectCount(c)
    const countNote =
      count > 0
        ? `\n\n${count} project${count > 1 ? 's' : ''} currently using this type will be reset to "Auto".`
        : ''

    const ok = await confirmAction(
      `Are you sure you want to remove the custom type "${c.name}"?${countNote}`,
      `Remove type "${c.name}"?`,
      'Remove Type',
    )
    if (!ok) return

    setBusy(true)
    setError(null)
    try {
      const res = await runCommand({ type: 'remove_custom_project_type', name: c.name })
      if (res.type === 'custom_project_types') {
        notify.success(`Removed custom type "${c.name}"`)
        await onUpdated()
      }
    } catch (e: any) {
      setError(e?.problem || (e instanceof Error ? e.message : String(e)))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Manage Project Types"
      size="form"
      icon={<Sliders className="size-5 text-primary" />}
      footer={
        <div className="flex w-full justify-end">
          <Button variant="outline" onClick={onClose} disabled={busy}>
            Close
          </Button>
        </div>
      }
    >
      <div className="flex flex-col gap-4 py-1">
        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2.5 text-xs text-destructive">
            {error}
          </div>
        )}

        {/* Add new custom type input */}
        <div className="flex flex-col gap-1.5 bg-muted/30 p-3 rounded-lg border border-border">
          <label className="text-xs font-semibold text-foreground">Create New Custom Type</label>
          <div className="flex items-center gap-2">
            <Input
              value={newTypeName}
              onChange={(e) => {
                setNewTypeName(e.target.value)
                setError(null)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  void handleAddType()
                }
              }}
              placeholder="e.g. Rust, Go API, Tauri, Flutter"
              disabled={busy}
              className="h-9 text-xs flex-1"
            />
            <Button
              onClick={() => void handleAddType()}
              disabled={busy || !newTypeName.trim()}
              size="sm"
              className="cursor-pointer gap-1.5 h-9"
            >
              <Plus className="size-3.5" /> Add Type
            </Button>
          </div>
          <span className="text-[11px] text-muted-foreground">
            Custom types appear in project filters, tab bars, and settings pickers.
          </span>
        </div>

        {/* Custom types list */}
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
              Your Custom Types ({customTypes.length})
            </h3>
          </div>

          {customTypes.length === 0 ? (
            <div className="rounded-md border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
              No custom project types added yet. Create one above to organize your projects.
            </div>
          ) : (
            <div className="flex flex-col divide-y divide-border border border-border rounded-lg overflow-hidden bg-card">
              {customTypes.map((c) => {
                const count = getProjectCount(c)
                const isEditing = editingId === c.id

                return (
                  <div key={c.id} className="flex items-center justify-between p-2.5 gap-2 hover:bg-muted/20">
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      <TechIcon id="custom" className="size-4 text-muted-foreground shrink-0" />

                      {isEditing ? (
                        <div className="flex items-center gap-1.5 flex-1 max-w-xs">
                          <Input
                            value={editName}
                            onChange={(e) => setEditName(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') {
                                e.preventDefault()
                                void handleSaveEdit(c)
                              } else if (e.key === 'Escape') {
                                setEditingId(null)
                              }
                            }}
                            className="h-7 text-xs"
                            autoFocus
                            disabled={busy}
                          />
                          <Button
                            size="sm"
                            variant="ghost"
                            className="size-7 p-0 cursor-pointer text-primary"
                            onClick={() => void handleSaveEdit(c)}
                            title="Save"
                          >
                            <Check className="size-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="size-7 p-0 cursor-pointer text-muted-foreground"
                            onClick={() => setEditingId(null)}
                            title="Cancel"
                          >
                            <X className="size-3.5" />
                          </Button>
                        </div>
                      ) : (
                        <span className="font-medium text-xs text-foreground truncate">{c.name}</span>
                      )}

                      <Badge variant="secondary" className="text-[10px] px-1.5 py-0 font-normal shrink-0">
                        {count} project{count !== 1 ? 's' : ''}
                      </Badge>
                    </div>

                    {!isEditing && (
                      <div className="flex items-center gap-1 shrink-0">
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-7 p-0 cursor-pointer text-muted-foreground hover:text-foreground"
                          onClick={() => handleStartEdit(c)}
                          disabled={busy}
                          title="Rename type"
                        >
                          <Edit2 className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-7 p-0 cursor-pointer text-muted-foreground hover:text-destructive"
                          onClick={() => void handleDelete(c)}
                          disabled={busy}
                          title="Remove type"
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}
        </div>

        {/* Built-in framework presets info */}
        <div className="flex flex-col gap-1.5 pt-2 border-t border-border">
          <span className="text-[11px] font-semibold text-muted-foreground uppercase tracking-wider">
            Built-in Framework Presets ({presets.length})
          </span>
          <div className="flex flex-wrap gap-1.5 max-h-24 overflow-y-auto pr-1">
            {presets.map((p) => (
              <Badge key={p.id} variant="outline" className="text-[10px] gap-1 py-0.5 px-2 font-normal text-muted-foreground">
                <TechIcon id={p.id} className="size-3" />
                {p.name}
              </Badge>
            ))}
          </div>
        </div>
      </div>
    </Dialog>
  )
}

