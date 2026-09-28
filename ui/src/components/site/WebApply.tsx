import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { runCommand } from '@/core'
import type { Web } from '@/lib/web'

/** What the last "apply" did, per server: started or reloaded, files written, warnings. */
export function ApplyReportCard({ web }: { web: Web }) {
  const { reports, setReports } = web
  if (reports.length === 0) return null
  return (
    <Card className="border-success/40">
      <CardHeader className="flex-row items-start justify-between space-y-0 pb-2">
        <CardTitle className="text-sm">
          {reports.length === 1 ? `Applied to ${reports[0].server}` : `Applied to ${reports.length} web servers`}
        </CardTitle>
        <button className="text-xs text-muted-foreground" onClick={() => setReports([])}>
          Dismiss
        </button>
      </CardHeader>
      <CardContent className="flex flex-col gap-2 text-sm text-muted-foreground">
        {reports.map((r) => (
          <div key={r.server} className="flex flex-col gap-1">
            <div>
              <span className="text-foreground">{r.server}</span>
              {r.started ? ' — started. ' : r.reloaded ? ' — reloaded. ' : ''}
              {r.written.length > 0 ? `Updated: ${r.written.join(', ')}.` : 'Config already up to date.'}
            </div>
            {r.warnings.map((w) => (
              <div key={w} className="text-warning">
                {w}
              </div>
            ))}
          </div>
        ))}
        {reports.some((r) => r.hosts_updated) && <div>Hosts file updated.</div>}
      </CardContent>
    </Card>
  )
}

/** §27: config files edited by hand are never overwritten without asking. */
export function DriftDialog({ web }: { web: Web }) {
  const { driftOpen, setDriftOpen, run, apply, driftedHosts } = web
  return (
    <Dialog
      open={driftOpen && driftedHosts.length > 0}
      onClose={() => setDriftOpen(false)}
      title="Config files were edited by hand"
      description="These sites' generated config no longer matches what OpenLocalServer wrote, so it left them alone."
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
