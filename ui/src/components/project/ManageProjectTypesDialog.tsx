import { Check, Edit2, GripVertical, Pin, Plus, Sliders, Trash2, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

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
import {
  getProjectFilterPreferences,
  type ProjectFilterPreferences,
  saveProjectFilterPreferences,
  updateRenamedTypeInPreferences,
} from '@/lib/projectTypes'
import { cn } from '@/lib/utils'

interface ManageProjectTypesDialogProps {
  open: boolean
  onClose: () => void
  presets: ProjectTypePreset[]
  customTypes: CustomProjectType[]
  projects: Project[]
  onUpdated: () => Promise<void> | void
}

interface ManagedTypeItem {
  id: string
  name: string
  icon: string
  count: number
  isCustom: boolean
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

  const [preferences, setPreferences] = useState<ProjectFilterPreferences>(() =>
    getProjectFilterPreferences(),
  )
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)

  useEffect(() => {
    if (open) {
      setPreferences(getProjectFilterPreferences())
      setError(null)
      setEditingId(null)
      setNewTypeName('')
    }
  }, [open])

  // Count projects for each type
  const counts = useMemo(() => {
    const map = new Map<string, number>()
    for (const p of projects) {
      const pt = p.project_type || 'auto'
      map.set(pt.toLowerCase(), (map.get(pt.toLowerCase()) ?? 0) + 1)
    }
    return map
  }, [projects])

  // Aggregate all managed items
  const items = useMemo<ManagedTypeItem[]>(() => {
    const list: ManagedTypeItem[] = []
    const seen = new Set<string>()

    // 1. Custom types
    for (const c of customTypes) {
      const id = c.id.startsWith('custom:') ? c.id : `custom:${c.name}`
      const lower = id.toLowerCase()
      if (seen.has(lower)) continue
      seen.add(lower)

      const nameLower = c.name.toLowerCase()
      const icon = nameLower === 'tauri' ? 'tauri' : nameLower === 'rust' ? 'rust' : 'custom'
      const count =
        (counts.get(lower) ?? 0) + (counts.get(`custom:${nameLower}`) ?? 0)

      list.push({
        id,
        name: c.name,
        icon,
        count: counts.get(lower) ?? count,
        isCustom: true,
      })
    }

    // Also check for any custom: keys on projects not explicitly in customTypes
    for (const [key, count] of counts.entries()) {
      if (key.startsWith('custom:') && !seen.has(key)) {
        seen.add(key)
        const name = key.replace(/^custom:/, '')
        const lower = name.toLowerCase()
        const icon = lower === 'tauri' ? 'tauri' : lower === 'rust' ? 'rust' : 'custom'
        list.push({
          id: key,
          name,
          icon,
          count,
          isCustom: true,
        })
      }
    }

    // 2. Presets
    for (const pr of presets) {
      const lower = pr.id.toLowerCase()
      if (seen.has(lower)) continue
      seen.add(lower)
      list.push({
        id: pr.id,
        name: pr.name,
        icon: pr.icon || pr.id,
        count: counts.get(lower) ?? 0,
        isCustom: false,
      })
    }

    // 3. Auto (if used)
    const autoCount = counts.get('auto') ?? 0
    if (autoCount > 0 && !seen.has('auto')) {
      seen.add('auto')
      list.push({
        id: 'auto',
        name: 'Auto',
        icon: 'folder',
        count: autoCount,
        isCustom: false,
      })
    }

    // Sort according to orderIds, then by count descending
    const orderList = preferences.orderIds.map((id) => id.toLowerCase())
    return list.sort((a, b) => {
      const aOrder = orderList.indexOf(a.id.toLowerCase())
      const bOrder = orderList.indexOf(b.id.toLowerCase())

      if (aOrder !== -1 && bOrder !== -1) return aOrder - bOrder
      if (aOrder !== -1 && bOrder === -1) return -1
      if (aOrder === -1 && bOrder !== -1) return 1

      if (b.count !== a.count) return b.count - a.count
      return a.name.localeCompare(b.name)
    })
  }, [customTypes, presets, counts, preferences.orderIds])

  if (!open) return null

  const pinnedSet = new Set(preferences.pinnedIds.map((id) => id.toLowerCase()))

  const togglePin = (item: ManagedTypeItem) => {
    const normId = item.id.toLowerCase()
    const isPinned = pinnedSet.has(normId)

    const nextPinned = isPinned
      ? preferences.pinnedIds.filter((id) => id.toLowerCase() !== normId)
      : [...preferences.pinnedIds, item.id]

    const nextPrefs = {
      ...preferences,
      pinnedIds: nextPinned,
    }
    saveProjectFilterPreferences(nextPrefs)
    setPreferences(nextPrefs)
    void onUpdated()
  }

  const handleDragStart = (e: React.DragEvent, index: number) => {
    setDraggingIndex(index)
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', String(index))
  }

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    if (dragOverIndex !== index) {
      setDragOverIndex(index)
    }
  }

  const handleDrop = (e: React.DragEvent, toIndex: number) => {
    e.preventDefault()
    if (draggingIndex === null || draggingIndex === toIndex) {
      setDraggingIndex(null)
      setDragOverIndex(null)
      return
    }

    const reordered = [...items]
    const [removed] = reordered.splice(draggingIndex, 1)
    reordered.splice(toIndex, 0, removed)

    const newOrderIds = reordered.map((item) => item.id)
    const nextPrefs = {
      ...preferences,
      orderIds: newOrderIds,
    }
    saveProjectFilterPreferences(nextPrefs)
    setPreferences(nextPrefs)
    setDraggingIndex(null)
    setDragOverIndex(null)
    void onUpdated()
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

  const handleStartEdit = (item: ManagedTypeItem) => {
    setEditingId(item.id)
    setEditName(item.name)
    setError(null)
  }

  const handleSaveEdit = async (item: ManagedTypeItem) => {
    const trimmed = editName.trim()
    if (!trimmed || trimmed === item.name) {
      setEditingId(null)
      return
    }

    const oldId = item.id
    const newId = `custom:${trimmed}`

    setBusy(true)
    setError(null)
    try {
      const res = await runCommand({
        type: 'rename_custom_project_type',
        old_name: item.name,
        new_name: trimmed,
      })
      if (res.type === 'custom_project_types') {
        // Keep position and pin state when renaming
        updateRenamedTypeInPreferences(oldId, newId)
        setPreferences(getProjectFilterPreferences())
        notify.success(`Renamed "${item.name}" to "${trimmed}"`)
        setEditingId(null)
        await onUpdated()
      }
    } catch (e: any) {
      setError(e?.problem || (e instanceof Error ? e.message : String(e)))
    } finally {
      setBusy(false)
    }
  }

  const handleDelete = async (item: ManagedTypeItem) => {
    const countNote =
      item.count > 0
        ? `\n\n${item.count} project${item.count > 1 ? 's' : ''} currently using this type will be reset to "Auto".`
        : ''

    const ok = await confirmAction(
      `Are you sure you want to remove the custom type "${item.name}"?${countNote}`,
      `Remove type "${item.name}"?`,
      'Remove Type',
    )
    if (!ok) return

    setBusy(true)
    setError(null)
    try {
      const res = await runCommand({ type: 'remove_custom_project_type', name: item.name })
      if (res.type === 'custom_project_types') {
        const norm = item.id.toLowerCase()
        const nextPrefs = {
          pinnedIds: preferences.pinnedIds.filter((id) => id.toLowerCase() !== norm),
          orderIds: preferences.orderIds.filter((id) => id.toLowerCase() !== norm),
        }
        saveProjectFilterPreferences(nextPrefs)
        setPreferences(nextPrefs)
        notify.success(`Removed custom type "${item.name}"`)
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
        <div className="flex w-full justify-between items-center text-xs text-muted-foreground">
          <span>Drag rows to reorder chips. Pin types to keep them on the bar.</span>
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

        {/* Reorderable types list with pin toggles */}
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
              Filter Bar Types & Order ({items.length})
            </h3>
            <span className="text-[11px] text-muted-foreground">
              {preferences.pinnedIds.length} pinned
            </span>
          </div>

          {items.length === 0 ? (
            <div className="rounded-md border border-dashed border-border p-6 text-center text-xs text-muted-foreground">
              No project types available.
            </div>
          ) : (
            <div className="flex flex-col divide-y divide-border border border-border rounded-lg overflow-hidden bg-card max-h-[340px] overflow-y-auto">
              {items.map((item, index) => {
                const isEditing = editingId === item.id
                const isPinned = pinnedSet.has(item.id.toLowerCase())
                const isDragging = draggingIndex === index
                const isDragOver = dragOverIndex === index

                return (
                  <div
                    key={item.id}
                    draggable={!isEditing && !busy}
                    onDragStart={(e) => handleDragStart(e, index)}
                    onDragOver={(e) => handleDragOver(e, index)}
                    onDrop={(e) => handleDrop(e, index)}
                    onDragEnd={() => {
                      setDraggingIndex(null)
                      setDragOverIndex(null)
                    }}
                    className={cn(
                      'flex items-center justify-between p-2.5 gap-2 transition-colors select-none',
                      isDragging && 'opacity-40 bg-muted/40',
                      isDragOver && !isDragging && 'bg-primary/10 border-t-2 border-primary',
                      !isDragging && !isDragOver && 'hover:bg-muted/20',
                    )}
                  >
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      {/* Drag handle */}
                      <span
                        className="cursor-grab active:cursor-grabbing text-muted-foreground hover:text-foreground shrink-0 p-0.5"
                        title="Drag to reorder"
                      >
                        <GripVertical className="size-3.5" />
                      </span>

                      {/* Pin to filter bar toggle */}
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => togglePin(item)}
                        className={cn(
                          'size-7 p-0 cursor-pointer shrink-0 transition-colors',
                          isPinned
                            ? 'text-primary bg-primary/10 hover:bg-primary/20'
                            : 'text-muted-foreground hover:text-foreground hover:bg-muted/60',
                        )}
                        title={isPinned ? 'Unpin from filter bar' : 'Pin to filter bar'}
                      >
                        <Pin className={cn('size-3.5', isPinned && 'fill-primary')} />
                      </Button>

                      {/* Tech icon */}
                      <TechIcon id={item.icon} className="size-4 text-muted-foreground shrink-0" />

                      {/* Name or inline edit */}
                      {isEditing ? (
                        <div className="flex items-center gap-1.5 flex-1 max-w-xs">
                          <Input
                            value={editName}
                            onChange={(e) => setEditName(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') {
                                e.preventDefault()
                                void handleSaveEdit(item)
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
                            onClick={() => void handleSaveEdit(item)}
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
                        <span className="font-medium text-xs text-foreground truncate">{item.name}</span>
                      )}

                      {/* Custom vs Preset badge */}
                      <Badge
                        variant={item.isCustom ? 'outline' : 'secondary'}
                        className="text-[10px] px-1.5 py-0 font-normal shrink-0"
                      >
                        {item.isCustom ? 'Custom' : 'Preset'}
                      </Badge>

                      {/* Project count badge */}
                      <Badge
                        variant="secondary"
                        className="text-[10px] px-1.5 py-0 font-normal shrink-0"
                      >
                        {item.count} project{item.count !== 1 ? 's' : ''}
                      </Badge>
                    </div>

                    {/* Actions: Edit & Delete for custom types */}
                    {!isEditing && item.isCustom && (
                      <div className="flex items-center gap-1 shrink-0">
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-7 p-0 cursor-pointer text-muted-foreground hover:text-foreground"
                          onClick={() => handleStartEdit(item)}
                          disabled={busy}
                          title="Rename type (keeps position and pin state)"
                        >
                          <Edit2 className="size-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-7 p-0 cursor-pointer text-muted-foreground hover:text-destructive"
                          onClick={() => void handleDelete(item)}
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
      </div>
    </Dialog>
  )
}
