import type { CustomProjectType, Project, ProjectTypePreset } from '@/core'

export interface ProjectTypeTab {
  id: string
  label: string
  icon: string
  count: number
  isCustom?: boolean
}

export interface ProjectFilterPreferences {
  pinnedIds: string[]
  orderIds: string[]
  typeIcons?: Record<string, string>
}

export const PROJECT_FILTER_PREFS_KEY = 'ols:project-filter-preferences'
export const PROJECTS_TAB_KEY = 'ols:projects-selected-tab'

export function getProjectFilterPreferences(): ProjectFilterPreferences {
  try {
    const raw = localStorage.getItem(PROJECT_FILTER_PREFS_KEY)
    if (!raw) return { pinnedIds: [], orderIds: [], typeIcons: {} }
    const parsed = JSON.parse(raw) as Partial<ProjectFilterPreferences>
    return {
      pinnedIds: Array.isArray(parsed?.pinnedIds) ? parsed.pinnedIds : [],
      orderIds: Array.isArray(parsed?.orderIds) ? parsed.orderIds : [],
      typeIcons: parsed?.typeIcons && typeof parsed.typeIcons === 'object' ? parsed.typeIcons : {},
    }
  } catch {
    return { pinnedIds: [], orderIds: [] }
  }
}

export function saveProjectFilterPreferences(prefs: ProjectFilterPreferences): void {
  try {
    localStorage.setItem(PROJECT_FILTER_PREFS_KEY, JSON.stringify(prefs))
  } catch {}
}

export function updateRenamedTypeInPreferences(oldId: string, newId: string): void {
  try {
    const prefs = getProjectFilterPreferences()
    const normOld = oldId.toLowerCase()

    const nextPinned = prefs.pinnedIds.map((id) => (id.toLowerCase() === normOld ? newId : id))
    const nextOrder = prefs.orderIds.map((id) => (id.toLowerCase() === normOld ? newId : id))

    saveProjectFilterPreferences({
      pinnedIds: nextPinned,
      orderIds: nextOrder,
    })

    // Also update selected tab if it was the old id
    const currentTab = localStorage.getItem(PROJECTS_TAB_KEY)
    if (currentTab && currentTab.toLowerCase() === normOld) {
      localStorage.setItem(PROJECTS_TAB_KEY, newId)
    }
    const sessionTab = sessionStorage.getItem('ols:projects-tab')
    if (sessionTab && sessionTab.toLowerCase() === normOld) {
      sessionStorage.setItem('ols:projects-tab', newId)
    }
  } catch {}
}

export function sortProjectTypeTabs(
  tabs: ProjectTypeTab[],
  preferences?: ProjectFilterPreferences,
): ProjectTypeTab[] {
  if (tabs.length <= 1) return tabs

  const allTab = tabs.find((t) => t.id === 'all') ?? tabs[0]
  const otherTabs = tabs.filter((t) => t.id !== allTab.id)

  const pinnedSet = new Set((preferences?.pinnedIds ?? []).map((id) => id.toLowerCase()))
  const orderList = (preferences?.orderIds ?? []).map((id) => id.toLowerCase())

  const getOrderIndex = (id: string) => {
    return orderList.indexOf(id.toLowerCase())
  }

  const sortedOthers = [...otherTabs].sort((a, b) => {
    const aPinned = pinnedSet.has(a.id.toLowerCase())
    const bPinned = pinnedSet.has(b.id.toLowerCase())

    // 1. Pinned chips come first
    if (aPinned && !bPinned) return -1
    if (!aPinned && bPinned) return 1

    const aOrder = getOrderIndex(a.id)
    const bOrder = getOrderIndex(b.id)

    // 2. If both have explicit positions from drag-to-reorder, honor that
    if (aOrder !== -1 && bOrder !== -1) {
      return aOrder - bOrder
    }
    if (aOrder !== -1 && bOrder === -1) {
      return -1
    }
    if (aOrder === -1 && bOrder !== -1) {
      return 1
    }

    // 3. Otherwise, sort by project count descending
    if (b.count !== a.count) {
      return b.count - a.count
    }

    // 4. Tiebreak alphabetically by label
    return a.label.localeCompare(b.label)
  })

  return [allTab, ...sortedOthers]
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
    const icon = name.toLowerCase() === 'tauri' ? 'tauri' : name.toLowerCase() === 'rust' ? 'rust' : 'custom'
    return { id: icon, label: name, isCustom: true }
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
  preferences?: ProjectFilterPreferences,
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

  const rawTabs: ProjectTypeTab[] = [
    {
      id: 'all',
      label: 'All',
      icon: 'folder',
      count: totalCount,
    },
  ]

  const autoCount = counts.get('auto') ?? 0
  if (autoCount > 0) {
    rawTabs.push({
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
      rawTabs.push({
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
      const lower = ct.name.toLowerCase()
      const icon = preferences?.typeIcons?.[customId] ?? preferences?.typeIcons?.[customId.toLowerCase()] ?? (lower === 'tauri' ? 'tauri' : lower === 'rust' ? 'rust' : 'custom')
      rawTabs.push({
        id: customId,
        label: ct.name,
        icon,
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
      const lower = name.toLowerCase()
      const icon = preferences?.typeIcons?.[key] ?? preferences?.typeIcons?.[key.toLowerCase()] ?? (lower === 'tauri' ? 'tauri' : lower === 'rust' ? 'rust' : 'custom')
      rawTabs.push({
        id: key,
        label: name,
        icon,
        count,
        isCustom: true,
      })
      encounteredCustom.add(key.toLowerCase())
    }
  }

  return {
    tabs: sortProjectTypeTabs(rawTabs, preferences),
    totalCount,
    missingCount,
  }
}

