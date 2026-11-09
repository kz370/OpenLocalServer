import test from 'node:test'
import assert from 'node:assert/strict'
import { summarizeEditorRefresh } from './editorRefresh.ts'

const vscode = { id: 'vscode', name: 'VS Code', path: 'C:\\Code\\Code.exe' }
const kiro = { id: 'kiro', name: 'Kiro', path: 'D:\\Kiro\\Kiro.exe' }
const zedMissing = { id: 'zed', name: 'Zed', path: null }

test('reports nothing new when the installed list is unchanged', () => {
  assert.deepEqual(summarizeEditorRefresh([vscode, zedMissing], [vscode, zedMissing]), {
    kind: 'info',
    message: 'No new editors found. 1 editor available.',
  })
})

test('names editors that appeared', () => {
  const r = summarizeEditorRefresh([vscode, zedMissing], [vscode, kiro, zedMissing])
  assert.equal(r.kind, 'success')
  assert.equal(r.message, 'Found 1 new editor: Kiro. 2 editors available.')
})

test('names editors that are gone', () => {
  const r = summarizeEditorRefresh([vscode, kiro], [vscode, { ...kiro, path: null }])
  assert.equal(r.kind, 'success')
  assert.equal(r.message, 'No longer found: Kiro. 1 editor available.')
})

test('says how to add one when nothing is installed', () => {
  const r = summarizeEditorRefresh([], [zedMissing])
  assert.equal(r.kind, 'info')
  assert.match(r.message, /^No editors found\./)
})
