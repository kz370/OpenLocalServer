import { Activity, Download, Search, Zap } from 'lucide-react'
import { useMemo } from 'react'

import { SaveButton, type SaveAction } from '@/components/SaveButton'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import type { PluginInfo } from '@/core'
import { buildPluginConsentSummary } from '@/lib/pluginConsent'

export interface PluginConsentDialogProps {
  plugin: PluginInfo | null
  open: boolean
  onClose: () => void
  onAllowAndTurnOn: () => Promise<void>
  action: SaveAction
}

/**
 * Objective, specific consent dialog before enabling a plugin (§133–135).
 * Generates verified explanations from manifest data (runtimes, verified hostnames,
 * detection markers, raw permissions), preventing deceptive plugin descriptions.
 */
export function PluginConsentDialog({
  plugin,
  open,
  onClose,
  onAllowAndTurnOn,
  action,
}: PluginConsentDialogProps) {
  const summary = useMemo(() => {
    if (!plugin) return null
    return buildPluginConsentSummary(plugin.manifest)
  }, [plugin])

  if (!plugin || !summary) return null

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={`Turn on ${plugin.manifest.name}?`}
      description="Review what this plugin can do and what it adds to OpenLocalServer."
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={action.busy !== null}>
            Cancel
          </Button>
          <SaveButton
            action={action}
            name="on"
            busyLabel="Turning on…"
            savedLabel="Turned on"
            onClick={onAllowAndTurnOn}
          >
            Allow and turn on
          </SaveButton>
        </>
      }
    >
      <div className="flex flex-col gap-3.5 text-sm">
        {summary.runtimes.length > 0 && (
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-[var(--dialog-card,#f8fafc)] dark:bg-[var(--dialog-card,#121c32)] p-3">
            <div className="flex items-center gap-2 font-medium text-foreground">
              <Download className="size-4 text-primary shrink-0" />
              <span>Runtime downloads</span>
            </div>
            <div className="flex flex-col gap-2.5 pl-6">
              {summary.runtimes.map((r) => (
                <div key={r.id || r.name} className="flex flex-col gap-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-semibold text-foreground">
                      {r.name} {r.version}
                      {r.sizeText ? `, ${r.sizeText},` : ''}
                    </span>
                    <span className="text-xs text-muted-foreground">from</span>
                    <code
                      title={r.url}
                      className="rounded border border-border/60 bg-muted px-1.5 py-0.5 font-mono text-xs text-foreground cursor-help"
                    >
                      {r.source}
                    </code>
                  </div>
                  <p className="text-xs text-muted-foreground">{r.explanation}</p>
                  <details className="text-xs text-muted-foreground group">
                    <summary className="cursor-pointer hover:text-foreground inline-flex items-center gap-1 select-none font-medium">
                      <span>View download URL</span>
                    </summary>
                    <div className="mt-1 break-all rounded bg-muted/80 p-2 font-mono text-[11px] text-foreground border border-border/40 select-all">
                      {r.url}
                    </div>
                  </details>
                </div>
              ))}
            </div>
          </div>
        )}

        {summary.detections && summary.detections.items.length > 0 && (
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-[var(--dialog-card,#f8fafc)] dark:bg-[var(--dialog-card,#121c32)] p-3">
            <div className="flex items-center gap-2 font-medium text-foreground">
              <Search className="size-4 text-primary shrink-0" />
              <span>Project detection</span>
            </div>
            <div className="flex flex-col gap-2 pl-6">
              {summary.detections.items.map((d) => (
                <div key={d.name} className="flex flex-wrap items-center gap-1.5 text-xs">
                  <span className="font-semibold text-foreground">{d.name}:</span>
                  <span className="text-muted-foreground">looks for</span>
                  {d.markers.map((m) => (
                    <code key={m} className="rounded border border-border/60 bg-muted px-1.5 py-0.5 font-mono text-foreground">
                      {m}
                    </code>
                  ))}
                </div>
              ))}
              <p className="border-t border-border/40 pt-1 text-xs text-muted-foreground">
                {summary.detections.explanation}
              </p>
            </div>
          </div>
        )}

        {summary.quickApps && (
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-[var(--dialog-card,#f8fafc)] dark:bg-[var(--dialog-card,#121c32)] p-3">
            <div className="flex items-center gap-2 font-medium text-foreground">
              <Zap className="size-4 text-primary shrink-0" />
              <span>Quick Apps</span>
            </div>
            <div className="flex flex-col gap-1 pl-6 text-xs text-muted-foreground">
              <div>
                Folder:{' '}
                <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">
                  {summary.quickApps.folder}
                </code>
              </div>
              <p>{summary.quickApps.explanation}</p>
            </div>
          </div>
        )}

        {summary.healthChecks.length > 0 && (
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-[var(--dialog-card,#f8fafc)] dark:bg-[var(--dialog-card,#121c32)] p-3">
            <div className="flex items-center gap-2 font-medium text-foreground">
              <Activity className="size-4 text-primary shrink-0" />
              <span>Health checks</span>
            </div>
            <div className="flex flex-col gap-1 pl-6 text-xs">
              {summary.healthChecks.map((h) => (
                <div key={h.name} className="flex flex-wrap items-center gap-1.5">
                  <span className="font-medium text-foreground">{h.name}:</span>
                  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">{h.target}</code>
                </div>
              ))}
              <p className="pt-1 text-muted-foreground">{summary.healthChecks[0]?.explanation}</p>
            </div>
          </div>
        )}

        {summary.runtimes.length === 0 && !summary.detections && !summary.quickApps && summary.healthChecks.length === 0 && (
          <p className="text-sm text-muted-foreground">This plugin requests no permissions and adds no extra capabilities.</p>
        )}

        {summary.rawPermissions.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            <span className="font-medium text-muted-foreground">Details:</span>
            <span>Permissions requested:</span>
            {summary.rawPermissions.map((p) => (
              <code key={p} className="rounded border border-border/40 bg-muted/60 px-1 py-0.5 font-mono text-foreground">
                {p}
              </code>
            ))}
            {plugin.folder && <span className="ml-1">• Installed in {plugin.folder}</span>}
          </div>
        )}

        <p className="border-t border-border pt-2 text-xs leading-relaxed text-muted-foreground">
          {summary.closingSentence}
        </p>
      </div>
    </Dialog>
  )
}
