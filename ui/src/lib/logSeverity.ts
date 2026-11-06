export type Severity = 'error' | 'warn' | 'info' | 'debug'

/** Best-effort severity from JSON logs (`"level":"WARN"`) and plain text (`[error]`, `ERROR:`). */
export function severityOf(line: string): Severity {
  const l = line.toLowerCase()
  if (/"level":"(error|fatal)"|\[(emerg|alert|crit|error)\]|\berror\b|✗|failed|fatal/.test(l)) return 'error'
  if (/"level":"warn|\[warn\]|\bwarn(ing)?\b/.test(l)) return 'warn'
  if (/"level":"debug|"level":"trace|\[debug\]/.test(l)) return 'debug'
  return 'info'
}

export const SEVERITY_COLOR: Record<Severity, string> = {
  error: 'text-destructive',
  warn: 'text-warning',
  info: '',
  debug: 'text-muted-foreground',
}
