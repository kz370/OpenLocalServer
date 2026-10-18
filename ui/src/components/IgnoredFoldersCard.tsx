import { Check, FolderPlus, FolderX, Pencil, RotateCcw, Trash2, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import { ErrorCard } from '@/components/ErrorCard'
import { Spinner } from '@/components/Spinner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { runCommand } from '@/core'
import { confirmAction } from '@/lib/confirm'
import { useAction } from '@/lib/hooks'

const DEFAULT_IGNORED = ['excluded_sites']

export function IgnoredFoldersCard() {
  const [folders, setFolders] = useState<string[] | null>(null)
  const [editingIndex, setEditingIndex] = useState<number | null>(null)
  const [editingValue, setEditingValue] = useState('')
  const [newFolder, setNewFolder] = useState('')
  const [saved, setSaved] = useState<string | null>(null)
  const { busy, error, setError, run } = useAction()

  const load = useCallback(async () => {
    const r = await runCommand({ type: 'get_setting', key: 'projects.ignored_folders' })
    if (r.type === 'setting' && Array.isArray(r.value)) {
      setFolders(r.value.filter((v): v is string => typeof v === 'string'))
    } else {
      setFolders(DEFAULT_IGNORED)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  function flash(message: string) {
    setSaved(message)
    setTimeout(() => setSaved(null), 2500)
  }

  async function saveList(next: string[], msg: string) {
    const r = await runCommand({ type: 'set_setting', key: 'projects.ignored_folders', value: next })
    if (r.type === 'ok') {
      setFolders(next)
      flash(msg)
      await runCommand({ type: 'sync_auto_domains' }).catch(() => undefined)
    } else {
      setError({
        problem: 'Could not save ignored folders.',
        cause: 'The settings store refused the update.',
        fix: 'Try again, or reload Settings.',
      })
    }
  }

  async function handleAdd() {
    const name = newFolder.trim()
    if (!name) return
    if (name.includes('/') || name.includes('\\')) {
      setError({
        problem: 'Invalid folder name.',
        cause: 'A folder name cannot contain path separators (/ or \\).',
        fix: 'Enter only the folder name, not a path.',
      })
      return
    }
    const current = folders ?? DEFAULT_IGNORED
    if (current.some((f) => f.toLowerCase() === name.toLowerCase())) {
      setError({
        problem: 'Folder is already ignored.',
        cause: `"${name}" is already in the ignored list.`,
        fix: 'Enter a different folder name.',
      })
      return
    }
    await run('add', async () => {
      const next = [...current, name]
      await saveList(next, `"${name}" will now be ignored by scans.`)
      setNewFolder('')
    })
  }

  async function handleRemove(folder: string) {
    const ok = await confirmAction(
      `OpenLocalServer will scan "${folder}" and its subfolders for projects and automatic domains again.`,
      `Stop ignoring "${folder}"?`,
      'Stop ignoring',
    )
    if (!ok) return
    await run(`remove:${folder}`, async () => {
      const current = folders ?? DEFAULT_IGNORED
      const next = current.filter((f) => f.toLowerCase() !== folder.toLowerCase())
      await saveList(next, `Stopped ignoring "${folder}".`)
    })
  }

  function startRename(index: number, folder: string) {
    setEditingIndex(index)
    setEditingValue(folder)
  }

  function cancelRename() {
    setEditingIndex(null)
    setEditingValue('')
  }

  async function submitRename(oldName: string) {
    const newName = editingValue.trim()
    if (!newName || newName.toLowerCase() === oldName.toLowerCase()) {
      cancelRename()
      return
    }
    if (newName.includes('/') || newName.includes('\\')) {
      setError({
        problem: 'Invalid folder name.',
        cause: 'A folder name cannot contain path separators (/ or \\).',
        fix: 'Enter only the folder name, not a path.',
      })
      return
    }
    const current = folders ?? DEFAULT_IGNORED
    if (current.some((f) => f.toLowerCase() === newName.toLowerCase() && f.toLowerCase() !== oldName.toLowerCase())) {
      setError({
        problem: 'Folder name already in list.',
        cause: `"${newName}" is already being ignored.`,
        fix: 'Choose a different folder name.',
      })
      return
    }

    await run(`rename:${oldName}`, async () => {
      const r = await runCommand({ type: 'rename_ignored_folder', old_name: oldName, new_name: newName })
      if (r.type === 'ok') {
        const next = current.map((f) => (f.toLowerCase() === oldName.toLowerCase() ? newName : f))
        setFolders(next)
        cancelRename()
        flash(`Renamed "${oldName}" to "${newName}".`)
        await runCommand({ type: 'sync_auto_domains' }).catch(() => undefined)
      } else {
        // Fallback to setting update if rename_ignored_folder fails
        const next = current.map((f) => (f.toLowerCase() === oldName.toLowerCase() ? newName : f))
        await saveList(next, `Renamed "${oldName}" to "${newName}".`)
        cancelRename()
      }
    })
  }

  async function handleResetDefault() {
    await run('reset', async () => {
      await saveList(DEFAULT_IGNORED, 'Restored default ignored folder (excluded_sites).')
    })
  }

  if (!folders) return null

  return (
    <>
      <ErrorCard error={error} onDismiss={() => setError(null)} />
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between gap-3">
            <div>
              <CardTitle className="text-sm">Ignored folders</CardTitle>
              <CardDescription>
                Folders with these names are completely ignored during scans. The app never scans or reads their contents.
                Deleted sites are moved to <code className="text-xs">excluded_sites</code> by default.
              </CardDescription>
            </div>
            {folders.length !== 1 || folders[0] !== 'excluded_sites' ? (
              <Button size="sm" variant="ghost" className="h-8 gap-1.5 text-xs text-muted-foreground" onClick={handleResetDefault} disabled={busy !== null} title="Reset to excluded_sites default">
                <RotateCcw className="size-3.5" /> Reset default
              </Button>
            ) : null}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {folders.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No folders are ignored. All directories will be scanned for projects.
            </p>
          ) : (
            <div className="divide-y divide-border overflow-hidden rounded-lg border border-border">
              {folders.map((folder, idx) => {
                const isEditing = editingIndex === idx
                return (
                  <div key={folder} className="flex items-center justify-between gap-3 px-3 py-2">
                    {isEditing ? (
                      <div className="flex flex-1 items-center gap-2">
                        <Input
                          value={editingValue}
                          onChange={(e) => setEditingValue(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') void submitRename(folder)
                            if (e.key === 'Escape') cancelRename()
                          }}
                          autoFocus
                          className="h-8 text-sm"
                          placeholder="Folder name"
                        />
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-8 p-0 text-success hover:bg-success/10"
                          disabled={busy !== null}
                          onClick={() => void submitRename(folder)}
                          title="Save new folder name"
                        >
                          <Check className="size-4" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="size-8 p-0"
                          disabled={busy !== null}
                          onClick={cancelRename}
                          title="Cancel"
                        >
                          <X className="size-4" />
                        </Button>
                      </div>
                    ) : (
                      <>
                        <div className="flex min-w-0 items-center gap-2">
                          <FolderX className="size-4 shrink-0 text-muted-foreground" />
                          <span className="truncate text-sm font-medium" title={folder}>
                            {folder}
                          </span>
                          {folder.toLowerCase() === 'excluded_sites' && (
                            <Badge variant="secondary" className="text-xs">
                              Default
                            </Badge>
                          )}
                        </div>
                        <div className="flex items-center gap-1">
                          <Button
                            size="sm"
                            variant="ghost"
                            className="size-7 px-0 text-muted-foreground hover:text-foreground"
                            disabled={busy !== null}
                            onClick={() => startRename(idx, folder)}
                            title={`Rename "${folder}"`}
                            aria-label={`Rename ${folder}`}
                          >
                            <Pencil className="size-3.5" />
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="size-7 px-0 text-muted-foreground hover:text-destructive"
                            disabled={busy !== null}
                            onClick={() => void handleRemove(folder)}
                            title={`Remove "${folder}" from ignored list`}
                            aria-label={`Remove ${folder}`}
                          >
                            {busy === `remove:${folder}` ? <Spinner /> : <Trash2 className="size-3.5" />}
                          </Button>
                        </div>
                      </>
                    )}
                  </div>
                )
              })}
            </div>
          )}

          <div className="flex gap-2">
            <div className="relative flex-1">
              <Input
                value={newFolder}
                onChange={(e) => setNewFolder(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void handleAdd()
                }}
                placeholder="Folder name to ignore (e.g. excluded_sites, archive)"
                className="h-8 text-sm"
              />
            </div>
            <Button
              size="sm"
              variant="secondary"
              className="h-8"
              disabled={!newFolder.trim() || busy !== null}
              onClick={() => void handleAdd()}
            >
              {busy === 'add' ? <Spinner /> : <FolderPlus className="size-3.5" />} Add folder
            </Button>
          </div>

          {saved && <span className="text-xs text-success">{saved}</span>}
        </CardContent>
      </Card>
    </>
  )
}
