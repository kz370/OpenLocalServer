import type { ExternalTool, ToolArgs } from '../core/types/web.ts'

export const KNOWN_ENGINES = ['mariadb', 'postgres', 'mongodb', 'redis', 'memcached', 'sqlite'] as const
export type KnownEngine = (typeof KNOWN_ENGINES)[number]

export const ALLOWED_PLACEHOLDERS = ['{uri}', '{path}', '{host}', '{port}', '{user}', '{database}'] as const

/** Key of a per-engine args map that applies to any engine without its own entry. */
export const DEFAULT_ARGS_KEY = 'default'

export const CURRENT_SCHEMA_VERSION = 1

export interface ToolDefinition {
  id: string
  name: string
  engines: string[]
  launchMode: 'args' | 'clipboard'
  /** One argument list for every engine, or a map of engine id to its own argument list. */
  args: ToolArgs
}

export function isPerEngineArgs(args: ToolArgs | undefined | null): args is Record<string, string[]> {
  return !!args && !Array.isArray(args) && typeof args === 'object'
}

/** True when the tool has no arguments at all. */
export function argsAreEmpty(args: ToolArgs | undefined | null): boolean {
  if (!args) return true
  if (isPerEngineArgs(args)) return Object.values(args).every((list) => list.length === 0)
  return args.length === 0
}

/** The argument list used for one engine (exact engine entry, else `default`, else none). */
export function argsForEngine(args: ToolArgs | undefined | null, engine: string): string[] {
  if (!args) return []
  if (!isPerEngineArgs(args)) return args
  return args[engine] ?? args[DEFAULT_ARGS_KEY] ?? []
}

/** One-line display form of an argument list (arguments containing spaces are quoted). */
export function joinArgs(list: string[]): string {
  return list.map((a) => (/\s/.test(a) && !a.includes('"') ? `"${a}"` : a)).join(' ')
}

/** The engine sections the args editor shows: one per ticked engine, or just `default` when none is ticked. */
export function argsSections(engines: string[]): string[] {
  return engines.length > 0 ? engines : [DEFAULT_ARGS_KEY]
}

/** Turns args into a per-engine map with one entry per section; a plain list is copied into each. */
export function toPerEngineArgs(args: ToolArgs, sections: string[]): Record<string, string[]> {
  const map: Record<string, string[]> = {}
  for (const section of sections) map[section] = [...argsForEngine(args, section)]
  return map
}

/** Collapses args to one plain list (the first non-empty entry of a per-engine map). */
export function toPlainArgs(args: ToolArgs): string[] {
  if (!isPerEngineArgs(args)) return [...args]
  return [...(Object.values(args).find((list) => list.length > 0) ?? [])]
}

/** Drops empty arguments and map entries for sections that are no longer shown. */
export function cleanArgs(args: ToolArgs, sections: string[]): ToolArgs {
  const clean = (list: string[]) => list.map((a) => a.trim()).filter(Boolean)
  if (!isPerEngineArgs(args)) return clean(args)
  const map: Record<string, string[]> = {}
  for (const section of sections) {
    const list = clean(args[section] ?? [])
    if (list.length > 0) map[section] = list
  }
  return map
}
/** Short one-line description of a tool's args for lists. */
export function summarizeArgs(args: ToolArgs | undefined | null): string {
  if (!args) return ''
  if (!isPerEngineArgs(args)) return joinArgs(args)
  return Object.entries(args)
    .map(([engine, list]) => `${engine}: ${joinArgs(list)}`)
    .join(' | ')
}

/** Returns the first unknown `{placeholder}` found in the list, or null when all are allowed. */
function checkPlaceholders(list: string[]): string | null {
  for (const a of list) {
    for (const p of a.match(/\{[^}]+\}/g) ?? []) {
      if (!ALLOWED_PLACEHOLDERS.includes(p as (typeof ALLOWED_PLACEHOLDERS)[number])) return p
    }
  }
  return null
}

export interface ToolsFile {
  schemaVersion: number
  tools: ToolDefinition[]
}

export interface ValidationSuccess {
  ok: true
  schemaVersion: number
  tools: ToolDefinition[]
}

