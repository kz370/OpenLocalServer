import assert from 'node:assert'
import { describe, it } from 'node:test'
import { getProjectDisplayType, groupProjectsByType } from '../projectTypes.ts'
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

    // Tabs should include All, Auto, Laravel, and custom:Rust
    const tabIds = result.tabs.map((t) => t.id)
    assert.deepStrictEqual(tabIds, ['all', 'auto', 'laravel', 'custom:Rust'])

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
})
