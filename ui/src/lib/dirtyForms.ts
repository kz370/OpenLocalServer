import { useCallback, useEffect, useId, useRef, useState, useSyncExternalStore } from 'react'

import { asDiagnostic } from '@/components/ErrorCard'
import { confirmAction } from '@/lib/confirm'
import type { Diagnostic } from '@/core'

export type DirtyFormScope = 'app' | 'dialog'

export type DirtyFormEntry = {
  id: string
  scope: DirtyFormScope
  dirty: boolean
  busy: boolean
  error: Diagnostic | null
  save: () => Promise<boolean>
  discard: () => void
}

type DirtyFormOptions<T> = {
  scope?: DirtyFormScope
  formKey: string
  value: T
  enabled: boolean
  busy: boolean
  error: Diagnostic | null
  onSave: (value: T) => Promise<boolean>
  onRestore: (value: T) => void
  /** Optional normalizer applied before comparing draft to saved (trim, null-collapse, etc.). */
  normalize?: (value: T) => unknown
}

type SavedSnapshot<T> = {
  key: string
  value: T
  serialized: string
}

const forms = new Map<string, DirtyFormEntry>()
const listeners = new Set<() => void>()
let revision = 0

function notify() {
  revision += 1
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function getRevision() {
  return revision
}

export function getDirtyForms(scope?: DirtyFormScope) {
  return [...forms.values()].filter((form) => form.dirty && (!scope || form.scope === scope))
}

export function useDirtyFormCount(scope: DirtyFormScope) {
  const getSnapshot = useCallback(
    () => getDirtyForms(scope).length,
    [scope],
  )
  return useSyncExternalStore(subscribe, getSnapshot, () => 0)
}

export function useDirtyFormRevision() {
  return useSyncExternalStore(subscribe, getRevision, () => 0)
}

export function useDirtyForm<T>({
  scope = 'app',
  formKey,
  value,
  enabled,
  busy,
  error,
  onSave,
  onRestore,
  normalize,
}: DirtyFormOptions<T>) {
  const generatedId = useId()
  const id = `dirty-form-${generatedId}`
  const normalizeRef = useRef(normalize ?? normalizeValue as (value: T) => unknown)
  useEffect(() => {
    normalizeRef.current = normalize ?? (normalizeValue as (value: T) => unknown)
  }, [normalize])
  const serialize = useCallback((v: T) => JSON.stringify(normalizeRef.current(v)), [])
  const serialized = serialize(value)
  const [baseline, setBaseline] = useStateSnapshot(formKey, value, serialized)
  const baselineRef = useRef(baseline)
  useEffect(() => {
    baselineRef.current = baseline
  }, [baseline])
  const currentDirty = enabled && baseline.key === formKey && baseline.serialized !== serialized
  const valueRef = useLatest(value)
  const keyRef = useLatest(formKey)
  const onSaveRef = useLatest(onSave)
  const onRestoreRef = useLatest(onRestore)
  const errorRef = useLatest(error)
  const dirtyRef = useLatest(currentDirty)

  const updateBaseline = useCallback((key: string, next: T) => {
    const snapshot = { key, value: next, serialized: serialize(next) }
    baselineRef.current = snapshot
    setBaseline(snapshot)
  }, [serialize, setBaseline])

  const syncSaved = useCallback((next: T | ((saved: T) => T), forKey = keyRef.current, force = false) => {
    if (forKey === keyRef.current && (force || !dirtyRef.current)) {
      const currentBaseline = baselineRef.current
      const saved = currentBaseline.key === forKey ? currentBaseline.value : valueRef.current
      updateBaseline(forKey, typeof next === 'function' ? (next as (saved: T) => T)(saved) : next)
    }
  }, [dirtyRef, keyRef, updateBaseline, valueRef])

  const discard = useCallback(() => {
    const currentBaseline = baselineRef.current
    if (currentBaseline.key !== keyRef.current) return
    onRestoreRef.current(currentBaseline.value)
  }, [baselineRef, keyRef, onRestoreRef])

  const save = useCallback(async () => {
    if (!dirtyRef.current) return true
    const submittedValue = valueRef.current
    try {
      const succeeded = await onSaveRef.current(submittedValue)
      if (succeeded) {
        updateBaseline(keyRef.current, submittedValue)
        return true
      }
      if (!errorRef.current) {
        throw {
          problem: 'Could not save changes',
          cause: 'The save operation did not complete.',
          fix: 'Review the form and try again.',
        } satisfies Diagnostic
      }
      return false
    } catch (e) {
      updateError(id, asDiagnostic(e))
      return false
    }
  }, [dirtyRef, errorRef, id, keyRef, onSaveRef, updateBaseline, valueRef])

  useEffect(() => {
    if (baseline.key !== formKey) updateBaseline(formKey, value)
  }, [baseline.key, formKey, serialized, updateBaseline, value])

  useEffect(() => {
    if (!enabled) {
      forms.delete(id)
      notify()
      return
    }

    forms.set(id, {
      id,
      scope,
      dirty: currentDirty,
      busy,
      error: error ?? registeredErrors.get(id) ?? null,
      save,
      discard,
    })
    notify()
    return () => {
      forms.delete(id)
      registeredErrors.delete(id)
      notify()
    }
  }, [busy, currentDirty, discard, enabled, error, id, save, scope])

  useEffect(() => {
    if (!currentDirty) registeredErrors.delete(id)
  }, [currentDirty, id])

  return { dirty: currentDirty, syncSaved }
}

const registeredErrors = new Map<string, Diagnostic>()

function updateError(id: string, error: Diagnostic) {
  registeredErrors.set(id, error)
  const form = forms.get(id)
  if (form) forms.set(id, { ...form, error })
  notify()
}

function useStateSnapshot<T>(key: string, value: T, serialized: string) {
  const [snapshot, setSnapshot] = useState<SavedSnapshot<T>>(() => ({ key, value, serialized }))
  return [snapshot, setSnapshot] as const
}

function useLatest<T>(value: T) {
  const ref = useRef(value)
  useEffect(() => {
    ref.current = value
  }, [value])
  return ref
}

export async function saveDirtyForms(scope?: DirtyFormScope) {
  const pending = getDirtyForms(scope)
  for (const form of pending) {
    if (form.busy || !(await form.save())) return false
  }
  return true
}

export function discardDirtyForms(scope?: DirtyFormScope) {
  for (const form of getDirtyForms(scope)) form.discard()
}

export async function warnBeforeLeavingUnsavedChanges(scope?: DirtyFormScope): Promise<boolean> {
  if (getDirtyForms(scope).length === 0) return true
  const confirmed = await confirmAction('You have unsaved changes. Discard them and leave?', 'Unsaved changes', 'Discard')
  if (confirmed) discardDirtyForms(scope)
  return confirmed
}

export function normalizeValue(value: unknown): unknown {
  if (value === undefined || value === null) return null
  if (typeof value === 'string') {
    const trimmed = value.trim()
    if (trimmed === '') return null
    if (/^-?\d+(\.\d+)?$/.test(trimmed)) return Number(trimmed)
    return trimmed
  }
  if (Array.isArray(value)) return value.map(normalizeValue)
  if (typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const key of Object.keys(value as Record<string, unknown>).sort()) {
      out[key] = normalizeValue((value as Record<string, unknown>)[key])
    }
    return out
  }
  return value
}
export function installDirtyFormGuards() {
  const onKeyDown = (event: KeyboardEvent) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's' && getDirtyForms().length > 0) {
      event.preventDefault()
      void saveDirtyForms()
      return
    }
    const isRefresh = event.key === 'F5' || event.code === 'F5' || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r')
    if (isRefresh && getDirtyForms().length > 0) {
      event.preventDefault()
      event.stopPropagation()
      event.stopImmediatePropagation()
      void confirmAction('You have unsaved changes. Reload the page and discard them?', 'Unsaved changes', 'Reload').then((ok) => {
        if (ok) window.location.reload()
      })
      return false
    }
  }
  window.addEventListener('keydown', onKeyDown, true)
  return () => {
    window.removeEventListener('keydown', onKeyDown, true)
  }
}