export interface ValidationFailure {
  ok: false
  problem: string
  cause: string
  fix: string
}

export type ValidationResult = ValidationSuccess | ValidationFailure

/**
 * Validates an import JSON file according to the External Tools schema.
 * Rejects unknown engines and unknown placeholders.
 * Strips any executable field that may be present in the file.
 */
export function validateImportJson(raw: string): ValidationResult {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (err) {
    return {
      ok: false,
      problem: 'Invalid JSON file.',
      cause: err instanceof Error ? err.message : 'The file could not be parsed as JSON.',
      fix: 'Select a valid JSON file exported from OpenLocalServer or following the tools schema.',
    }
  }

  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return {
      ok: false,
      problem: 'Invalid file format.',
      cause: 'The file root must be a JSON object.',
      fix: 'Ensure the file contains {"schemaVersion": 1, "tools": [...]}.',
    }
  }

  const file = parsed as Record<string, unknown>

  if (!('schemaVersion' in file) || typeof file.schemaVersion !== 'number' || !Number.isInteger(file.schemaVersion)) {
    return {
      ok: false,
      problem: 'Missing or invalid schemaVersion.',
      cause: 'schemaVersion must be an integer.',
      fix: 'Add "schemaVersion": 1 to the root of the file.',
    }
  }

  if (file.schemaVersion > CURRENT_SCHEMA_VERSION) {
    return {
      ok: false,
      problem: 'Unsupported schema version.',
      cause: `This file uses schemaVersion ${file.schemaVersion}, but this version of OpenLocalServer only supports version ${CURRENT_SCHEMA_VERSION}.`,
      fix: 'Please update OpenLocalServer to import this file.',
    }
  }

  if (!Array.isArray(file.tools)) {
    return {
      ok: false,
      problem: 'Invalid tools list.',
      cause: '"tools" property must be an array.',
      fix: 'Provide a list of tools under the "tools" key.',
    }
  }

  const validTools: ToolDefinition[] = []

  for (let i = 0; i < file.tools.length; i++) {
    const rawTool = file.tools[i]
    if (!rawTool || typeof rawTool !== 'object' || Array.isArray(rawTool)) {
      return {
        ok: false,
        problem: `Invalid tool at index ${i}.`,
        cause: 'Each item in "tools" must be an object.',
        fix: 'Ensure every tool has an id, name, engines, and launchMode.',
      }
    }

    const t = rawTool as Record<string, unknown>

    if (typeof t.id !== 'string' || !t.id.trim()) {
      return {
        ok: false,
        problem: `Invalid or missing "id" at tool #${i + 1}.`,
        cause: 'Tool id must be a non-empty string.',
        fix: 'Specify a valid identifier (e.g. "dbeaver").',
      }
    }

    if (typeof t.name !== 'string' || !t.name.trim()) {
      return {
        ok: false,
        problem: `Invalid or missing "name" for tool "${t.id}".`,
        cause: 'Tool name must be a non-empty string.',
        fix: 'Provide a display name for the tool.',
      }
    }

    if (!Array.isArray(t.engines)) {
      return {
        ok: false,
        problem: `Invalid engines list for tool "${t.id}".`,
        cause: '"engines" must be an array of strings.',
        fix: `Valid engines are: ${KNOWN_ENGINES.join(', ')}.`,
      }
    }

    const engines: string[] = []
    for (const e of t.engines) {
      if (typeof e !== 'string') {
        return {
          ok: false,
          problem: `Invalid engine entry in tool "${t.id}".`,
          cause: 'Each engine in the array must be a string.',
          fix: `Valid engines are: ${KNOWN_ENGINES.join(', ')}.`,
        }
      }
      const normalized = e.trim().toLowerCase()
      if (!KNOWN_ENGINES.includes(normalized as KnownEngine)) {
        return {
          ok: false,
          problem: `Unknown database engine "${e}" in tool "${t.id}".`,
          cause: `"${e}" is not a recognized database engine in OpenLocalServer.`,
          fix: `Use only supported engines: ${KNOWN_ENGINES.join(', ')}.`,
        }
      }
      if (!engines.includes(normalized)) {
        engines.push(normalized)
      }
    }

    const launchMode = t.launchMode === 'clipboard' ? 'clipboard' : 'args'

    const unknownPlaceholder = (list: string[]): ValidationFailure | null => {
      const p = checkPlaceholders(list)
      return p
        ? {
            ok: false,
            problem: `Unknown argument placeholder "${p}" in tool "${t.id as string}".`,
            cause: `Placeholder "${p}" is not recognized.`,
            fix: `Allowed placeholders are: ${ALLOWED_PLACEHOLDERS.join(', ')}.`,
          }
        : null
    }

    let args: ToolArgs = []
    if (Array.isArray(t.args)) {
      const list = t.args.filter((a): a is string => typeof a === 'string')
      const bad = unknownPlaceholder(list)
      if (bad) return bad
      args = list
    } else if (t.args && typeof t.args === 'object') {
      const map: Record<string, string[]> = {}
      for (const [rawKey, value] of Object.entries(t.args as Record<string, unknown>)) {
        const key = rawKey.trim().toLowerCase()
        if (!KNOWN_ENGINES.includes(key as KnownEngine) && key !== DEFAULT_ARGS_KEY) {
          return {
            ok: false,
            problem: `Unknown engine "${rawKey}" in the args of tool "${t.id}".`,
            cause: `"${rawKey}" is not a recognized database engine in OpenLocalServer.`,
            fix: `Key each argument list by one of: ${KNOWN_ENGINES.join(', ')}, or "${DEFAULT_ARGS_KEY}".`,
          }
        }
        if (!Array.isArray(value)) {
          return {
            ok: false,
            problem: `Invalid args for engine "${rawKey}" in tool "${t.id}".`,
            cause: 'Each engine entry must be an array of strings.',
            fix: `Write it like "${rawKey}": ["-con", "{uri}"].`,
          }
        }
        const list = value.filter((a): a is string => typeof a === 'string')
        const bad = unknownPlaceholder(list)
        if (bad) return bad
        map[key] = list
      }
      args = map
    }

    validTools.push({
      id: t.id.trim().toLowerCase(),
      name: t.name.trim(),
      engines,
      launchMode,
      args,
    })
  }

  return {
    ok: true,
    schemaVersion: file.schemaVersion,
    tools: validTools,
  }
}

