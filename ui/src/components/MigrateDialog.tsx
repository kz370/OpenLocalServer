import { AlertTriangle, Check, CheckCircle2, ChevronRight, Copy, Database, Download, FolderOpen, RotateCw, Search, XCircle } from 'lucide-react'
import { useEffect, useState } from 'react'
import { open as openFolder, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'

import { Spinner } from '@/components/Spinner'
import { TechTile } from '@/components/TechIcon'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Tabs } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { SiteImportPanel } from '@/components/SiteImportPanel'
import { type Diagnostic, type MigratedDb, type MigrationProgress, type MigrationSource, runCommand } from '@/core'
import { formatBytes, usePoll } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'
import { notify } from '@/lib/notify'
import { cn } from '@/lib/utils'

interface SourceImport {
  password: string
  databases: string[] | null
  picked: string[]
  /** Search text narrowing the database list (view only; never changes what is picked). */
  filter: string
  results: MigratedDb[] | null
  error: string | null
}

/**
 * Import databases from Laragon, XAMPP, WampServer or an older OLS MariaDB series:
 * no .sql export needed, and the original files are only ever read.
 */
export function MigrateDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [tab, setTab] = useState<'sites' | 'databases'>('databases')
  const [sources, setSources] = useState<MigrationSource[] | null>(null)
  const [selectedSourceIds, setSelectedSourceIds] = useState<string[]>([])
  const [sourceImports, setSourceImports] = useState<Record<string, SourceImport>>({})
  const [conflictPolicy, setConflictPolicy] = useState<ConflictPolicy>('skip')
  /** Scanned sources whose database list the user folded away. */
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [busy, setBusy] = useState<'scan' | 'import' | null>(null)
  const [activeSourceId, setActiveSourceId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [progress, setProgress] = useState<MigrationProgress | null>(null)
  /** Snapshot of the last finished import run, shown as the Import report section. */
  const [report, setReport] = useState<ImportReport | null>(null)

  // The scan / import command blocks until it is done, so ask where it is up to meanwhile.
  usePoll(async () => {
    if (busy === null) return
    const r = await runCommand({ type: 'get_migration_progress' })
    if (r.type === 'migration_progress' && r.progress.running) setProgress(r.progress)
  }, 500)

  useEffect(() => {
    if (!open) return
    void runCommand({ type: 'list_migration_sources' }).then((r) => {
      if (r.type !== 'migration_sources') return
      setSources(r.sources)
    })
  }, [open])

  const emptyImportState = (): SourceImport => ({
    password: '', databases: null, picked: [], filter: '', results: null, error: null,
  })

  const importState = (id: string): SourceImport => sourceImports[id] ?? emptyImportState()

  const updateImport = (id: string, patch: Partial<SourceImport>) =>
    setSourceImports((previous) => ({
      ...previous,
      [id]: { ...(previous[id] ?? emptyImportState()), ...patch },
    }))

  const toggleDatabase = (id: string, database: string, checked: boolean) =>
    setSourceImports((previous) => {
      const current = previous[id] ?? emptyImportState()
      const picked = checked
        ? current.picked.includes(database) ? current.picked : [...current.picked, database]
        : current.picked.filter((name) => name !== database)
      return { ...previous, [id]: { ...current, picked } }
    })

  const browse = async () => {
    try {
      const selected = await openFolder({ directory: true, multiple: false, title: 'Select MySQL/MariaDB data folder' })
      if (typeof selected !== 'string') return
      setError(null)
      setNotice(null)
      const r = await runCommand({ type: 'inspect_migration_source', path: selected })
      if (r.type !== 'migration_source') return
      const key = normalizePath(r.source.data_dir)
      const existing = (sources ?? []).find((candidate) => normalizePath(candidate.data_dir) === key)
      if (existing) {
        setSelectedSourceIds((previous) => previous.includes(existing.id) ? previous : [...previous, existing.id])
        setNotice('That data folder is already listed; its existing source is selected.')
        return
      }
      setSources((previous) => [...(previous ?? []), r.source])
      setSelectedSourceIds((previous) => [...previous, r.source.id])
      setNotice('Added the selected data folder to this import session.')
    } catch (e) {
      const d = e as Diagnostic
      setError(d.cause || d.problem)
    }
  }

  async function attempt(kind: 'scan' | 'import', fn: () => Promise<void>) {
    setBusy(kind)
    setProgress(null)
    setError(null)
    try {
      await fn()
    } catch (e) {
      const d = e as Diagnostic
      setError(d.cause || d.problem)
    } finally {
      setBusy(null)
      setProgress(null)
    }
  }

  const scan = (sourceId: string) =>
    attempt('scan', async () => {
      setActiveSourceId(sourceId)
      updateImport(sourceId, { databases: null, results: null, error: null })
      const r = await runCommand({ type: 'list_foreign_databases', source_id: sourceId, password: importState(sourceId).password })
      if (r.type === 'names') {
        updateImport(sourceId, { databases: r.names, picked: r.names, filter: '' })
        setCollapsed((previous) => previous.filter((id) => id !== sourceId))
      }
    })

  const selectedCount = selectedSourceIds.reduce((sum, id) => sum + importState(id).picked.length, 0)
  const importableSourceIds = selectedSourceIds.filter((id) => importState(id).databases && importState(id).picked.length > 0)
  const allScanned = selectedSourceIds.length > 0 && selectedSourceIds.every((id) => importState(id).databases !== null)
  const readyToImport = allScanned && importableSourceIds.length > 0
  const seenNames = new Set<string>()
  const duplicateNames = new Set<string>()
  for (const id of selectedSourceIds) {
    for (const name of importState(id).picked) {
      const key = name.toLocaleLowerCase()
      if (seenNames.has(key)) duplicateNames.add(name)
      seenNames.add(key)
    }
  }

  const migrate = () =>
    attempt('import', async () => {
      if (conflictPolicy === 'replace_with_backup') {
        const ok = await confirmAction(
          'For every name conflict, OLS first saves the existing database as a SQL backup. It replaces the database only after that backup succeeds. If import fails, OLS attempts to restore the backup.',
          'Replace conflicting databases',
          'Replace with backup',
        )
        if (!ok) return
      }
      setReport(null)
      const finished: MigratedDb[] = []
      const reportSources: ImportReport['sources'] = []
      let failedSources = 0
      for (const sourceId of importableSourceIds) {
        const source = sources?.find((item) => item.id === sourceId)
        const entry: ImportReport['sources'][number] = {
          label: source?.label ?? sourceId,
          dataDir: source?.data_dir ?? '',
          results: [],
          error: null,
        }
        reportSources.push(entry)
        setActiveSourceId(sourceId)
        updateImport(sourceId, { results: null, error: null })
        try {
          const state = importState(sourceId)
          const r = await runCommand({
            type: 'migrate_databases_resolved',
            source_id: sourceId,
            password: state.password,
            databases: state.picked,
            target: 'mariadb',
            conflict_policy: conflictPolicy,
          })
          if (r.type === 'migrated') {
            finished.push(...r.results)
            entry.results = r.results
            // Clear the selection once a source is done so the Import button resets;
            // per-database results stay visible on the tiles.
            updateImport(sourceId, { results: r.results, picked: [] })
          }
        } catch (e) {
          const d = e as Diagnostic
          failedSources += 1
          entry.error = d.cause || d.problem
          updateImport(sourceId, { error: entry.error })
        }
      }
      setReport({ finishedAt: new Date(), policy: conflictPolicy, sources: reportSources })
      const done = tally(finished)
      const summary = `${done.imported} imported · ${done.skipped} skipped · ${done.failed} failed`
      if (failedSources > 0) {
        notify.error(`Import finished with errors: ${summary}; ${failedSources} source${failedSources === 1 ? '' : 's'} could not be imported. See the source rows for details.`)
      } else if (done.failed > 0) {
        notify.warning(`Import finished with failures: ${summary}. Hover a failed database for details.`)
      } else {
        notify.success(`Databases imported into MariaDB: ${summary}.`)
      }
    })

  const selectedSources = selectedSourceIds.flatMap((id) => {
    const source = sources?.find((item) => item.id === id)
    return source ? [source] : []
  })
  const allResults = selectedSourceIds.flatMap((id) => importState(id).results ?? [])
  const toggleSource = (id: string, selected: boolean) => {
    setSelectedSourceIds((previous) => selected
      ? previous.includes(id) ? previous : [...previous, id]
      : previous.filter((existing) => existing !== id))
    setNotice(null)
  }

  const toggleCollapsed = (id: string) =>
    setCollapsed((previous) => previous.includes(id) ? previous.filter((existing) => existing !== id) : [...previous, id])

  const unscannedCount = selectedSourceIds.filter((id) => importState(id).databases === null).length
  const tally = (results: MigratedDb[]) => ({
    imported: results.filter((result) => result.ok).length,
    skipped: results.filter((result) => !result.ok && result.detail.startsWith('Skipped')).length,
    failed: results.filter((result) => !result.ok && !result.detail.startsWith('Skipped')).length,
  })
  const totals = tally(allResults)
  const footerStatus = allResults.length > 0 && busy === null && selectedCount === 0
    ?`${totals.imported} imported · ${totals.skipped} skipped · ${totals.failed} failed`
    : selectedSourceIds.length === 0
      ? 'Select a source to start.'
      : unscannedCount > 0
        ? `Scan ${unscannedCount} more source${unscannedCount === 1 ? '' : 's'} to continue.`
        : selectedCount === 0
          ? 'Select at least one database.'
          : `${selectedCount} database${selectedCount === 1 ? '' : 's'} from ${importableSourceIds.length} source${importableSourceIds.length === 1 ? '' : 's'}`

  return (
    <Dialog
      open={open}
      onClose={() => busy === null && onClose()}
      title={tab === 'sites' ? 'Import sites and databases' : 'Import databases'}
      description={tab === 'sites' ? 'Bring over the sites you had in Laragon, XAMPP or WampServer. Their files and settings are never changed.' : 'Copy databases from another local MySQL/MariaDB data folder. Source files are never changed.'}
      size="wide"
      footer={
        tab === 'sites' ? (
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
        ) : (
        <>
          <span className="mr-auto truncate text-xs text-muted-foreground">{footerStatus}</span>
          <Button variant="ghost" disabled={busy !== null} onClick={onClose}>
            Close
          </Button>
          <Button disabled={busy !== null || !readyToImport} onClick={migrate}>
            {busy === 'import' ? <Spinner /> : <Database />} Import {selectedCount || ''} into MariaDB
          </Button>
        </>
        )
      }
    >
      <div className="flex flex-col gap-4">
        <Tabs
          tabs={[
            { id: 'sites' as const, label: 'Sites' },
            { id: 'databases' as const, label: 'Databases' },
          ]}
          value={tab}
          onChange={setTab}
        />
        {tab === 'sites' ? (
          <SiteImportPanel />
        ) : (
          <>
        {/* Step 1: pick sources (click order = import order). */}
        <section className="flex flex-col gap-2">
          <StepHeader
            step={1}
            title="Sources"
            hint="Click in the order to import"
            action={
              <Button
                size="sm"
                variant="ghost"
                disabled={sources === null || busy !== null}
                onClick={() => void browse()}
                title="Choose the folder that contains the MySQL system database"
              >
                <FolderOpen /> Browse…
              </Button>
            }
          />
          {sources === null && (
            <p className="flex items-center gap-2 text-xs text-muted-foreground">
              <Spinner /> Looking for MySQL and MariaDB data folders…
            </p>
          )}
          {sources?.length === 0 && <p className="text-xs text-muted-foreground">No MySQL or MariaDB data folders found. Use Browse to pick one.</p>}
          {!!sources?.length && (
            <div className="grid gap-1.5 sm:grid-cols-2">
              {sources.map((s) => {
                const order = selectedSourceIds.indexOf(s.id) + 1
                const isSelected = order > 0
                return (
                  <button
                    key={s.id}
                    type="button"
                    role="checkbox"
                    aria-checked={isSelected}
                    aria-label={isSelected ? `${s.label}, import order ${order}` : s.label}
                    disabled={busy !== null}
                    onClick={() => toggleSource(s.id, !isSelected)}
                    title={s.data_dir}
                    className={cn(
                      'group flex min-w-0 cursor-pointer items-center gap-2.5 rounded-md border px-2.5 py-2 text-left transition-colors',
                      'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-60',
                      isSelected
                        ? 'border-primary/60 bg-primary/10 hover:bg-primary/15'
                        : 'border-border hover:border-muted-foreground/40 hover:bg-muted/30',
                    )}
                  >
                    <OrderDot order={isSelected ? order : null} />
                    <TechTile id={s.engine} className="size-7 shrink-0" />
                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 items-center gap-1.5">
                        <span className="truncate text-sm font-medium">{s.label}</span>
                        {s.running_port && (
                          <span className="flex shrink-0 items-center gap-1 text-[11px] text-success">
                            <span className="size-1.5 rounded-full bg-success" /> :{s.running_port}
                          </span>
                        )}
                      </div>
                      <div className="truncate text-[11px] text-muted-foreground">{s.data_dir}</div>
                    </div>
                    <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">{formatBytes(s.size_bytes)}</span>
                  </button>
                )
              })}
            </div>
          )}
        </section>

        {/* Step 2: scan each selected source and pick its databases. */}
        <section className="flex flex-col gap-2">
          <StepHeader step={2} title="Databases" hint={selectedSources.length ? `${selectedCount} selected` : undefined} />
          {selectedSources.length === 0 ? (
            <p className="rounded-md border border-dashed border-border px-3 py-3 text-center text-xs text-muted-foreground">
              Select a source above to scan its databases.
            </p>
          ) : (
            <div className="flex flex-col gap-1.5">
              {selectedSources.map((source, index) => {
                const state = importState(source.id)
                const scanning = busy === 'scan' && activeSourceId === source.id
                const working = busy !== null && activeSourceId === source.id
                const expanded = state.databases !== null && !collapsed.includes(source.id)
                const sourceTally = state.results ? tally(state.results) : null
                const notes = (state.results ?? []).filter((result) => !result.ok || result.detail !== 'copied')
                return (
                  <div key={source.id} className="rounded-md border border-border">
                    <div className="flex flex-wrap items-center gap-2 px-2.5 py-1.5">
                      <button
                        type="button"
                        disabled={state.databases === null}
                        onClick={() => toggleCollapsed(source.id)}
                        aria-expanded={expanded}
                        className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left disabled:cursor-default"
                      >
                        <ChevronRight className={cn('size-4 shrink-0 text-muted-foreground transition-transform', expanded && 'rotate-90', state.databases === null && 'opacity-30')} />
                        <OrderDot order={index + 1} />
                        <span className="truncate text-sm font-medium">{source.label}</span>
                        <span
                          className={cn(
                            'shrink-0 rounded-full px-2 py-0.5 text-[11px] tabular-nums',
                            working ? 'bg-primary/10 text-primary' : state.databases ? 'bg-muted/60 text-muted-foreground' : 'text-muted-foreground/70',
                          )}
                        >
                          {working
                            ? scanning ? 'Scanning…' : 'Importing…'
                            : sourceTally
                              ? <><span className="text-success">{sourceTally.imported} imported</span> · {sourceTally.skipped} skipped{sourceTally.failed > 0 && <span className="text-destructive"> · {sourceTally.failed} failed</span>}</>
                              : state.databases
                                ? `${state.picked.length} of ${state.databases.length} selected`
                                : 'Not scanned'}
                        </span>
                      </button>
                      <div className="flex shrink-0 items-center gap-1.5">
                        <Input
                          type="password"
                          value={state.password}
                          disabled={busy !== null}
                          onChange={(e) => updateImport(source.id, { password: e.target.value })}
                          placeholder="Root password"
                          aria-label={`Root password for ${source.label} (leave blank if none)`}
                          title="Leave blank if root has no password"
                          className="h-8 w-36 text-xs"
                          autoComplete="off"
                        />
                        <Button size="sm" variant={state.databases ? 'ghost' : 'secondary'} disabled={busy !== null} onClick={() => scan(source.id)}>
                          {scanning ? <Spinner /> : state.databases ? <RotateCw /> : <Search />}
                          {state.databases ? 'Rescan' : 'Scan'}
                        </Button>
                      </div>
                    </div>
                    {working && <ProgressPanel progress={progress} />}
                    {state.error && <p className="border-t border-border px-3 py-1.5 text-xs text-destructive">{state.error}</p>}
                    {expanded && state.databases && (
                      <div className="flex flex-col gap-2 border-t border-border px-2.5 py-2">
                        <DatabasePicker
                          databases={state.databases}
                          picked={state.picked}
                          filter={state.filter}
                          disabled={busy !== null}
                          duplicateNames={duplicateNames}
                          resultFor={(db) => state.results?.find((r) => r.name === db) ?? (activeSourceId === source.id ? progress?.done.find((r) => r.name === db) : undefined)}
                          onFilter={(filter) => updateImport(source.id, { filter })}
                          onToggle={(db, checked) => toggleDatabase(source.id, db, checked)}
                          onSetPicked={(picked) => updateImport(source.id, { picked })}
                        />
                        {notes.length > 0 && (
                          <ul className="flex max-h-24 flex-col gap-0.5 overflow-y-auto text-[11px]">
                            {notes.map((result) => (
                              <li
                                key={`${source.id}:${result.name}`}
                                className={result.ok || result.detail.startsWith('Skipped') ? 'text-muted-foreground' : 'text-destructive'}
                              >
                                <span className="font-mono">{result.name}</span>: {result.detail}
                              </li>
                            ))}
                          </ul>
                        )}
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}
          {duplicateNames.size > 0 && (
            <p className="flex items-start gap-1.5 text-xs text-warning">
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              <span>Picked in more than one source: {[...duplicateNames].join(', ')}. Resolved in source order.</span>
            </p>
          )}
        </section>

        {/* Step 3: what to do when a name already exists. */}
        <section className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <StepHeader step={3} title="If a database already exists" />
            <div role="radiogroup" aria-label="If a database already exists" className="inline-flex flex-wrap rounded-md border border-border bg-muted/30 p-0.5">
              {CONFLICT_POLICIES.map((policy) => {
                const active = conflictPolicy === policy.id
                return (
                  <button
                    key={policy.id}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    disabled={busy !== null}
                    onClick={() => setConflictPolicy(policy.id)}
                    className={cn(
                      'cursor-pointer rounded px-3 py-1 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60',
                      active ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
                    )}
                  >
                    {policy.label}
                  </button>
                )
              })}
            </div>
          </div>
          <p className="-mt-1 text-[11px] text-muted-foreground">{CONFLICT_POLICIES.find((policy) => policy.id === conflictPolicy)?.hint}</p>
        </section>

        {report && busy === null && <ImportReportPanel report={report} />}

        {busy !== null && !selectedSourceIds.includes(activeSourceId ?? '') && (
          <div className="rounded-md border border-border">
            <ProgressPanel progress={progress} />
          </div>
        )}

        {error && <p className="text-sm text-destructive">{error}</p>}
        {notice && <p className="text-xs text-muted-foreground">{notice}</p>}
          </>
        )}
      </div>
    </Dialog>
  )
}

type ConflictPolicy = 'skip' | 'replace_with_backup' | 'rename_incoming'

/** Result of one finished import run. Never holds passwords. */
interface ImportReport {
  finishedAt: Date
  policy: ConflictPolicy
  sources: { label: string; dataDir: string; results: MigratedDb[]; error: string | null }[]
}

type ResultStatus = 'Imported' | 'Skipped' | 'Failed'

function resultStatus(result: MigratedDb): ResultStatus {
  if (result.ok) return 'Imported'
  return result.detail.startsWith('Skipped') ? 'Skipped' : 'Failed'
}

/** Plain-text version of the report for Copy / Save. */
function reportToText(report: ImportReport) {
  const policy = CONFLICT_POLICIES.find((item) => item.id === report.policy)?.label ?? report.policy
  const all = report.sources.flatMap((source) => source.results)
  const count = (status: ResultStatus) => all.filter((result) => resultStatus(result) === status).length
  const lines = [
    'OLS database import report',
    `Finished: ${report.finishedAt.toLocaleString()}`,
    'Target: MariaDB',
    `If a database already exists: ${policy}`,
    `Totals: ${count('Imported')} imported, ${count('Skipped')} skipped, ${count('Failed')} failed`,
  ]
  for (const source of report.sources) {
    lines.push('', `Source: ${source.label}${source.dataDir ? ` (${source.dataDir})` : ''}`)
    if (source.error) lines.push(`  ERROR: ${source.error}`)
    for (const result of source.results) lines.push(`  [${resultStatus(result)}] ${result.name}: ${result.detail}`)
    if (!source.error && source.results.length === 0) lines.push('  No databases processed.')
  }
  return `${lines.join('\n')}\n`
}

/** Step 4: per-source, per-database outcome of the last import, with Copy and Save. */
function ImportReportPanel({ report }: { report: ImportReport }) {
  const [saving, setSaving] = useState(false)
  const all = report.sources.flatMap((source) => source.results)
  const count = (status: ResultStatus) => all.filter((result) => resultStatus(result) === status).length
  const failedSources = report.sources.filter((source) => source.error).length

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(reportToText(report))
      notify.success('Import report copied to the clipboard.')
    } catch (e) {
      notify.error('Could not copy the import report.', { details: `Clipboard access failed: ${String(e)}. Use Save report… instead.` })
    }
  }

  const saveToFile = async () => {
    setSaving(true)
    try {
      const stamp = report.finishedAt.toISOString().slice(0, 19).replaceAll(':', '-')
      const path = await saveDialog({
        title: 'Save import report',
        defaultPath: `ols-import-report-${stamp}.txt`,
        filters: [{ name: 'Text', extensions: ['txt'] }],
      })
      if (!path) return
      await writeTextFile(path, reportToText(report))
      notify.success(`Import report saved to ${path}.`)
    } catch (e) {
      notify.error('Could not save the import report.', { details: `Writing the file failed: ${String(e)}. Pick a folder you can write to and try again.` })
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="flex flex-col gap-2" aria-label="Import report">
      <StepHeader
        step={4}
        title="Import report"
        hint={report.finishedAt.toLocaleTimeString()}
        action={
          <div className="flex items-center gap-1">
            <Button size="sm" variant="ghost" onClick={() => void copy()}>
              <Copy /> Copy
            </Button>
            <Button size="sm" variant="ghost" disabled={saving} onClick={() => void saveToFile()}>
              {saving ? <Spinner /> : <Download />} Save report…
            </Button>
          </div>
        }
      />
      <div className="flex flex-wrap gap-2 text-xs">
        <span className="rounded-full bg-success/10 px-2 py-0.5 text-success">{count('Imported')} imported</span>
        <span className="rounded-full bg-muted/60 px-2 py-0.5 text-muted-foreground">{count('Skipped')} skipped</span>
        <span className={cn('rounded-full px-2 py-0.5', count('Failed') > 0 ? 'bg-destructive/10 text-destructive' : 'bg-muted/60 text-muted-foreground')}>
          {count('Failed')} failed
        </span>
        {failedSources > 0 && (
          <span className="rounded-full bg-destructive/10 px-2 py-0.5 text-destructive">
            {failedSources} source{failedSources === 1 ? '' : 's'} failed
          </span>
        )}
      </div>
      <div className="max-h-56 overflow-y-auto rounded-md border border-border">
        {report.sources.map((source) => (
          <div key={`${source.label}:${source.dataDir}`} className="border-b border-border last:border-b-0">
            <div className="truncate bg-muted/30 px-2.5 py-1 text-xs font-medium" title={source.dataDir}>
              {source.label}
            </div>
            {source.error && <p className="px-2.5 py-1 text-xs text-destructive">{source.error}</p>}
            {source.results.length > 0 && (
              <table className="w-full text-xs">
                <tbody>
                  {source.results.map((result) => {
                    const status = resultStatus(result)
                    return (
                      <tr key={result.name} className="border-t border-border/50">
                        <td className="w-1/3 truncate px-2.5 py-1 font-mono">{result.name}</td>
                        <td
                          className={cn(
                            'w-20 px-2 py-1 font-medium',
                            status === 'Imported' ? 'text-success' : status === 'Failed' ? 'text-destructive' : 'text-muted-foreground',
                          )}
                        >
                          {status}
                        </td>
                        <td className="break-words px-2 py-1 text-muted-foreground">{result.detail}</td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
            {!source.error && source.results.length === 0 && (
              <p className="px-2.5 py-1 text-xs text-muted-foreground">No databases processed.</p>
            )}
          </div>
        ))}
      </div>
    </section>
  )
}

const CONFLICT_POLICIES: { id: ConflictPolicy; label: string; hint: string }[] = [
  { id: 'skip', label: 'Skip (default)', hint: 'Keeps the database already in OLS and skips the incoming one.' },
  { id: 'replace_with_backup', label: 'Replace (backup first)', hint: 'Saves a SQL backup first; stops if the backup fails and restores it if the import fails.' },
  { id: 'rename_incoming', label: 'Rename incoming', hint: 'Imports the incoming database under a unique _imported name; existing data stays unchanged.' },
]

/** Small numbered step title with an optional hint and right-aligned action. */
function StepHeader({ step, title, hint, action }: { step: number; title: string; hint?: string; action?: React.ReactNode }) {
  return (
    <div className="flex min-h-8 items-center gap-2">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
        {step} · {title}
      </span>
      {hint && <span className="text-[11px] text-muted-foreground/70">{hint}</span>}
      {action && <div className="ml-auto">{action}</div>}
    </div>
  )
}

/** Fixed-size circle: empty when not selected, the import order number when selected. Never changes size. */
function OrderDot({ order }: { order: number | null }) {
  return (
    <span
      aria-hidden
      className={cn(
        'inline-flex size-5 shrink-0 items-center justify-center rounded-full border-2 text-[11px] font-semibold tabular-nums transition-colors',
        order !== null ? 'border-primary bg-primary text-primary-foreground' : 'border-muted-foreground/40 group-hover:border-muted-foreground',
      )}
    >
      {order ?? ''}
    </span>
  )
}

/**
 * Selectable database tiles: aligned checkbox + name, per-database import status,
 * a warning mark for names also picked in another source, plus search and bulk select.
 */
function DatabasePicker({
  databases,
  picked,
  filter,
  disabled,
  duplicateNames,
  resultFor,
  onFilter,
  onToggle,
  onSetPicked,
}: {
  databases: string[]
  picked: string[]
  filter: string
  disabled: boolean
  duplicateNames: Set<string>
  resultFor: (db: string) => MigratedDb | undefined
  onFilter: (filter: string) => void
  onToggle: (db: string, checked: boolean) => void
  onSetPicked: (picked: string[]) => void
}) {
  const query = filter.trim().toLocaleLowerCase()
  const visible = query ? databases.filter((db) => db.toLocaleLowerCase().includes(query)) : databases
  const pickedSet = new Set(picked)
  const duplicateKeys = new Set([...duplicateNames].map((name) => name.toLocaleLowerCase()))
  const allVisiblePicked = visible.length > 0 && visible.every((db) => pickedSet.has(db))
  const noneVisiblePicked = visible.every((db) => !pickedSet.has(db))
  // Bulk actions only touch what the search shows, so hidden picks are kept.
  const selectVisible = () => onSetPicked([...new Set([...picked, ...visible])])
  const clearVisible = () => {
    const shown = new Set(visible)
    onSetPicked(picked.filter((db) => !shown.has(db)))
  }

  if (databases.length === 0) return <p className="text-xs text-muted-foreground">No user databases found.</p>

  return (
    <>
      {databases.length > 1 && <div className="flex items-center gap-1">
        {databases.length > 6 && (
          <div className="relative min-w-0 flex-1 sm:max-w-56">
            <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={filter}
              onChange={(e) => onFilter(e.target.value)}
              placeholder="Filter"
              aria-label="Filter databases"
              className="h-7 w-full pl-7 text-xs"
              autoComplete="off"
            />
          </div>
        )}
        <div className="ml-auto flex items-center text-xs">
          <button
            type="button"
            disabled={disabled || allVisiblePicked}
            onClick={selectVisible}
            className="cursor-pointer rounded px-2 py-1 font-medium text-primary hover:bg-primary/10 disabled:cursor-default disabled:text-muted-foreground/50 disabled:hover:bg-transparent"
          >
            All
          </button>
          <span className="text-muted-foreground/40">|</span>
          <button
            type="button"
            disabled={disabled || noneVisiblePicked}
            onClick={clearVisible}
            className="cursor-pointer rounded px-2 py-1 font-medium text-primary hover:bg-primary/10 disabled:cursor-default disabled:text-muted-foreground/50 disabled:hover:bg-transparent"
          >
            None
          </button>
        </div>
      </div>}

      {visible.length === 0 ? (
        <p className="py-2 text-center text-xs text-muted-foreground">No databases match “{filter.trim()}”.</p>
      ) : (
        <div className="grid max-h-48 grid-cols-2 gap-1 overflow-y-auto pr-1 sm:grid-cols-3 lg:grid-cols-4">
          {visible.map((db) => {
            const isPicked = pickedSet.has(db)
            const result = resultFor(db)
            const isDuplicate = isPicked && duplicateKeys.has(db.toLocaleLowerCase())
            const skipped = result && !result.ok && result.detail.startsWith('Skipped')
            return (
              <button
                key={db}
                type="button"
                role="checkbox"
                aria-checked={isPicked}
                disabled={disabled}
                title={result?.detail ?? (isDuplicate ? 'Also selected in another source; resolved in source order.' : db)}
                onClick={() => onToggle(db, !isPicked)}
                className={cn(
                  'group flex min-w-0 cursor-pointer items-center gap-2 rounded border px-2 py-1 text-left text-xs transition-colors',
                  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-60',
                  isPicked
                    ? 'border-primary/50 bg-primary/10 text-foreground hover:bg-primary/15'
                    : 'border-border bg-transparent text-muted-foreground hover:border-muted-foreground/40 hover:text-foreground',
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    'inline-flex size-3.5 shrink-0 items-center justify-center rounded-sm border transition-colors',
                    isPicked ? 'border-primary bg-primary text-primary-foreground' : 'border-muted-foreground/40 bg-input/60 group-hover:border-muted-foreground',
                  )}
                >
                  {isPicked && <Check className="size-2.5" strokeWidth={3.5} />}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono">{db}</span>
                {isDuplicate && !result && <AlertTriangle className="size-3.5 shrink-0 text-warning" />}
                {result && (result.ok
                  ? <CheckCircle2 className="size-3.5 shrink-0 text-success" />
                  : skipped
                    ? <span className="shrink-0 text-[10px] uppercase tracking-wide text-muted-foreground">skip</span>
                    : <XCircle className="size-3.5 shrink-0 text-destructive" />)}
              </button>
            )
          })}
        </div>
      )}
    </>
  )
}

function normalizePath(path: string) {
  return path.replaceAll('\\', '/').replace(/\/+$/, '').toLocaleLowerCase()
}

/** Slim live strip for the running scan or import, shown inside the source row it belongs to. */
function ProgressPanel({ progress }: { progress: MigrationProgress | null }) {
  const [now, setNow] = useState(() => Date.now())
  usePoll(() => setNow(Date.now()), 1000)
  if (!progress) {
    return (
      <div className="flex items-center gap-2 border-t border-border px-2.5 py-2 text-xs text-muted-foreground">
        <Spinner className="size-3.5" /> Getting started…
      </div>
    )
  }
  const { bytes, bytes_total: total } = progress
  // Export sizes are an estimate, so the bar never claims to be done before the step is.
  const pct = total ? Math.min(99, Math.round((bytes / total) * 100)) : null
  const elapsed = Math.max(0, Math.round((now - progress.started_ms) / 1000))
  const failed = progress.done.filter((d) => !d.ok).length
  const size = bytes > 0
    ? total ? `${formatBytes(bytes)} / ${progress.step.startsWith('Exporting') ? '~' : ''}${formatBytes(total)}` : `${formatBytes(bytes)} written`
    : null

  return (
    <div className="flex flex-col gap-1.5 border-t border-border bg-muted/20 px-2.5 py-2" aria-live="polite">
      <div className="flex min-w-0 items-center gap-2 text-xs">
        <Spinner className="size-3.5 shrink-0 text-primary" />
        <span className="min-w-0 flex-1 truncate">
          {progress.current_db && progress.db_total > 0 && <span className="font-mono">{progress.current_db} · </span>}
          {progress.step || 'Working…'}
        </span>
        <span className="flex shrink-0 items-center gap-2 tabular-nums text-muted-foreground">
          {size && <span>{size}</span>}
          {progress.db_total > 0 && (
            <span>
              {progress.db_index}/{progress.db_total}
              {failed > 0 && <span className="text-destructive"> · {failed} failed</span>}
            </span>
          )}
          <span>{Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, '0')}</span>
        </span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-muted">
        {pct === null ? (
          <div className="h-full w-1/3 animate-[indeterminate_1.2s_ease-in-out_infinite] rounded-full bg-primary" />
        ) : (
          <div className="h-full rounded-full bg-primary transition-[width] duration-500" style={{ width: `${pct}%` }} />
        )}
      </div>
    </div>
  )
}
