/**
 * Tooltip string and shortcut parsing helpers.
 */

export interface ParsedTooltip {
  label: string
  shortcut: string | null
}

/**
 * Checks if a string ends with a shortcut like `(Ctrl+R)` or `(⌘+S)` and splits it.
 */
export function parseTooltipShortcut(text: string): ParsedTooltip {
  if (typeof text !== 'string') return { label: text, shortcut: null }
  const match = text.match(/^(.*?)\s*\((Ctrl\+[^)]+|Cmd\+[^)]+|Alt\+[^)]+|Shift\+[^)]+|[⌘⇧⌥⌃][^)]*)\)$/i)
  if (match) {
    return { label: match[1].trim(), shortcut: match[2].trim() }
  }
  return { label: text, shortcut: null }
}