/**
 * Exports registered external tools to a clean JSON string with schemaVersion: 1.
 * Local executable paths are deliberately omitted.
 */
export function exportToolsJson(tools: ExternalTool[]): string {
  const exported: ToolsFile = {
    schemaVersion: CURRENT_SCHEMA_VERSION,
    tools: tools.map((t) => ({
      id: t.id,
      name: t.name,
      engines: t.engines,
      launchMode: t.launchMode ?? 'args',
      args: t.args ?? [],
    })),
  }
  return JSON.stringify(exported, null, 2)
}

/**
 * Returns placeholder chips valid for the selected engines.
 * {uri} is valid for any engine.
 * {path} is for SQLite.
 * {host}, {port}, {user}, {database} are for network engines.
 */
export function getValidPlaceholderChips(selectedEngines: string[]): Array<{ token: string; label: string }> {
  const hasSqlite = selectedEngines.length === 0 || selectedEngines.includes('sqlite')
  const hasNetwork =
    selectedEngines.length === 0 ||
    selectedEngines.some((e) => ['mariadb', 'postgres', 'mongodb', 'redis', 'memcached'].includes(e))

  const chips: Array<{ token: string; label: string }> = []

  chips.push({ token: '{uri}', label: 'Full connection URI' })

  if (hasNetwork) {
    chips.push({ token: '{host}', label: 'Host (127.0.0.1)' })
    chips.push({ token: '{port}', label: 'Port number' })
    chips.push({ token: '{user}', label: 'Username' })
    chips.push({ token: '{database}', label: 'Database name' })
  }

  if (hasSqlite) {
    chips.push({ token: '{path}', label: 'SQLite file path' })
  }

  return chips
}

/**
 * Generates a clean URL/slug ID from a display name.
 */
export function slugify(name: string): string {
  return name
    .toLowerCase()
    .trim()
    .replace(/[^\w\s-]/g, '')
    .replace(/[\s_-]+/g, '-')
    .replace(/^-+|-+$/g, '')
}
