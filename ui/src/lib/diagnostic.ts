import type { Diagnostic } from '@/core'

export function asDiagnostic(err: unknown): Diagnostic {
  if (err && typeof err === 'object' && 'problem' in err) return err as Diagnostic
  return { problem: 'Something went wrong.', cause: String(err), fix: null }
}
