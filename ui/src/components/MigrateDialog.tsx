import { AlertTriangle, Check, CheckCircle2, CheckSquare, Database, FolderOpen, Search, Square, XCircle } from 'lucide-react'
import { useEffect, useState } from 'react'
import { open as openFolder } from '@tauri-apps/plugin-dialog'

import { Spinner } from '@/components/Spinner'
import { TechTile } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type Diagnostic, type MigratedDb, type MigrationProgress, type MigrationSource, runCommand } from '@/core'
import { formatBytes, usePoll } from '@/lib/hooks'
import { confirmAction } from '@/lib/confirm'
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
  const [sources, setSources] = useState<MigrationSource[] | null>(null)
  const [selectedSourceIds, setSelectedSourceIds] = useState<string[]>([])
  const [sourceImports, setSourceImports] = useState<Record<string, SourceImport>>({})
  const [conflictPolicy, setConflictPolicy] = useState<'skip' | 'replace_with_backup' | 'rename_incoming'>('skip')
  const [busy, setBusy] = useState<'scan' | 'import' | null>(null)
  const [activeSourceId, setActiveSourceId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [progress, setProgress] = useState<MigrationProgress | null>(null)

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
      for (const sourceId of importableSourceIds) {
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
          if (r.type === 'migrated') updateImport(sourceId, { results: r.results })
        } catch (e) {
          const d = e as Diagnostic
          updateImport(sourceId, { error: d.cause || d.problem })
        }
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

  return (
    <Dialog
      open={open}
      onClose={() => busy === null && onClose()}
      title="Import databases"
      description="Copies databases from another local MySQL/MariaDB data folder. Stopped sources are read from a temporary copy; source files are never changed."
      wide
      footer={
        <>
          <Button variant="ghost" disabled={busy !== null} onClick={onClose}>
            Close
          </Button>
          {!readyToImport && (
            <span className="mr-auto text-xs text-muted-foreground">
              Scan every selected folder and select at least one database to enable import.
            </span>
          )}
          <Button disabled={busy !== null || !readyToImport} onClick={migrate}>
            {busy === 'import' ? <Spinner /> : <Database />} Import {selectedCount} database{selectedCount === 1 ? '' : 's'} into MariaDB
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {sources === null && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Looking for MySQL and MariaDB data folders…
          </p>
        )}
        {sources?.length === 0 && <p className="text-sm text-muted-foreground">No compatible MySQL or MariaDB data folders were found on this PC.</p>}

        {!!sources?.length && (
          <div className="grid gap-2 sm:grid-cols-2">
            {sources.map((s) => (
              <div key={s.id} className={`rounded-lg border p-3 ${selectedSourceIds.includes(s.id) ? 'border-primary bg-primary/5' : 'border-border'}`}>
                <div className="flex items-center gap-3">
                  <TechTile id={s.engine} className="size-9" />
                  <div className="min-w-0 flex-1">
                    <Toggle
                      checked={selectedSourceIds.includes(s.id)}
                      disabled={busy !== null}
                      onChange={(checked) => toggleSource(s.id, checked)}
                      label={s.label}
                    />
                    <span className="mt-1 block truncate text-xs text-muted-foreground" title={s.data_dir}>
                      {formatBytes(s.size_bytes)} · {s.data_dir}
                    </span>
                  </div>
                  {selectedSourceIds.includes(s.id) && <Badge variant="secondary">Order {selectedSourceIds.indexOf(s.id) + 1}</Badge>}
                  {s.running_port && <Badge variant="success">running :{s.running_port}</Badge>}
                </div>
              </div>
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button variant="secondary" disabled={sources === null || busy !== null} onClick={() => void browse()}>
            <FolderOpen /> Browse for data folder
          </Button>
          <span className="text-xs text-muted-foreground">Choose the folder containing the MySQL system database. Detected sources remain available above.</span>
        </div>

        {selectedSources.length === 0 && <p className="text-sm text-muted-foreground">Select one or more detected sources, or browse for a data folder.</p>}

        {selectedSources.map((source, index) => {
          const state = importState(source.id)
          return (
            <div key={source.id} className="flex flex-col gap-3 rounded-lg border border-border p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-sm font-medium">{index + 1}. {source.label}</span>
                <Badge variant="secondary">Import order {index + 1}</Badge>
              </div>
              <div className="flex flex-wrap items-end gap-2">
                <label className="flex min-w-0 flex-1 flex-col gap-1.5 text-xs font-medium text-muted-foreground">
                  Root password
                  <Input
                    type="password"
                    value={state.password}
                    disabled={busy !== null}
                    onChange={(e) => updateImport(source.id, { password: e.target.value })}
                    placeholder="Leave blank if none"
                    className="min-w-0 sm:max-w-72"
                    autoComplete="off"
                  />
                </label>
                <Button variant="secondary" disabled={busy !== null} onClick={() => scan(source.id)}>
                  {busy === 'scan' && activeSourceId === source.id ? <Spinner /> : <Search />}
                  {busy === 'scan' && activeSourceId === source.id ? (source.running_port ? 'Reading…' : 'Starting a copy…') : 'Find databases'}
                </Button>
              </div>
              {state.databases && (
                <div className="flex flex-col gap-2.5 border-t border-border pt-3">
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
                  {state.error && <p className="text-xs text-destructive">{state.error}</p>}
                  {state.results?.filter((result) => !result.ok).map((result) => (
                    <p key={`${source.id}:${result.name}`} className={`text-xs ${result.detail.startsWith('Skipped') ? 'text-muted-foreground' : 'text-destructive'}`}>{result.name}: {result.detail}</p>
                  ))}
                  {state.results?.filter((result) => result.ok && result.detail !== 'copied').map((result) => (
                    <p key={`${source.id}:${result.name}`} className="text-xs text-muted-foreground">{result.name}: {result.detail}</p>
                  ))}
                </div>
              )}
            </div>
          )
        })}

        {busy !== null && <ProgressPanel progress={progress} label={sources?.find((s) => s.id === activeSourceId)?.label} />}

        <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
          <label htmlFor="migration-conflict-policy" className="text-sm font-medium">Database Conflict Resolution</label>
          <select
            id="migration-conflict-policy"
            className="h-9 rounded-md border border-input bg-background px-3 text-sm"
            value={conflictPolicy}
            disabled={busy !== null}
            onChange={(e) => setConflictPolicy(e.target.value as typeof conflictPolicy)}
          >
            <option value="skip">Skip Existing (Default)</option>
            <option value="replace_with_backup">Replace Existing (Backup First)</option>
            <option value="rename_incoming">Rename Incoming</option>
          </select>
          <p className="text-xs text-muted-foreground">
            {conflictPolicy === 'skip' && 'Keeps the database already in OLS and skips incoming databases with the same name.'}
            {conflictPolicy === 'replace_with_backup' && 'Creates a safety backup first. OLS stops if the backup fails and attempts to restore it if import fails.'}
            {conflictPolicy === 'rename_incoming' && 'Gives each conflicting incoming database a unique _imported name; existing data stays unchanged.'}
          </p>
          {duplicateNames.size > 0 && (
            <p className="flex items-center gap-2 text-xs text-warning">
              <AlertTriangle className="size-3.5" /> Same-name databases appear in multiple selected folders: {[...duplicateNames].join(', ')}. They will be resolved in the displayed folder order.
            </p>
          )}
          {selectedSourceIds.length > 0 && !allScanned && <p className="text-xs text-muted-foreground">Scan every selected folder before importing.</p>}
          {allScanned && selectedCount === 0 && <p className="text-xs text-muted-foreground">Select at least one database to enable import.</p>}
          {allResults.length > 0 && (
            <p className="text-xs font-medium">
              {allResults.filter((result) => result.ok).length} imported · {allResults.filter((result) => !result.ok && result.detail.startsWith('Skipped')).length} skipped · {allResults.filter((result) => !result.ok && !result.detail.startsWith('Skipped')).length} failed
            </p>
          )}
        </div>

        {error && <p className="text-sm text-destructive">{error}</p>}
        {notice && <p className="text-sm text-muted-foreground">{notice}</p>}
      </div>
    </Dialog>
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
      <div className="flex flex-wrap items-center gap-2">
        <span className="flex items-center gap-2 text-sm font-medium">
          Databases
          <Badge variant={picked.length ? 'default' : 'secondary'} className="tabular-nums">
            {picked.length} of {databases.length}
          </Badge>
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          {databases.length > 6 && (
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={filter}
                onChange={(e) => onFilter(e.target.value)}
                placeholder="Filter databases"
                aria-label="Filter databases"
                className="h-8 w-44 pl-8 text-xs"
                autoComplete="off"
              />
            </div>
          )}
          <Button size="sm" variant="ghost" disabled={disabled || allVisiblePicked} onClick={selectVisible}>
            <CheckSquare /> All
          </Button>
          <Button size="sm" variant="ghost" disabled={disabled || noneVisiblePicked} onClick={clearVisible}>
            <Square /> None
          </Button>
        </div>
      </div>

      {visible.length === 0 ? (
        <p className="rounded-md border border-dashed border-border px-3 py-4 text-center text-xs text-muted-foreground">
          No databases match “{filter.trim()}”.
        </p>
      ) : (
        <div className="grid max-h-64 grid-cols-1 gap-1.5 overflow-y-auto pr-1 sm:grid-cols-2 lg:grid-cols-3">
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
                title={result?.detail ?? (isDuplicate ? 'Also selected in another source; resolved in folder order.' : db)}
                onClick={() => onToggle(db, !isPicked)}
                className={cn(
                  'group flex min-w-0 cursor-pointer items-center gap-2.5 rounded-md border px-2.5 py-2 text-left text-sm transition-colors',
                  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-60',
                  isPicked
                    ? 'border-primary/50 bg-primary/10 text-foreground hover:bg-primary/15'
                    : 'border-border bg-transparent text-muted-foreground hover:border-muted-foreground/40 hover:text-foreground',
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    'inline-flex size-4 shrink-0 items-center justify-center rounded border transition-colors',
                    isPicked ? 'border-primary bg-primary text-primary-foreground' : 'border-muted-foreground/40 bg-input/60 group-hover:border-muted-foreground',
                  )}
                >
                  {isPicked && <Check className="size-3" strokeWidth={3} />}
                </span>
                <Database className={cn('size-3.5 shrink-0', isPicked ? 'text-primary' : 'text-muted-foreground/60')} />
                <span className="min-w-0 flex-1 truncate font-mono text-[13px]">{db}</span>
                {isDuplicate && !result && <AlertTriangle className="size-3.5 shrink-0 text-warning" />}
                {result && (result.ok
                  ? <CheckCircle2 className="size-3.5 shrink-0 text-success" />
                  : skipped
                    ? <Badge variant="secondary" className="shrink-0">Skipped</Badge>
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

/** Live step, per-database count and a byte bar for the running scan or import. */
function ProgressPanel({ progress, label }: { progress: MigrationProgress | null; label?: string }) {
  const [now, setNow] = useState(Date.now())
  usePoll(() => setNow(Date.now()), 1000)
  if (!progress) {
    return (
      <div className="flex items-center gap-2 rounded-lg border border-border p-3 text-sm text-muted-foreground">
        <Spinner /> Getting started…
      </div>
    )
  }
  const { bytes, bytes_total: total } = progress
  // Export sizes are an estimate, so the bar never claims to be done before the step is.
  const pct = total ? Math.min(99, Math.round((bytes / total) * 100)) : null
  const overall = progress.db_total ? Math.round(((progress.db_index - 1 + (pct ?? 0) / 100) / progress.db_total) * 100) : null
  const elapsed = Math.max(0, Math.round((now - progress.started_ms) / 1000))
  const failed = progress.done.filter((d) => !d.ok).length

  return (
    <div className="flex flex-col gap-2.5 rounded-lg border border-border bg-muted/30 p-3" aria-live="polite">
      <div className="flex items-center justify-between gap-3 text-sm">
        <span className="flex min-w-0 items-center gap-2 font-medium">
          <Spinner className="shrink-0" />
          <span className="truncate">{label ? `${label} · ` : ''}{progress.step || 'Working…'}</span>
        </span>
        <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
          {Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, '0')}
        </span>
      </div>

      <div className="h-1.5 overflow-hidden rounded-full bg-muted">
        {pct === null ? (
          <div className="h-full w-1/3 animate-[indeterminate_1.2s_ease-in-out_infinite] rounded-full bg-primary" />
        ) : (
          <div className="h-full rounded-full bg-primary transition-[width] duration-500" style={{ width: `${pct}%` }} />
        )}
      </div>

      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-xs tabular-nums text-muted-foreground">
        <span>{bytes > 0 && (total ? `${formatBytes(bytes)} of ${progress.step.startsWith('Exporting') ? '~' : ''}${formatBytes(total)}` : `${formatBytes(bytes)} written`)}</span>
        {progress.db_total > 0 && (
          <span>
            Database {progress.db_index} of {progress.db_total}
            {overall !== null && ` · ${overall}% overall`}
            {failed > 0 && <span className="text-destructive"> · {failed} failed</span>}
          </span>
        )}
      </div>
    </div>
  )
}
