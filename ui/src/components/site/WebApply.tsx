import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog } from '@/components/ui/dialog'
import { runCommand } from '@/core'
import type { Web } from '@/lib/web'

/** What the last "apply" did: server started or reloaded, files written, warnings. */
export function ApplyReportCard({ web }: { web: Web }) {
  const { report, setReport } = web
  if (!report) return null
  return (
    <Card className="border-success/40">
      <CardHeader className="flex-row items-start justify-between space-y-0 pb-2">
        <CardTitle className="text-sm">Applied to {report.server}</CardTitle>
        <button className="text-xs text-muted-foreground" onClick={() => setReport(null)}>
          Dismiss
        </button>
      </CardHeader>
      <CardContent className="flex flex-col gap-1 text-sm text-muted-foreground">
        <div>
          {report.started ? 'Server started. ' : report.reloaded ? 'Server reloaded. ' : ''}
          {report.written.length > 0 ? `Updated: ${report.written.join(', ')}. ` : 'Config already up to date. '}
          {report.hosts_updated && 'Hosts file updated.'}
        </div>
        {report.warnings.map((w) => (
          <div key={w} className="text-warning">
            {w}
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

/** §27: config files edited by hand are never overwritten without asking. */
export function DriftDialog({ web }: { web: Web }) {
  const { report, driftOpen, setDriftOpen, run, apply } = web
  return (
    <Dialog
      open={driftOpen && !!report && report.drifted.length > 0}
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
                for (const h of report?.drifted ?? []) await runCommand({ type: 'set_ownership', hostname: h, ownership: 'manual' })
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
                await apply(report?.drifted ?? [])
              })
            }
          >
            Overwrite (a copy stays in history)
          </Button>
        </>
      }
    >
      <ul className="list-disc pl-5 text-sm">
        {report?.drifted.map((h) => (
          <li key={h}>{h}</li>
        ))}
      </ul>
    </Dialog>
  )
}
