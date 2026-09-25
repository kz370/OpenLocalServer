import { useEffect, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { type Diagnostic, runCommand } from '@/core'

export function DashboardPage() {
  const [pingStatus, setPingStatus] = useState<'checking' | 'ok' | 'error'>('checking')
  const [version, setVersion] = useState('')
  const [error, setError] = useState<Diagnostic | null>(null)

  const [settingKey, setSettingKey] = useState('editor')
  const [settingValue, setSettingValue] = useState('vscode')
  const [savedValue, setSavedValue] = useState<string | null>(null)

  useEffect(() => {
    runCommand({ type: 'ping' })
      .then((res) => {
        if (res.type === 'pong') {
          setVersion(res.version)
          setPingStatus('ok')
        }
      })
      .catch((err: Diagnostic) => {
        setError(err)
        setPingStatus('error')
      })
  }, [])

  async function handleSave() {
    setError(null)
    try {
      await runCommand({ type: 'set_setting', key: settingKey, value: settingValue })
      const res = await runCommand({ type: 'get_setting', key: settingKey })
      if (res.type === 'setting') {
        setSavedValue(typeof res.value === 'string' ? res.value : JSON.stringify(res.value))
      }
    } catch (err) {
      setError(err as Diagnostic)
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
        <p className="text-sm text-muted-foreground">
          A free, open-source local development environment manager.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle>Core connection</CardTitle>
            {pingStatus === 'ok' && <Badge variant="success">● Online</Badge>}
            {pingStatus === 'checking' && <Badge variant="secondary">Checking…</Badge>}
            {pingStatus === 'error' && <Badge variant="destructive">● Offline</Badge>}
          </CardHeader>
          <CardContent>
            <CardDescription>
              {pingStatus === 'ok' && `ols-core v${version} responded to a ping over IPC.`}
              {pingStatus === 'checking' && 'Waiting for the Rust core to respond…'}
              {pingStatus === 'error' && 'Could not reach the core process.'}
            </CardDescription>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Settings round-trip</CardTitle>
            <CardDescription>Writes to disk, reads it back through the same command.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <div className="flex gap-2">
              <Input
                value={settingKey}
                onChange={(e) => setSettingKey(e.target.value)}
                placeholder="key"
                className="w-28"
              />
              <Input
                value={settingValue}
                onChange={(e) => setSettingValue(e.target.value)}
                placeholder="value"
              />
              <Button onClick={handleSave} size="sm">
                Save
              </Button>
            </div>
            {savedValue !== null && (
              <p className="text-sm text-muted-foreground">
                Read back <code className="rounded bg-muted px-1 py-0.5">{settingKey}</code> ={' '}
                <code className="rounded bg-muted px-1 py-0.5">{savedValue}</code>
              </p>
            )}
          </CardContent>
        </Card>
      </div>

      {error && (
        <Card className="border-destructive/40 bg-destructive/5">
          <CardHeader>
            <CardTitle className="text-destructive">{error.problem}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-1">
            <p className="text-sm">{error.cause}</p>
            {error.fix && <p className="text-sm text-success">{error.fix}</p>}
          </CardContent>
        </Card>
      )}
    </div>
  )
}
