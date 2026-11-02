import { type Diagnostic, type ProcessId, runCommand } from '@/core'
import { resolveNotificationsForService } from '@/lib/notify'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** Default per-service readiness timeout (30s). */
export const DEFAULT_SERVICE_READY_TIMEOUT_MS = 30_000

/** Poll interval while waiting for service readiness (250-500ms range: 300ms). */
export const SERVICE_READY_POLL_INTERVAL_MS = 300

/** Per-service readiness timeouts in milliseconds (configurable per service ID). */
export const SERVICE_READY_TIMEOUTS_MS: Record<string, number> = {
  apache: 30_000,
  nginx: 30_000,
  caddy: 30_000,
  mariadb: 45_000,
  postgres: 45_000,
  mongodb: 30_000,
  redis: 30_000,
  memcached: 30_000,
  mailpit: 30_000,
}

export function getServiceReadyTimeoutMs(id: string): number {
  const norm = id.trim().toLowerCase()
  return SERVICE_READY_TIMEOUTS_MS[norm] ?? DEFAULT_SERVICE_READY_TIMEOUT_MS
}

/**
 * Starting/stopping only *asks*; these wait until it has actually happened, so a button
 * can keep spinning until the thing is really up (its port answers) or really down.
 */
async function until(
  done: () => Promise<boolean>,
  what: string,
  timeoutMs: number,
  pollIntervalMs = SERVICE_READY_POLL_INTERVAL_MS,
): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await done()) return
    await sleep(pollIntervalMs)
  }
  throw { problem: `${what} is taking too long.`, cause: 'It may still finish; check the Logs page.', fix: null } satisfies Diagnostic
}

export async function waitForService(id: string, want: 'running' | 'stopped', timeoutMs?: number): Promise<void> {
  const effectiveTimeoutMs = timeoutMs ?? (want === 'running' ? getServiceReadyTimeoutMs(id) : 20_000)
  let sawRunning = false
  await until(
    async () => {
      const r = await runCommand({ type: 'list_services' })
      const s = r.type === 'services' ? r.services.find((x) => x.id === id) : undefined
      if (!s) return true
      if (want === 'stopped') return !s.running
      if (s.running) {
        sawRunning = true
      } else if (sawRunning) {
        // Re-check once after a short delay in case of a reload/restart bounce before reporting exit
        await sleep(SERVICE_READY_POLL_INTERVAL_MS)
        const recheck = await runCommand({ type: 'list_services' })
        const s2 = recheck.type === 'services' ? recheck.services.find((x) => x.id === id) : undefined
        if (s2 && s2.running && s2.healthy !== false) {
          resolveNotificationsForService(id)
          return true
        }
        if (!s2 || !s2.running) {
          throw { problem: `${s.name} stopped right after starting.`, cause: 'See its output on the Logs page.', fix: null } satisfies Diagnostic
        }
      }
      if (s.running && s.healthy !== false) {
        resolveNotificationsForService(id)
        return true
      }
      return false
    },
    want === 'running' ? `Starting ${id}` : `Stopping ${id}`,
    effectiveTimeoutMs,
    SERVICE_READY_POLL_INTERVAL_MS,
  )
}

export async function waitForWebStopped(timeoutMs = 20_000): Promise<void> {
  await until(
    async () => {
      const r = await runCommand({ type: 'get_web_status' })
      return r.type === 'web_status' && !r.status.running
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
