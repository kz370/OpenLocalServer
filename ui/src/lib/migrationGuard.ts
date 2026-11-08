import { runCommand } from '@/core'
import { notify } from '@/lib/notify'

/**
 * §168: after a project command starts, check whether OLS just saved the database before a
 * migration (a copy younger than 15 seconds) and say so, with a Rewind button.
 */
export async function announceGuardCopy(projectId: string | null | undefined): Promise<void> {
  if (!projectId) return
  try {
    const r = await runCommand({ type: 'list_guard_snapshots', project_id: projectId })
    if (r.type !== 'guard_snapshots') return
    const newest = r.snapshots[0]
    if (!newest || Date.now() / 1000 - newest.created > 15) return
    notify.success(`Saved ${newest.database} before the ${newest.kind} migration.`, {
      action: {
        label: 'Rewind',
        onClick: async () => {
          await runCommand({ type: 'rewind_guard_snapshot', id: newest.id })
          notify.info(`Putting ${newest.database} back. Progress is on the Background tasks page.`)
        },
      },
    })
  } catch {
    // The toast is a courtesy; a failure to look must never get in the way of the command.
  }
}
