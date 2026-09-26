import { CheckCircle2, Database, Search, XCircle } from 'lucide-react'
import { useEffect, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { TechIcon } from '@/components/TechIcon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Field, Toggle } from '@/components/ui/form'
import { Input } from '@/components/ui/input'
import { type Diagnostic, type MigratedDb, type MigrationSource, runCommand } from '@/core'
import { formatBytes } from '@/lib/hooks'

/**
 * Import databases from Laragon, XAMPP or WampServer, straight from their data folders:
 * no .sql export needed, and the original files are only ever read.
 */
export function MigrateDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [sources, setSources] = useState<MigrationSource[] | null>(null)
  const [sourceId, setSourceId] = useState('')
  const [password, setPassword] = useState('')
  const [dbs, setDbs] = useState<string[] | null>(null)
  const [picked, setPicked] = useState<string[]>([])
  const [busy, setBusy] = useState<'scan' | 'import' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [results, setResults] = useState<MigratedDb[] | null>(null)

  useEffect(() => {
    if (!open) return
    void runCommand({ type: 'list_migration_sources' }).then((r) => {
      if (r.type !== 'migration_sources') return
      setSources(r.sources)
      const first = r.sources[0]
      if (first) {
        setSourceId(first.id)
      }
    })
  }, [open])

  const source = sources?.find((s) => s.id === sourceId)

  async function attempt(kind: 'scan' | 'import', fn: () => Promise<void>) {
    setBusy(kind)
    setError(null)
    try {
      await fn()
    } catch (e) {
      const d = e as Diagnostic
      setError(d.cause || d.problem)
    } finally {
      setBusy(null)
    }
  }

  const scan = () =>
    attempt('scan', async () => {
      setResults(null)
      const r = await runCommand({ type: 'list_foreign_databases', source_id: sourceId, password })
      if (r.type === 'names') {
        setDbs(r.names)
        setPicked(r.names)
      }
    })

  const migrate = () =>
    attempt('import', async () => {
      const r = await runCommand({ type: 'migrate_databases', source_id: sourceId, password, databases: picked, target: 'mariadb' })
      if (r.type === 'migrated') setResults(r.results)
    })

  const choose = (id: string) => {
    setSourceId(id)
    setDbs(null)
    setResults(null)
  }

  return (
    <Dialog
      open={open}
      onClose={() => busy === null && onClose()}
      title="Import from Laragon, XAMPP or Wamp"
      description="Copies databases straight from the other app's data folder. No .sql export needed, and its files are never changed."
      wide
      footer={
        <>
          <Button variant="ghost" disabled={busy !== null} onClick={onClose}>
            Close
          </Button>
          {dbs && (
            <Button disabled={busy !== null || picked.length === 0} onClick={migrate}>
              {busy === 'import' ? <Spinner /> : <Database />} Import {picked.length} database{picked.length === 1 ? '' : 's'} into MariaDB
            </Button>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {sources === null && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Looking for Laragon, XAMPP and WampServer…
          </p>
        )}
        {sources?.length === 0 && <p className="text-sm text-muted-foreground">No Laragon, XAMPP or WampServer databases were found on this PC.</p>}

        {!!sources?.length && (
          <div className="grid gap-2 sm:grid-cols-2">
            {sources.map((s) => (
              <button
                key={s.id}
                disabled={busy !== null}
                onClick={() => choose(s.id)}
                className={`flex items-center gap-3 rounded-lg border p-3 text-left transition-colors ${
                  s.id === sourceId ? 'border-primary bg-primary/5 ring-1 ring-primary' : 'border-border hover:bg-accent/50'
                }`}
              >
                <TechIcon id={s.engine} className="size-5" />
                <span className="min-w-0 flex-1">
                  <span className="block text-sm font-medium">{s.label}</span>
                  <span className="block truncate text-xs text-muted-foreground" title={s.data_dir}>
                    {formatBytes(s.size_bytes)} · {s.data_dir}
                  </span>
                </span>
                {s.running_port && <Badge variant="success">running :{s.running_port}</Badge>}
              </button>
            ))}
          </div>
        )}

        {source && (
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Its root password" hint="Blank for Laragon and XAMPP defaults.">
              <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} className="w-56" autoComplete="off" />
            </Field>
            <Button variant="secondary" disabled={busy !== null} onClick={scan}>
              {busy === 'scan' ? <Spinner /> : <Search />} {busy === 'scan' ? (source.running_port ? 'Reading…' : 'Starting a copy…') : 'Find databases'}
            </Button>
          </div>
        )}

        {error && <p className="text-sm text-destructive">{error}</p>}

        {dbs && dbs.length === 0 && <p className="text-sm text-muted-foreground">That server has no databases of its own.</p>}
        {dbs && dbs.length > 0 && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">
                Databases · {picked.length}/{dbs.length} selected
              </span>
              <Button size="sm" variant="ghost" onClick={() => setPicked(picked.length === dbs.length ? [] : dbs)}>
                {picked.length === dbs.length ? 'Select none' : 'Select all'}
              </Button>
            </div>
            <div className="grid max-h-56 grid-cols-2 gap-2 overflow-y-auto sm:grid-cols-3">
              {dbs.map((db) => {
                const result = results?.find((r) => r.name === db)
                return (
                  <div key={db} className="flex items-center gap-1.5" title={result?.detail}>
                    <Toggle checked={picked.includes(db)} disabled={busy !== null} onChange={(v) => setPicked(v ? [...picked, db] : picked.filter((x) => x !== db))} label={db} />
                    {result && (result.ok ? <CheckCircle2 className="size-3.5 text-success" /> : <XCircle className="size-3.5 text-destructive" />)}
                  </div>
                )
              })}
            </div>
            <p className="text-xs text-muted-foreground">Imported into MariaDB. If a database with the same name already exists there, tables with the same names are replaced.</p>
          </div>
        )}

        {results && (
          <div className="rounded-lg border border-border p-3 text-sm">
            <p className="font-medium">
              {results.filter((r) => r.ok).length} of {results.length} imported.
            </p>
            {results
              .filter((r) => !r.ok)
              .map((r) => (
                <p key={r.name} className="mt-1 text-xs text-destructive">
                  {r.name}: {r.detail}
                </p>
              ))}
          </div>
        )}
      </div>
    </Dialog>
  )
}
