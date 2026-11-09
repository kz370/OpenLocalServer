/** Setting that holds the security items the user chose to hide. */
export const SECURITY_IGNORED_SETTING = 'security.ignored'

/** Key for an ended runtime version, e.g. `eol:php:8.1.34`. */
export const eolKey = (runtime: string, version: string) => `eol:${runtime}:${version}`

/** Key for one package finding in one project. Stable across re-checks of the same problem. */
export function findingKey(projectId: string, tool: string, f: { package: string; advisory?: string | null; title: string }) {
  return `finding:${projectId}:${tool}:${f.package}:${f.advisory ?? f.title}`
}

/** Reads the saved setting value; anything that is not a list of strings is treated as empty. */
export function parseIgnored(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : []
}

/** The list with all `keys` added, or removed when `hidden` is false. No duplicates. */
export function withManyIgnored(list: string[], keys: string[], hidden: boolean): string[] {
  const keySet = new Set(keys)
  const rest = list.filter((k) => !keySet.has(k))
  return hidden ? [...rest, ...Array.from(keySet)] : rest
}

/** The list with `key` added, or removed when `hidden` is false. No duplicates. */
export function withIgnored(list: string[], key: string, hidden: boolean): string[] {
  return withManyIgnored(list, [key], hidden)
}
