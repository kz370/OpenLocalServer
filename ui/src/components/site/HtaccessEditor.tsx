import { useEffect, useState } from 'react'

import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Select } from '@/components/ui/form'
import { runCommand } from '@/core'
import { useAction } from '@/lib/hooks'
import { useDirtyForm } from '@/lib/dirtyForms'

const TEMPLATES: Record<string, string> = {
  Laravel: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteRule ^ index.php [L]\n</IfModule>\n',
  WordPress: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteRule ^index\\.php$ - [L]\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteRule . /index.php [L]\n</IfModule>\n',
  SPA: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteRule . /index.html [L]\n</IfModule>\n',
}

export function HtaccessEditor({ hostname }: { hostname: string }) {
  const [content, setContent] = useState('')
  const [server, setServer] = useState('nginx')
  const [exists, setExists] = useState(true)
  const [loaded, setLoaded] = useState(false)
  const action = useAction()
  const { error, setError, run } = action
  const fileForm = useDirtyForm({
    scope: 'dialog',
    formKey: `htaccess:${hostname}`,
    value: { content, exists },
    enabled: loaded,
    busy: action.busy !== null,
    error,
    onRestore: (savedFile) => {
      setContent(savedFile.content)
      setExists(savedFile.exists)
    },
    onSave: async (draft) => {
      const result = await run('save', async () => {
        const r = await runCommand({ type: 'write_site_file', hostname, name: '.htaccess', content: draft.content })
        if (r.type !== 'text') {
          throw {
            problem: 'Could not save .htaccess',
            cause: `The core returned an unexpected response: ${r.type}.`,
            fix: 'Try saving again.',
          }
        }
        setExists(true)
        return true
      })
      return result === true
    },
  })

  useEffect(() => {
    let alive = true
    setLoaded(false)
    Promise.all([runCommand({ type: 'read_site_file', hostname, name: '.htaccess' }), runCommand({ type: 'get_web_config' })]).then(([file, web]) => {
      if (!alive) return
      const nextContent = file.type === 'text' ? file.text : ''
      const nextExists = file.type === 'text' && file.text.length > 0
      setContent(nextContent)
      setExists(nextExists)
      fileForm.syncSaved({ content: nextContent, exists: nextExists })
      if (web.type === 'web_config') setServer(web.config.default_server)
      setLoaded(true)
    }).catch((e: unknown) => { if (alive) setError({ problem: 'Could not read the site file', cause: String(e), fix: null }); if (alive) setLoaded(true) })
    return () => { alive = false }
  }, [fileForm.syncSaved, hostname, setError])

  return <div className="flex min-h-0 flex-1 flex-col gap-3">
    <ErrorCard error={error} onDismiss={() => setError(null)} />
    {server !== 'apache' && <div className="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-sm text-warning">The active server is {server === 'nginx' ? 'Nginx' : 'Caddy'}; it does not read .htaccess files.</div>}
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="flex items-center gap-2"><Badge variant="secondary">Apache</Badge><span className="text-xs text-muted-foreground">{exists ? 'Saved in the site document root' : 'File will be created when saved'}</span></div>
      <div className="flex gap-2">
        <Select
          aria-label="Create .htaccess from template"
          className="w-52"
          triggerClassName="h-8 text-xs"
          value=""
          onChange={(e) => {
            if (e.target.value) {
              setContent(TEMPLATES[e.target.value])
              setExists(false)
            }
          }}
        >
          <option value="">Create from template…</option>
          {Object.keys(TEMPLATES).map((name) => (
            <option key={name} value={name}>{name}</option>
          ))}
        </Select>
      </div>
    </div>
    <CodeEditor value={content} onChange={setContent} language="apache" height="100%" />
  </div>
}
