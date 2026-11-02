import { invoke } from '@tauri-apps/api/core'
import type { Diagnostic } from './types/system'
import type { CoreCommand } from './commands'
import type { CoreResponse } from './responses'

export interface CommandLifecycleHook {
  onBeforeCommand?: (command: CoreCommand) => void
  onCommandSuccess?: (command: CoreCommand, response: CoreResponse) => void
  onCommandError?: (command: CoreCommand, error: Diagnostic) => void
  onBeforeExit?: () => void
}

let lifecycleHook: CommandLifecycleHook | null = null

export function setCommandLifecycleHook(hook: CommandLifecycleHook | null) {
  lifecycleHook = hook
}

export async function runCommand(command: CoreCommand): Promise<CoreResponse> {
  lifecycleHook?.onBeforeCommand?.(command)
  try {
    const response = await invoke<CoreResponse>('run_command', { command })
    lifecycleHook?.onCommandSuccess?.(command, response)
    return response
  } catch (err) {
    // Tauri surfaces our Diagnostic as the rejection value.
    const diag = err as Diagnostic
    lifecycleHook?.onCommandError?.(command, diag)
    throw diag
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

/**
 * Requests graceful shutdown of OpenLocalServer via Tauri.
 */
export async function requestExit(): Promise<void> {
  lifecycleHook?.onBeforeExit?.()
  try {
    await invoke('request_exit')
  } catch {
    // ignore
  }
}
