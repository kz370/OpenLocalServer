import { invoke } from '@tauri-apps/api/core'
import type { Diagnostic } from './types/system'
import type { CoreCommand } from './commands'
import type { CoreResponse } from './responses'

export async function runCommand(command: CoreCommand): Promise<CoreResponse> {
  try {
    return await invoke<CoreResponse>('run_command', { command })
  } catch (err) {
    // Tauri surfaces our Diagnostic as the rejection value.
    throw err as Diagnostic
  }
}

/**
 * `runCommand` with a deadline. A command that never answers — a project on a dead network
 * mount, a poisoned core mutex, a saturated blocking pool — otherwise leaves whatever is
 * waiting on it spinning forever, so every read that gates a page goes through this.
 *
 * The timer only rejects the caller; the command keeps running in the core, exactly as it
 * would have without the wrapper.
 */
export function runCommandWithin(command: CoreCommand, ms: number): Promise<CoreResponse> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () =>
        reject({
          problem: 'That did not finish in time.',
          cause: `The core did not answer \`${command.type}\` within ${Math.round(ms / 1000)}s, so it is blocked or unreachable.`,
          fix: 'Check that the folder it reads is still reachable, then retry. A second copy of the app holding the database lock does the same thing.',
        } satisfies Diagnostic),
      ms,
    )
  })
  return Promise.race([runCommand(command), deadline]).finally(() => clearTimeout(timer))
}


// ---- Stage 16: plugins and signed catalogs ---------------------------------------------

