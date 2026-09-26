import { useEffect, useState } from 'react'

import { AiButton } from '@/components/ai/AiButton'
import { CodeEditor } from '@/components/CodeEditor'
import { ErrorCard } from '@/components/ErrorCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { runCommand } from '@/core'
import type { AskAi } from '@/lib/ai'
import { useAction } from '@/lib/hooks'

const TEMPLATES: Record<string, string> = {
  Laravel: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteRule ^ index.php [L]\n</IfModule>\n',
  WordPress: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteRule ^index\\.php$ - [L]\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteRule . /index.php [L]\n</IfModule>\n',
  SPA: 'Options -Indexes\n\n<IfModule mod_rewrite.c>\n    RewriteEngine On\n    RewriteCond %{REQUEST_FILENAME} !-f\n    RewriteCond %{REQUEST_FILENAME} !-d\n    RewriteRule . /index.html [L]\n</IfModule>\n',
}

export function HtaccessEditor({ hostname }: { hostname: string }) {
  const [content, setContent] = useState('')
  const [server, setServer] = useState('nginx')
  const [exists, setExists] = useState(true)
  const { busy, error, setError, run } = useAction()

  useEffect(() => {
    let alive = true
    Promise.all([runCommand({ type: 'read_site_file', hostname, name: '.htaccess' }), runCommand({ type: 'get_web_config' })]).then(([file, web]) => {
      if (!alive) return
      if (file.type === 'text') { setContent(file.text); setExists(file.text.length > 0) }
      else { setContent(''); setExists(false) }
      if (web.type === 'web_config') setServer(web.config.server)
    }).catch((e: unknown) => { if (alive) setError({ problem: 'Could not read the site file', cause: String(e), fix: null }) })
    return () => { alive = false }
  }, [hostname, setError])

  const ai = (): AskAi => ({
    request: { feature: 'config', kind: 'htaccess', title: `${hostname} .htaccess`, text: content },
    title: 'Improve .htaccess',
    description: 'The current file is sent to your selected AI provider. Review the diff before applying it.',
    question: 'optional',
    onFile: setContent,
  })

  return <div className="flex min-h-0 flex-1 flex-col gap-3">
    <ErrorCard error={error} onDismiss={() => setError(null)} />
    {server !== 'apache' && <div className="rounded-md border border-warning/40 bg-warning/5 px-3 py-2 text-sm text-warning">The active server is {server === 'nginx' ? 'Nginx' : 'Caddy'}; it does not read .htaccess files.</div>}
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="flex items-center gap-2"><Badge variant="secondary">Apache</Badge><span className="text-xs text-muted-foreground">{exists ? 'Saved in the site document root' : 'File will be created when saved'}</span></div>
      <div className="flex gap-2"><select aria-label="Create .htaccess from template" className="h-8 rounded-md border border-input bg-background px-2 text-xs" defaultValue="" onChange={(e) => { if (e.target.value) { setContent(TEMPLATES[e.target.value]); setExists(false); e.currentTarget.value = '' } }}><option value="">Create from template…</option>{Object.keys(TEMPLATES).map((name) => <option key={name}>{name}</option>)}</select><AiButton label="Suggest with AI" ask={ai} /><Button size="sm" disabled={busy !== null} onClick={() => run('save', async () => { const r = await runCommand({ type: 'write_site_file', hostname, name: '.htaccess', content }); if (r.type === 'text') setExists(true) })}>Save .htaccess</Button></div>
    </div>
    <CodeEditor value={content} onChange={setContent} language="apache" height="100%" />
  </div>
}
