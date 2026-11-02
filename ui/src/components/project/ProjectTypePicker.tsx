import { useEffect, useState } from 'react'
import { Select } from '@/components/ui/select'
import { Input } from '@/components/ui/input'
import { type CustomProjectType, type ProjectTypePreset, runCommand } from '@/core'

interface ProjectTypePickerProps {
  value: string | null | undefined
  onChange: (typeId: string | null) => void
  presets: ProjectTypePreset[]
  customTypes: CustomProjectType[]
  onCustomTypesChanged?: () => Promise<void> | void
  disabled?: boolean
}

export function ProjectTypePicker({
  value,
  onChange,
  presets,
  customTypes,
  onCustomTypesChanged,
  disabled = false,
}: ProjectTypePickerProps) {
  const [mode, setMode] = useState<string>('auto')
  const [inlineCustomName, setInlineCustomName] = useState('')
  const [creatingCustom, setCreatingCustom] = useState(false)
  const [customError, setCustomError] = useState<string | null>(null)

  useEffect(() => {
    if (!value || value === 'auto') {
      setMode('auto')
      return
    }

    if (value.startsWith('custom:')) {
      const customName = value.replace(/^custom:/, '')
      // Check if it's already in customTypes
      const exists = customTypes.some(
        (c) => c.id.toLowerCase() === value.toLowerCase() || c.name.toLowerCase() === customName.toLowerCase(),
      )
      if (exists) {
        setMode(value)
      } else {
        // Fallback or new custom
        setMode('new-custom')
        setInlineCustomName(customName)
      }
      return
    }

    // Check presets
    const isPreset = presets.some((p) => p.id === value)
    if (isPreset) {
      setMode(value)
    } else {
      setMode('auto')
    }
  }, [value, presets, customTypes])

  const handleSelectChange = (e: React.ChangeEvent<HTMLSelectElement> | string) => {
    const val = typeof e === 'string' ? e : e.target.value
    setCustomError(null)
    if (val === 'auto') {
      setMode('auto')
      onChange(null)
    } else if (val === 'new-custom') {
      setMode('new-custom')
      if (inlineCustomName.trim()) {
        onChange(`custom:${inlineCustomName.trim()}`)
      } else {
        onChange(null)
      }
    } else {
      setMode(val)
      onChange(val)
    }
  }

  const handleCreateCustom = async () => {
    const trimmed = inlineCustomName.trim()
    if (!trimmed) {
      setCustomError('Custom type name cannot be blank.')
      return
    }
    setCustomError(null)
    setCreatingCustom(true)
    try {
      const res = await runCommand({ type: 'add_custom_project_type', name: trimmed })
      if (res.type === 'custom_project_types') {
        await onCustomTypesChanged?.()
        const customId = `custom:${trimmed}`
        setMode(customId)
        onChange(customId)
      }
    } catch (e) {
      setCustomError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreatingCustom(false)
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <Select
        value={mode}
        onChange={handleSelectChange}
        className="w-full"
        searchable={true}
        searchPlaceholder="Search frameworks & languages..."
        disabled={disabled}
      >
        <option value="auto">Auto-detect from project files (default)</option>

        {presets.length > 0 && (
          <optgroup label="Preset Frameworks & Languages">
            {presets.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </optgroup>
        )}

        {customTypes.length > 0 && (
          <optgroup label="Custom Project Types">
            {customTypes.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </optgroup>
        )}

        <option value="new-custom">+ New custom project type…</option>
      </Select>

      {mode === 'new-custom' && (
        <div className="flex flex-col gap-1.5 pl-2.5 border-l-2 border-primary/40 pt-1">
          <label className="text-[11px] font-medium text-foreground">Custom Type Name</label>
          <div className="flex items-center gap-1.5">
            <Input
              value={inlineCustomName}
              onChange={(e) => {
                setInlineCustomName(e.target.value)
                setCustomError(null)
                if (e.target.value.trim()) {
                  onChange(`custom:${e.target.value.trim()}`)
                } else {
                  onChange(null)
                }
              }}
              placeholder="e.g. Rust, Go API, SvelteKit"
              disabled={disabled || creatingCustom}
              className="h-8 text-xs flex-1"
            />
            {onCustomTypesChanged && inlineCustomName.trim() && (
              <button
                type="button"
                onClick={() => void handleCreateCustom()}
                disabled={disabled || creatingCustom}
                className="cursor-pointer text-xs font-medium px-2.5 py-1.5 bg-primary/10 text-primary hover:bg-primary/20 rounded-md transition-colors whitespace-nowrap"
              >
                {creatingCustom ? 'Saving…' : 'Save to Type List'}
              </button>
            )}
          </div>
          {customError && <span className="text-[11px] text-destructive">{customError}</span>}
          <span className="text-[10px] text-muted-foreground">
            Custom types appear in project filters and badges.
          </span>
        </div>
      )}
    </div>
  )
}
