import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
import { ChevronRight, Copy, Download, Eye, EyeOff, FolderOpen, ShieldAlert } from 'lucide-react'
import { useState } from 'react'
import { createPortal } from 'react-dom'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { type AuditFinding, type AuditResult, type EolItem, runCommand } from '@/core'
import { eolLabel } from '@/lib/hooks'
import { notify } from '@/lib/notify'
import { type ExportItem, formatFinding, formatReport } from '@/lib/securityExport'
import { eolKey, findingKey } from '@/lib/securityIgnore'

const SEVERITY_VARIANT: Record<string, 'destructive' | 'warning' | 'secondary' | 'outline'> = {
  critical: 'destructive',
  high: 'destructive',
  moderate: 'warning',
  low: 'secondary',
  unknown: 'outline',
}
const SEVERITY_ORDER = ['critical', 'high', 'moderate', 'low', 'unknown']
const rank = (s: string) => {
  const i = SEVERITY_ORDER.indexOf(s)
  return i === -1 ? SEVERITY_ORDER.length : i
}

/**
 * Every security problem the last checks found, in detail: versions that no longer get
 * fixes, and package problems per project with severity, the affected range, the fix and a
 * link. Each item can be hidden (or shown again). Text from the registries is plain text.
 */
export function SecurityDetailsDialog({
  open,
  onClose,
  results,
  eol,
  projectNames,
  ignored,
  setHidden,
  setManyHidden,
  onError,
}: {
  open: boolean
  onClose: () => void
  results: AuditResult[]
  eol: EolItem[]
  projectNames: Record<string, string>
  ignored: string[]
  setHidden: (key: string, hidden: boolean) => void
  setManyHidden?: (keys: string[], hidden: boolean) => void
  onError: (e: unknown) => void
}) {
  const [showHidden, setShowHidden] = useState(false)
  // Keys of items explicitly deselected by the user. By default (empty Set), everything is selected.
  const [deselected, setDeselected] = useState<Set<string>>(() => new Set())
  const isSelected = (key: string) => !deselected.has(key)

  const ended = eol.filter((i) => i.status.state === 'ended')
  const groups = results
    .map((r) => ({
      id: r.project_id,
      name: projectNames[r.project_id] ?? r.project_id,
      items: r.tools
        .flatMap((t) => t.findings.map((f) => ({ tool: t.tool, f, key: findingKey(r.project_id, t.tool, f) })))
        .filter((x) => showHidden || !ignored.includes(x.key))
        .sort((a, b) => rank(a.f.severity) - rank(b.f.severity)),
    }))
    .filter((g) => g.items.length > 0)
  const visibleEnded = ended.filter((i) => showHidden || !ignored.includes(eolKey(i.runtime, i.version)))
  const versionKeys = visibleEnded.map((i) => eolKey(i.runtime, i.version))
  const hiddenTotal =
    ended.filter((i) => ignored.includes(eolKey(i.runtime, i.version))).length +
    results.reduce((n, r) => n + r.tools.reduce((m, t) => m + t.findings.filter((f) => ignored.includes(findingKey(r.project_id, t.tool, f))).length, 0), 0)

  const allVisibleKeys = [
    ...versionKeys,
    ...groups.flatMap((g) => g.items.map((x) => x.key)),
  ]
  const selectedKeys = allVisibleKeys.filter((k) => !deselected.has(k))
  const selectedToHide = selectedKeys.filter((k) => !ignored.includes(k))

  const toggleItem = (key: string) => {
    setDeselected((prev) => {
      const next = new Set(prev)
      if (next.has(key)) {
        next.delete(key)
      } else {
        next.add(key)
      }
      return next
    })
  }

  const toggleGroup = (keys: string[]) => {
    setDeselected((prev) => {
      const next = new Set(prev)
      const allGroupSelected = keys.length > 0 && keys.every((k) => !prev.has(k))
      if (allGroupSelected) {
        for (const k of keys) next.add(k)
      } else {
        for (const k of keys) next.delete(k)
      }
      return next
    })
  }

  const allSelected = allVisibleKeys.length > 0 && selectedKeys.length === allVisibleKeys.length

  const toggleSelectAll = () => {
    if (allSelected) {
      setDeselected(new Set(allVisibleKeys))
    } else {
      setDeselected(new Set())
    }
  }

  const handleHideSelected = () => {
    if (selectedToHide.length === 0) return
    if (setManyHidden) {
      setManyHidden(selectedToHide, true)
    } else {
      for (const k of selectedToHide) setHidden(k, true)
    }
    notify.success(`Hidden ${selectedToHide.length} selected security issue${selectedToHide.length === 1 ? '' : 's'}.`)
  }

  /** What the user sees right now (hidden items only when shown), as plain data for copy and export. */
  const exportItems: ExportItem[] = groups.flatMap((g) =>
    g.items.filter((x) => isSelected(x.key)).map((x) => ({ project: g.name, tool: x.tool, f: x.f }))
  )
  const exportEnded = visibleEnded
    .filter((i) => isSelected(eolKey(i.runtime, i.version)))
    .map((i) => ({ runtime: i.runtime, version: i.version, usedBy: i.used_by }))

  async function copyText(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text)
      notify.success(`Copied ${what}.`)
    } catch (e) {
      onError(e)
    }
  }

  async function exportAll() {
    try {
      const dest = await save({ title: 'Export security problems', defaultPath: 'ols-security-report.md', filters: [{ name: 'Markdown', extensions: ['md'] }] })
      if (!dest) return
      await writeTextFile(dest, formatReport(exportItems, exportEnded, new Date()))
      notify.success(`Exported ${exportItems.length + exportEnded.length} problem(s) to ${dest}.`)
    } catch (e) {
      onError(e)
    }
  }

  const hideButton = (key: string) => {
    const hidden = ignored.includes(key)
    return (
      <button type="button" className="ml-auto inline-flex shrink-0 cursor-pointer items-center gap-1 text-xs text-muted-foreground hover:text-foreground" onClick={() => setHidden(key, !hidden)}>
        {hidden ? <Eye className="size-3" /> : <EyeOff className="size-3" />} {hidden ? 'Unhide' : 'Hide'}
      </button>
    )
  }

  const finding = (x: { tool: string; f: AuditFinding; key: string }, project: string) => (
    <li key={x.key} className={ignored.includes(x.key) ? 'opacity-60' : ''}>
      <details className="group rounded-lg border border-border">
        <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm [&::-webkit-details-marker]:hidden">
          <input
            type="checkbox"
            className="size-3.5 shrink-0 cursor-pointer accent-primary"
            checked={isSelected(x.key)}
            title="Select this issue"
            aria-label={`Select ${x.f.package} - ${x.f.title}`}
            onChange={() => toggleItem(x.key)}
            onClick={(e) => e.stopPropagation()}
          />
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
          <Badge variant={SEVERITY_VARIANT[x.f.severity] ?? 'outline'}>{x.f.severity}</Badge>
          <span className="inline-flex max-w-[10rem] shrink-0 items-center gap-1 truncate rounded bg-muted px-1.5 py-0.5 text-xs" title={`Project: ${project}`}>
            <FolderOpen className="size-3 shrink-0" />
            <span className="truncate">{project}</span>
          </span>
          <span className="shrink-0 font-mono text-xs">{x.f.package}</span>
          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={x.f.title}>
            {x.f.title}
          </span>
        </summary>
        <div className="flex flex-col gap-1.5 border-t border-border px-3 py-2">
          <p className="text-sm font-medium">{x.f.title}</p>
          <p className="text-xs text-muted-foreground">
            Project: <span className="text-foreground">{project}</span> · {x.tool} · <span className="font-mono">{x.f.package}</span>
          </p>
          {x.f.affected && <p className="break-words text-xs text-muted-foreground">Affected: {x.f.affected}</p>}
          <p className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
            {x.f.fix ? <span>Fix: {x.f.fix}</span> : <span>No fixed version listed.</span>}
            {x.f.url && (
              <button type="button" className="cursor-pointer text-primary underline" onClick={() => void runCommand({ type: 'open_url', url: x.f.url ?? '' }).catch(onError)}>
                {x.f.advisory ?? 'Details'}
              </button>
            )}
            <button
              type="button"
              className="inline-flex cursor-pointer items-center gap-1 hover:text-foreground"
              onClick={() => void copyText(formatFinding({ project, tool: x.tool, f: x.f }), 'this problem')}
            >
              <Copy className="size-3" /> Copy
            </button>
            {hideButton(x.key)}
          </p>
        </div>
      </details>
    </li>
  )

  const sevCounts = (items: { f: AuditFinding }[]) =>
    SEVERITY_ORDER.map((sev) => [sev, items.filter((i) => i.f.severity === sev).length] as const).filter(([, n]) => n > 0)

  return createPortal(
    <Dialog
      open={open}
      onClose={onClose}
      size="wide"
      title="Security problems"
      description="What the last checks found. Press Check all projects to look again."
      footer={
        <div className="flex w-full items-center justify-between gap-2">
          <span className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              className="cursor-pointer"
              disabled={exportItems.length + exportEnded.length === 0}
              onClick={() => void copyText(formatReport(exportItems, exportEnded, new Date()), 'all problems')}
            >
              <Copy /> Copy selected
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="cursor-pointer"
              disabled={exportItems.length + exportEnded.length === 0}
              onClick={() => void exportAll()}
            >
              <Download /> Export selected
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="cursor-pointer"
              disabled={selectedToHide.length === 0}
              onClick={handleHideSelected}
            >
              <EyeOff /> Hide selected
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="cursor-pointer"
              disabled={allVisibleKeys.length === 0}
              onClick={toggleSelectAll}
            >
              {allSelected ? 'Select none' : 'Select all'}
            </Button>
            {hiddenTotal > 0 && (
              <Button size="sm" variant="ghost" className="cursor-pointer" onClick={() => setShowHidden((v) => !v)}>
                {showHidden ? 'Hide the hidden ones' : `Show ${hiddenTotal} hidden`}
              </Button>
            )}
          </span>
          <Button size="sm" className="cursor-pointer" onClick={onClose}>
            Close
          </Button>
        </div>
      }
    >
      <div className="flex flex-col gap-5">
        {visibleEnded.length > 0 && (() => {
          const isVersionsChecked = versionKeys.length > 0 && versionKeys.every((k) => isSelected(k))
          const isVersionsIndeterminate = versionKeys.some((k) => isSelected(k)) && !isVersionsChecked
          return (
            <details className="group flex flex-col gap-2">
              <summary className="flex cursor-pointer list-none items-center gap-2 text-sm font-medium [&::-webkit-details-marker]:hidden">
                <input
                  type="checkbox"
                  ref={(el) => {
                    if (el) el.indeterminate = isVersionsIndeterminate
                  }}
                  className="size-3.5 shrink-0 cursor-pointer accent-primary"
                  checked={isVersionsChecked}
                  title="Select all versions"
                  aria-label="Select all versions"
                  onChange={() => toggleGroup(versionKeys)}
                  onClick={(e) => e.stopPropagation()}
                />
                <ChevronRight className="size-4 text-muted-foreground transition-transform group-open:rotate-90" />
                Versions that no longer get security fixes
                <span className="text-xs font-normal text-muted-foreground">{visibleEnded.length}</span>
              </summary>
              <ul className="mt-2 flex flex-col gap-2">
                {visibleEnded.map((i) => {
                  const key = eolKey(i.runtime, i.version)
                  const label = eolLabel(i.status)
                  return (
                    <li key={key} className={`flex flex-col gap-1 rounded-lg border border-border px-3 py-2 ${ignored.includes(key) ? 'opacity-60' : ''}`}>
                      <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                        <input
                          type="checkbox"
                          className="size-3.5 shrink-0 cursor-pointer accent-primary"
                          checked={isSelected(key)}
                          title="Select this version"
                          aria-label={`Select ${i.runtime} ${i.version}`}
                          onChange={() => toggleItem(key)}
                          onClick={(e) => e.stopPropagation()}
                        />
                        <ShieldAlert className="size-4" /> {i.runtime} {i.version}
                        {label && <Badge variant={label.variant}>{label.text}</Badge>}
                        {hideButton(key)}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {i.used_by.length > 0 ? `Used by: ${i.used_by.join(', ')}.` : 'Installed on this computer.'} Install a newer version on the Runtimes page.
                      </p>
                    </li>
                  )
                })}
              </ul>
            </details>
          )
        })()}
        {groups.map((g) => {
          const groupKeys = g.items.map((x) => x.key)
          const isGroupChecked = groupKeys.length > 0 && groupKeys.every((k) => isSelected(k))
          const isGroupIndeterminate = groupKeys.some((k) => isSelected(k)) && !isGroupChecked
          return (
            <details key={g.id} className="group">
              <summary className="flex cursor-pointer list-none flex-wrap items-center gap-2 text-sm font-medium [&::-webkit-details-marker]:hidden">
                <input
                  type="checkbox"
                  ref={(el) => {
                    if (el) el.indeterminate = isGroupIndeterminate
                  }}
                  className="size-3.5 shrink-0 cursor-pointer accent-primary"
                  checked={isGroupChecked}
                  title={`Select all in ${g.name}`}
                  aria-label={`Select all in ${g.name}`}
                  onChange={() => toggleGroup(groupKeys)}
                  onClick={(e) => e.stopPropagation()}
                />
                <ChevronRight className="size-4 text-muted-foreground transition-transform group-open:rotate-90" />
                <FolderOpen className="size-4 text-muted-foreground" />
                {g.name}
                <span className="text-xs font-normal text-muted-foreground">{g.items.length} finding(s)</span>
                {sevCounts(g.items).map(([sev, n]) => (
                  <Badge key={sev} variant={SEVERITY_VARIANT[sev] ?? 'outline'}>
                    {n} {sev}
                  </Badge>
                ))}
              </summary>
              <ul className="mt-2 flex flex-col gap-2">{g.items.map((x) => finding(x, g.name))}</ul>
            </details>
          )
        })}
        {visibleEnded.length === 0 && groups.length === 0 && <p className="text-sm text-muted-foreground">Nothing to show.</p>}
      </div>
    </Dialog>,
    document.body,
  )
}
