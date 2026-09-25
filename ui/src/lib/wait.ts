import { type Diagnostic, type ProcessId, runCommand } from '@/core'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/**
 * Starting/stopping only *asks*; these wait until it has actually happened, so a button
 * can keep spinning until the thing is really up (its port answers) or really down.
 */
async function until(done: () => Promise<boolean>, what: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await done()) return
    await sleep(400)
  }
  throw { problem: `${what} is taking too long.`, cause: 'It may still finish; check the Logs page.', fix: null } satisfies Diagnostic
}

export async function waitForService(id: string, want: 'running' | 'stopped', timeoutMs = 60_000): Promise<void> {
  const started = Date.now()
  await until(
    async () => {
      const r = await runCommand({ type: 'list_services' })
      const s = r.type === 'services' ? r.services.find((x) => x.id === id) : undefined
      if (!s) return true
      if (want === 'stopped') return !s.running && s.port_status !== 'in_use'
      // The process vanished after being asked to start: it crashed on the way up.
      if (!s.running && Date.now() - started > 1500) {
        throw { problem: `${s.name} stopped right after starting.`, cause: 'See its output on the Logs page.', fix: null } satisfies Diagnostic
      }
      return s.running && (s.port === null || s.healthy === true)
    },
    want === 'running' ? `Starting ${id}` : `Stopping ${id}`,
    timeoutMs,
  )
}

export async function waitForWebStopped(timeoutMs = 20_000): Promise<void> {
  await until(
    async () => {
      const r = await runCommand({ type: 'get_web_status' })
      return r.type === 'web_status' && !r.status.running && r.status.port_conflicts.length === 0
    },
    'Stopping the web server',
    timeoutMs,
  )
}

export async function waitForProcessExit(id: ProcessId, timeoutMs = 20_000): Promise<void> {
  await until(
    async () => {
      const r = await runCommand({ type: 'list_processes' })
      const p = r.type === 'processes' ? r.processes.find((x) => x.id === id) : undefined
      return !p || !['running', 'starting', 'stopping', 'restarting'].includes(p.state)
    },
    'Stopping the process',
    timeoutMs,
  )
}
