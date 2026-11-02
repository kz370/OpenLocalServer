import assert from 'node:assert'
import { describe, it } from 'node:test'
import {
  getProjectDisplayType,
  getProjectFilterPreferences,
  groupProjectsByType,
  saveProjectFilterPreferences,
  updateRenamedTypeInPreferences,
} from '../projectTypes.ts'
import type { CustomProjectType, Project, ProjectTypePreset } from '@/core'

const samplePresets: ProjectTypePreset[] = [
  { id: 'laravel', name: 'Laravel', icon: 'laravel' },
  { id: 'wordpress', name: 'WordPress', icon: 'wordpress' },
  { id: 'node', name: 'Node.js', icon: 'node' },
]

const sampleCustom: CustomProjectType[] = [
  { id: 'custom:Rust', name: 'Rust' },
  { id: 'custom:Go API', name: 'Go API' },
]

describe('projectTypes', () => {
  it('groups projects into tabs with correct counts', () => {
    const projects: Project[] = [
      { id: '1', name: 'App 1', path: 'C:/app1', standalone: true, health: { status: 'healthy' } },
      { id: '2', name: 'App 2', path: 'C:/app2', standalone: true, project_type: 'laravel', health: { status: 'healthy' } },
      { id: '3', name: 'App 3', path: 'C:/app3', standalone: true, project_type: 'laravel', health: { status: 'missing', since: 123 } },
      { id: '4', name: 'App 4', path: 'C:/app4', standalone: true, project_type: 'custom:Rust', health: { status: 'healthy' } },
    ]

    const result = groupProjectsByType(projects, samplePresets, sampleCustom)

    assert.strictEqual(result.totalCount, 4)
    assert.strictEqual(result.missingCount, 1)

    // Tabs should include All first, then sorted by project count: Laravel (2), Auto (1), custom:Rust (1)
    const tabIds = result.tabs.map((t) => t.id)
    assert.deepStrictEqual(tabIds, ['all', 'laravel', 'auto', 'custom:Rust'])

    assert.strictEqual(result.tabs.find((t) => t.id === 'all')?.count, 4)
    assert.strictEqual(result.tabs.find((t) => t.id === 'auto')?.count, 1)
    assert.strictEqual(result.tabs.find((t) => t.id === 'laravel')?.count, 2)
    assert.strictEqual(result.tabs.find((t) => t.id === 'custom:Rust')?.count, 1)
  })

  it('correctly resolves display type for presets and custom types', () => {
    const pPreset: Project = { id: '1', name: 'P1', path: 'C:/p1', project_type: 'laravel' }
    const pCustom: Project = { id: '2', name: 'P2', path: 'C:/p2', project_type: 'custom:Rust' }
    const pAuto: Project = { id: '3', name: 'P3', path: 'C:/p3' }

    assert.deepStrictEqual(getProjectDisplayType(pPreset, samplePresets, sampleCustom), {
      id: 'laravel',
      label: 'Laravel',
      isCustom: false,
    })

    assert.deepStrictEqual(getProjectDisplayType(pCustom, samplePresets, sampleCustom), {
      id: 'rust',
      label: 'Rust',
      isCustom: true,
    })

    const pTauri: Project = { id: '4', name: 'P4', path: 'C:/p4', project_type: 'custom:Tauri' }
    assert.deepStrictEqual(getProjectDisplayType(pTauri, samplePresets, sampleCustom), {
      id: 'tauri',
      label: 'Tauri',
      isCustom: true,
    })

    assert.deepStrictEqual(getProjectDisplayType(pAuto, samplePresets, sampleCustom), {
      id: 'folder',
      label: 'Auto',
      isCustom: false,
    })
  })

  it('honors pinned tabs placing them ahead of higher count tabs', () => {
    const projects: Project[] = [
      { id: '1', name: 'P1', path: 'C:/p1', project_type: 'laravel' },
      { id: '2', name: 'P2', path: 'C:/p2', project_type: 'laravel' },
      { id: '3', name: 'P3', path: 'C:/p3', project_type: 'laravel' },
      { id: '4', name: 'P4', path: 'C:/p4', project_type: 'custom:Rust' },
      { id: '5', name: 'P5', path: 'C:/p5', project_type: 'node' },
      { id: '6', name: 'P6', path: 'C:/p6', project_type: 'node' },
    ]

    const preferences = {
      pinnedIds: ['custom:Rust'],
      orderIds: [],
    }

    const result = groupProjectsByType(projects, samplePresets, sampleCustom, preferences)
    const tabIds = result.tabs.map((t) => t.id)
    assert.deepStrictEqual(tabIds, ['all', 'custom:Rust', 'laravel', 'node'])
  })

  it('honors custom orderIds from drag-to-reorder', () => {
    const projects: Project[] = [
      { id: '1', name: 'P1', path: 'C:/p1', project_type: 'laravel' },
      { id: '2', name: 'P2', path: 'C:/p2', project_type: 'wordpress' },
      { id: '3', name: 'P3', path: 'C:/p3', project_type: 'node' },
    ]

    const preferences = {
      pinnedIds: [],
      orderIds: ['node', 'wordpress', 'laravel'],
    }

    const result = groupProjectsByType(projects, samplePresets, sampleCustom, preferences)
    const tabIds = result.tabs.map((t) => t.id)
    assert.deepStrictEqual(tabIds, ['all', 'node', 'wordpress', 'laravel'])
  })

  it('shows only types with >= 1 project (excluding empty presets and custom types)', () => {
    const projects: Project[] = [{ id: '1', name: 'P1', path: 'C:/p1', project_type: 'laravel' }]

    const result = groupProjectsByType(projects, samplePresets, sampleCustom)
    const tabIds = result.tabs.map((t) => t.id)
    assert.deepStrictEqual(tabIds, ['all', 'laravel'])
  })

  it('keeps position and pin state when a custom type is renamed', () => {
    // Mock localStorage if missing in environment
    if (typeof globalThis.localStorage === 'undefined') {
      const store: Record<string, string> = {}
      // @ts-expect-error test mock
      globalThis.localStorage = {
        getItem: (k: string) => store[k] ?? null,
        setItem: (k: string, v: string) => {
          store[k] = v
        },
        removeItem: (k: string) => {
          delete store[k]
        },
      }
    }

    saveProjectFilterPreferences({
      pinnedIds: ['laravel', 'custom:Rust'],
      orderIds: ['node', 'custom:Rust', 'laravel', 'wordpress'],
    })

    updateRenamedTypeInPreferences('custom:Rust', 'custom:Rust Core')

    const updated = getProjectFilterPreferences()
    // Pin state must be kept at the same position
    assert.deepStrictEqual(updated.pinnedIds, ['laravel', 'custom:Rust Core'])
    // Order state must be kept at the same position
    assert.deepStrictEqual(updated.orderIds, ['node', 'custom:Rust Core', 'laravel', 'wordpress'])
  })
})
