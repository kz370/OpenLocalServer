import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { runCommand } from '@/core'
import type { Web } from '@/lib/web'

/** §27: config files edited by hand are never overwritten without asking. */
export function DriftDialog({ web }: { web: Web }) {
  const { driftOpen, setDriftOpen, run, apply, driftedHosts } = web
  return (
    <Dialog
      open={driftOpen && driftedHosts.length > 0}
      onClose={() => setDriftOpen(false)}
      title="Config files were edited by hand"
      description="These sites' generated config no longer matches what OLS wrote, so it left them alone."
      footer={
        <>
          <Button variant="ghost" onClick={() => setDriftOpen(false)}>
            Cancel
          </Button>
          <Button
            variant="secondary"
            onClick={() =>
              run('keep', async () => {
                for (const h of driftedHosts) await runCommand({ type: 'set_ownership', hostname: h, ownership: 'manual' })
                setDriftOpen(false)
                await apply()
              })
            }
          >
            Keep my edits (switch to Manual)
          </Button>
          <Button
            onClick={() =>
              run('overwrite', async () => {
                setDriftOpen(false)
                await apply(driftedHosts)
              })
            }
          >
            Overwrite (a copy stays in history)
          </Button>
        </>
      }
    >
      <ul className="list-disc pl-5 text-sm">
        {driftedHosts.map((h) => (
          <li key={h}>{h}</li>
        ))}
      </ul>
    </Dialog>
  )
}
