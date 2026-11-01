import type { ExternalTool } from '../core/types/web.ts'

export const KNOWN_ENGINES = ['mariadb', 'postgres', 'mongodb', 'redis', 'memcached', 'sqlite'] as const
export type KnownEngine = (typeof KNOWN_ENGINES)[number]

export const ALLOWED_PLACEHOLDERS = ['{uri}', '{path}', '{host}', '{port}', '{user}', '{database}'] as const

export const CURRENT_SCHEMA_VERSION = 1

export interface ToolDefinition {
  id: string
  name: string
  engines: string[]
  launchMode: 'args' | 'clipboard'
  args: string[]
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

    const args: string[] = []
    if (Array.isArray(t.args)) {
      for (const a of t.args) {
        if (typeof a !== 'string') continue
        // Validate placeholders in arg
        const placeholders = a.match(/\{[^}]+\}/g) ?? []
        for (const p of placeholders) {
          if (!ALLOWED_PLACEHOLDERS.includes(p as (typeof ALLOWED_PLACEHOLDERS)[number])) {
            return {
              ok: false,
              problem: `Unknown argument placeholder "${p}" in tool "${t.id}".`,
              cause: `Placeholder "${p}" is not recognized.`,
              fix: `Allowed placeholders are: ${ALLOWED_PLACEHOLDERS.join(', ')}.`,
            }
          }
        }
        args.push(a)
      }
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
