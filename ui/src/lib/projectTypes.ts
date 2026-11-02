import type { CustomProjectType, Project, ProjectTypePreset } from '@/core'

export interface ProjectTypeTab {
  id: string
  label: string
  icon: string
  count: number
  isCustom?: boolean
}

export function getProjectTypeId(p: Project): string {
  if (!p.project_type || p.project_type === 'auto') {
    return 'auto'
  }
  return p.project_type
}

export function getProjectDisplayType(
  p: Project,
  presets: ProjectTypePreset[],
  customTypes: CustomProjectType[] = [],
): { id: string; label: string; isCustom: boolean } {
  if (!p.project_type || p.project_type === 'auto') {
    return { id: 'folder', label: 'Auto', isCustom: false }
  }

  if (p.project_type.startsWith('custom:')) {
    const rawName = p.project_type.replace(/^custom:/, '')
    const found = customTypes.find(
      (c) => c.id.toLowerCase() === p.project_type?.toLowerCase() || c.name.toLowerCase() === rawName.toLowerCase(),
    )
    const name = found?.name ?? rawName
    return { id: 'custom', label: name, isCustom: true }
  }

  const preset = presets.find((pr) => pr.id.toLowerCase() === p.project_type?.toLowerCase())
  if (preset) {
    return { id: preset.id, label: preset.name, isCustom: false }
  }

  return { id: p.project_type, label: p.project_type, isCustom: false }
}

export function groupProjectsByType(
  projects: Project[],
  presets: ProjectTypePreset[],
  customTypes: CustomProjectType[] = [],
): {
  tabs: ProjectTypeTab[]
  totalCount: number
  missingCount: number
} {
  const totalCount = projects.length
  const missingCount = projects.filter((p) => Boolean(p.health && p.health.status !== 'healthy')).length

  const counts = new Map<string, number>()
  for (const p of projects) {
    const typeId = getProjectTypeId(p)
    counts.set(typeId, (counts.get(typeId) ?? 0) + 1)
  }

  const tabs: ProjectTypeTab[] = [
    {
      id: 'all',
      label: 'All',
      icon: 'folder',
      count: totalCount,
    },
  ]

  const autoCount = counts.get('auto') ?? 0
  if (autoCount > 0) {
    tabs.push({
      id: 'auto',
      label: 'Auto',
      icon: 'folder',
      count: autoCount,
    })
  }

  // Add presets that have at least one project
  for (const pr of presets) {
    const count = counts.get(pr.id) ?? 0
    if (count > 0) {
      tabs.push({
        id: pr.id,
        label: pr.name,
        icon: pr.icon || pr.id,
        count,
        isCustom: false,
      })
    }
  }

  // Add custom types that have at least one project (or configured custom types)
  const encounteredCustom = new Set<string>()

  // From registered custom types
  for (const ct of customTypes) {
    const customId = ct.id.startsWith('custom:') ? ct.id : `custom:${ct.name}`
    const count = counts.get(customId) ?? 0
    if (count > 0) {
      tabs.push({
        id: customId,
        label: ct.name,
        icon: 'custom',
        count,
        isCustom: true,
      })
      encounteredCustom.add(customId.toLowerCase())
    }
  }

  // Also check for any custom: keys on projects not explicitly in customTypes
  for (const [key, count] of counts.entries()) {
    if (key.startsWith('custom:') && !encounteredCustom.has(key.toLowerCase()) && count > 0) {
      const name = key.replace(/^custom:/, '')
      tabs.push({
        id: key,
        label: name,
        icon: 'custom',
        count,
        isCustom: true,
      })
      encounteredCustom.add(key.toLowerCase())
    }
  }

  return {
    tabs,
    totalCount,
    missingCount,
  }
}

