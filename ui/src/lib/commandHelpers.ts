import type { CommandSource, QuickCommand } from '@/core'

/** Brand icon for each discovered source. */
export const SOURCE_ICON: Record<string, string> = {
  artisan: 'laravel',
  console: 'symfony',
  composer: 'composer',
  django: 'django',
}
/** Discovery runs the project's tools, which takes a second or two: keep it for the session. */
export const sourceCache = new Map<string, CommandSource[]>()

/** Quotes one argument the way the backend's command-line splitter reads it back. */
export function quote(arg: string): string {
  return arg === '' || /[\s"']/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg
}

export const commandLine = (c: QuickCommand) => (c.command?.executable ? [c.command.executable, ...c.command.arguments].map(quote).join(' ') : '')
