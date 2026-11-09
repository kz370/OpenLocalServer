import test from 'node:test'
import assert from 'node:assert/strict'
import { eolKey, findingKey, parseIgnored, withIgnored, withManyIgnored } from './securityIgnore.ts'

test('parseIgnored keeps only strings and tolerates junk', () => {
  assert.deepEqual(parseIgnored(['a', 1, null, 'b']), ['a', 'b'])
  assert.deepEqual(parseIgnored(undefined), [])
  assert.deepEqual(parseIgnored('x'), [])
})

test('withIgnored adds once and removes', () => {
  assert.deepEqual(withIgnored(['a'], 'b', true), ['a', 'b'])
  assert.deepEqual(withIgnored(['a', 'b'], 'b', true), ['a', 'b'])
  assert.deepEqual(withIgnored(['a', 'b'], 'a', false), ['b'])
})

test('withManyIgnored adds multiple keys without duplicates and removes them', () => {
  assert.deepEqual(withManyIgnored(['a'], ['b', 'c', 'b'], true), ['a', 'b', 'c'])
  assert.deepEqual(withManyIgnored(['a', 'b', 'c', 'd'], ['a', 'c'], false), ['b', 'd'])
})

test('keys are stable and distinguish version, project and advisory', () => {
  assert.equal(eolKey('php', '8.1.34'), 'eol:php:8.1.34')
  const f = { package: 'x/y', advisory: 'GHSA-1', title: 't' }
  assert.equal(findingKey('p1', 'composer', f), 'finding:p1:composer:x/y:GHSA-1')
  assert.notEqual(findingKey('p2', 'composer', f), findingKey('p1', 'composer', f))
  assert.equal(findingKey('p1', 'npm', { package: 'a', title: 'T' }), 'finding:p1:npm:a:T')
})
