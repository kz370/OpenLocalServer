import assert from 'node:assert'
import { test } from 'node:test'

import { keepsNativeMenu } from '../nativeContextMenu.ts'

/** A stand-in element: `closest` matches when the selector names one of `matches`. */
function el(...matches: string[]) {
  return {
    closest: (selector: string) =>
      matches.some((m) => selector.split(',').some((s) => s.trim().startsWith(m))) ? {} : null,
  }
}

test('text fields and editors keep cut / copy / paste', () => {
  assert.ok(keepsNativeMenu(el('input:not')))
  assert.ok(keepsNativeMenu(el('textarea')))
  assert.ok(keepsNativeMenu(el('[contenteditable="true"]')))
  assert.ok(keepsNativeMenu(el('.cm-editor')))
})

test('everything else gets no browser menu', () => {
  assert.ok(!keepsNativeMenu(el()))
  assert.ok(!keepsNativeMenu(null))
  assert.ok(!keepsNativeMenu({}), 'a target without closest (the document, a text node)')
})
