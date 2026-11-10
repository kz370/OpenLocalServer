import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  type CoreCommand,
  type CoreResponse,
  type DashboardData,
  type Diagnostic,
  type Finding,
  type HealthItem,
  type ProcessInfo,
  type ServiceStatus,
  type StartupSettings,
  runCommand,
  setCommandLifecycleHook,
} from '@/core'
import {
  evaluateNotificationConditions,
  notify,
  resolveNotificationByKey,
  resolveNotificationsForService,
} from '@/lib/notify'
import {
  DEFAULT_SERVICE_READY_TIMEOUT_MS,
  SERVICE_READY_POLL_INTERVAL_MS,
  SERVICE_READY_TIMEOUTS_MS,
  getServiceReadyTimeoutMs,
  waitForService,
} from '@/lib/wait'

export {
  DEFAULT_SERVICE_READY_TIMEOUT_MS,
  SERVICE_READY_POLL_INTERVAL_MS,
  SERVICE_READY_TIMEOUTS_MS,
  getServiceReadyTimeoutMs,
}

export function handleExpectedServiceStop(serviceId: string) {
  setServiceIntent(serviceId, 'stopped')
  emitStateChange()
}

export function handleExpectedStopAll() {
  const allIds = new Set<string>([
    ...Object.keys(BUILTIN_SERVICE_NAMES),
    ...serviceDisplayNames.keys(),
    ...currentlyRunningServices,
    ...knownRunningServices,
  ])
  for (const id of allIds) {
    setServiceIntent(id, 'stopped')
  }
  isWebCurrentlyRunning = false
  emitStateChange()
}

// ---------------------------------------------------------------------------
// Configuration constants (Rules 2 & 3 + Intent & Crash Grouping)
// ---------------------------------------------------------------------------

/** Maximum duration of the startup grace period (~45 seconds after launch). */
export const STARTUP_GRACE_PERIOD_MS = 45_000

/** Number of consecutive post-grace-period checks required before notifying. */
export const PERSISTENCE_CONSECUTIVE_CHECKS = 2

/** Minimum elapsed time (ms) between first and confirming check for persistence (~10s). */
export const PERSISTENCE_MIN_INTERVAL_MS = 10_000

/** Poll interval (ms) for periodic diagnostics checks (~10s). */
export const DIAGNOSTICS_POLL_INTERVAL_MS = 10_000

/** Hold timeout (~10s) for stop/restart intent so delayed exit events or in-flight polls never race. */
export const INTENT_HOLD_TIMEOUT_MS = 10_000

/** Short buffer window (ms) to batch simultaneous unexpected service exits into one notification. */
export const CRASH_GROUP_BUFFER_MS = 1_200

/** Window (ms) within which a subsequent unexpected exit upgrades an active toast into a grouped one. */
export const CRASH_GROUP_MERGE_WINDOW_MS = 5_000

/** localStorage key storing unresolved issue IDs across sessions (Rule 6). */
export const UNRESOLVED_ISSUES_STORAGE_KEY = 'ols:diagnostics:unresolved_issues'

const WEB_SERVER_IDS = ['nginx', 'apache', 'caddy'] as const

const BUILTIN_SERVICE_NAMES: Record<string, string> = {
  nginx: 'Nginx',
  apache: 'Apache',
  caddy: 'Caddy',
  mailpit: 'Mailpit',
  mariadb: 'MariaDB',
  postgres: 'PostgreSQL',
  mongodb: 'MongoDB',
  redis: 'Redis',
  memcached: 'Memcached',
}

const NAME_TO_SERVICE_ID: Record<string, string> = {
  nginx: 'nginx',
  apache: 'apache',
  'apache httpd': 'apache',
  caddy: 'caddy',
  mailpit: 'mailpit',
  mariadb: 'mariadb',
  postgresql: 'postgres',
  postgres: 'postgres',
  mongodb: 'mongodb',
  redis: 'redis',
  memcached: 'memcached',
}

// ---------------------------------------------------------------------------
// Per-Service Intent State Machine
// ---------------------------------------------------------------------------

export type ServiceExpectedState = 'running' | 'stopped' | 'restarting'

export interface ServiceIntentRecord {
  expected: ServiceExpectedState
  updatedAtMs: number
  holdUntilMs: number
  exitObserved: boolean
  inFlightOps: number
}

/** Per-service expected state (`running` | `stopped` | `restarting`). Survives poller restarts. */
const serviceIntents = new Map<string, ServiceIntentRecord>()

// ---------------------------------------------------------------------------
// Module-level singleton state (survives component remounts / page navigation / HMR)
// ---------------------------------------------------------------------------

const SESSION_LAUNCH_KEY = 'ols:session:launch_time_ms'
const SESSION_AUTOSTART_DONE_KEY = 'ols:session:autostart_done'
const SESSION_STOPPED_KEY = 'ols:session:stopped_services'
const SESSION_EVER_RUNNING_KEY = 'ols:session:ever_running_services'
const SESSION_LAUNCH_AUTOSTART_KEY = 'ols:session:launch_autostart_ids'

function loadSessionLaunchTimeMs(): number {
  if (typeof window === 'undefined') return Date.now()
  try {
    const raw = sessionStorage.getItem(SESSION_LAUNCH_KEY)
    if (raw) {
      const parsed = Number(raw)
      if (Number.isFinite(parsed) && parsed > 0) return parsed
    }
    const now = Date.now()
    sessionStorage.setItem(SESSION_LAUNCH_KEY, String(now))
    return now
  } catch {
    return Date.now()
  }
}

function loadSessionSet(key: string): Set<string> {
  if (typeof window === 'undefined') return new Set()
  try {
    const raw = sessionStorage.getItem(key)
    if (!raw) return new Set()
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? new Set(parsed.filter((x): x is string => typeof x === 'string')) : new Set()
  } catch {
    return new Set()
  }
}

function saveSessionSet(key: string, set: Set<string>) {
  if (typeof window === 'undefined') return
  try {
    sessionStorage.setItem(key, JSON.stringify([...set]))
  } catch {
    // ignore
  }
}

const appLaunchTimeMs = loadSessionLaunchTimeMs()

let startupSettingsLoaded = false
let liveDashboardLoaded = false
let autostartCompletedEventReceived =
  typeof window !== 'undefined' && sessionStorage.getItem(SESSION_AUTOSTART_DONE_KEY) === '1'
let isShuttingDown = false

let autostartIds: string[] = []
/**
 * Autostart IDs as they were when the settings first loaded at launch. Only these were
 * actually started by the app this session; a service ticked later was never started, so
 * it must not be reported as an autostart failure or shown as "starting".
 */
// Kept in sessionStorage with the launch time: a page reload keeps the old launch time,
// so it must keep the old snapshot too, or services ticked since then count as autostart.
let launchAutostartIds: Set<string> | null = (() => {
  if (typeof window === 'undefined') return null
  try {
    return sessionStorage.getItem(SESSION_LAUNCH_AUTOSTART_KEY) !== null
      ? loadSessionSet(SESSION_LAUNCH_AUTOSTART_KEY)
      : null
  } catch {
    return null
  }
})()
let defaultWebServerId = 'nginx'
let isWebCurrentlyRunning = false

/** Display names learned from live service listings. */
const serviceDisplayNames = new Map<string, string>(Object.entries(BUILTIN_SERVICE_NAMES))

/** Services currently observed with `running === true`. */
const currentlyRunningServices = new Set<string>()

/** Services currently observed with `running === true && healthy !== false`. */
const currentlyHealthyRunningServices = new Set<string>()

/** Services that have been confirmed `running === true` (and healthy) at least once since last start. */
const knownRunningServices = new Set<string>()

/** Services that have been observed running at least once during this app session. */
const everRunningThisSession = loadSessionSet(SESSION_EVER_RUNNING_KEY)

/** Services explicitly stopped by the user during this app session. */
const explicitlyStoppedThisSession = loadSessionSet(SESSION_STOPPED_KEY)

// Seed intents for services already stopped by the user in this session (survives HMR)
for (const stoppedId of explicitlyStoppedThisSession) {
  serviceIntents.set(stoppedId, {
    expected: 'stopped',
    updatedAtMs: Date.now(),
    holdUntilMs: Date.now() + INTENT_HOLD_TIMEOUT_MS,
    exitObserved: true,
    inFlightOps: 0,
  })
}

/** Services currently being started manually or via restart/apply by the user. */
const userStartingServices = new Set<string>()

/** Services in `autostart_services` whose startup attempt failed this session. */
const autostartFailedServices = new Set<string>()

/** Services for which an unexpected-exit or start-failure notification is currently active. */
const activeCrashNotifiedServices = new Set<string>()

/** Services whose most recent start attempt failed (so process exit does not double-notify). */
const startFailureNotifiedServices = new Set<string>()

/** Process IDs (`ProcessInfo.id`) already processed for crash/failure. */
const notifiedProcessIds = new Set<number>()

/** Most recent `ProcessInfo` observed per service ID (for exit code lookup). */
const latestProcessByService = new Map<string, ProcessInfo>()

/** Whether the silent initial baseline check has completed (Rule 1). */
let hasCompletedBaseline = false

/** Issue IDs captured in the silent baseline or during the startup grace period. */
const baselineIssueIds = new Set<string>()

/** Unresolved issue IDs persisted across checks and sessions (Rule 6). */
const unresolvedIssueIds = loadUnresolvedIssues()

interface PendingCandidate {
  firstSeenMs: number
  lastSeenMs: number
  checkCount: number
  finding: Finding
}

/** Post-grace-period candidate issues awaiting confirmation across consecutive checks (Rule 3). */
const pendingCandidates = new Map<string, PendingCandidate>()

/** Subscribers notified when the set of "starting" services changes or a re-scan is needed. */
const stateListeners = new Set<() => void>()
const rescanListeners = new Set<() => void>()

// ---------------------------------------------------------------------------
// Intent Helpers
// ---------------------------------------------------------------------------

function getOrCreateIntent(serviceId: string): ServiceIntentRecord {
  let rec = serviceIntents.get(serviceId)
  if (!rec) {
    rec = {
      expected: 'stopped',
      updatedAtMs: 0,
      holdUntilMs: 0,
      exitObserved: true,
      inFlightOps: 0,
    }
    serviceIntents.set(serviceId, rec)
  }
  return rec
}

export function getServiceExpectedState(serviceId: string): ServiceExpectedState {
  return serviceIntents.get(serviceId)?.expected ?? 'stopped'
}

/**
 * Sets the expected state for a service BEFORE sending any stop, start, or restart signal.
 */
export function setServiceIntent(
  serviceId: string,
  expected: ServiceExpectedState,
  opts: { inFlightDelta?: number; starting?: boolean; holdMs?: number } = {},
) {
  const now = Date.now()
  const holdMs = opts.holdMs ?? INTENT_HOLD_TIMEOUT_MS
  const rec = getOrCreateIntent(serviceId)

  rec.expected = expected
  rec.updatedAtMs = now
  if (opts.inFlightDelta) {
    rec.inFlightOps = Math.max(0, rec.inFlightOps + opts.inFlightDelta)
  }

  if (expected === 'stopped') {
    rec.holdUntilMs = now + holdMs
    rec.exitObserved = !currentlyRunningServices.has(serviceId)
    userStartingServices.delete(serviceId)
    knownRunningServices.delete(serviceId)
    currentlyRunningServices.delete(serviceId)
    currentlyHealthyRunningServices.delete(serviceId)
    pendingUnexpectedExits.delete(serviceId)
    activeCrashNotifiedServices.delete(serviceId)
    startFailureNotifiedServices.delete(serviceId)
    autostartFailedServices.delete(serviceId)
    explicitlyStoppedThisSession.add(serviceId)
    saveSessionSet(SESSION_STOPPED_KEY, explicitlyStoppedThisSession)
    resolveNotificationsForService(serviceId)
  } else if (expected === 'restarting') {
    rec.holdUntilMs = now + holdMs
    rec.exitObserved = false
    activeCrashNotifiedServices.delete(serviceId)
    startFailureNotifiedServices.delete(serviceId)
    autostartFailedServices.delete(serviceId)
    pendingUnexpectedExits.delete(serviceId)
    if (opts.starting === true) {
      userStartingServices.add(serviceId)
      explicitlyStoppedThisSession.delete(serviceId)
      saveSessionSet(SESSION_STOPPED_KEY, explicitlyStoppedThisSession)
    }
  } else if (expected === 'running') {
    // Starting a service clears any prior 'stopped' hold so a later crash is never hidden
    rec.holdUntilMs = opts.starting ? now + holdMs : 0
    rec.exitObserved = false
    activeCrashNotifiedServices.delete(serviceId)
    startFailureNotifiedServices.delete(serviceId)
    autostartFailedServices.delete(serviceId)
    pendingUnexpectedExits.delete(serviceId)
    explicitlyStoppedThisSession.delete(serviceId)
    saveSessionSet(SESSION_STOPPED_KEY, explicitlyStoppedThisSession)
    if (opts.starting) {
      userStartingServices.add(serviceId)
    }
  }
}

function finishServiceCommandHold(serviceId: string, succeeded: boolean) {
  const rec = serviceIntents.get(serviceId)
  if (!rec) return
  rec.inFlightOps = Math.max(0, rec.inFlightOps - 1)
  const now = Date.now()
  if (rec.expected === 'stopped' || rec.expected === 'restarting') {
    // Keep the intent hold for INTENT_HOLD_TIMEOUT_MS after the operation returns
    // so an exit event or poll arriving after return is still recognized as expected.
    rec.holdUntilMs = Math.max(rec.holdUntilMs, now + INTENT_HOLD_TIMEOUT_MS)
  }
  if (!succeeded && (rec.expected === 'running' || rec.expected === 'restarting')) {
    userStartingServices.delete(serviceId)
  }
}

/**
 * Returns true if the service has an active `'stopped'` or `'restarting'` intent
 * (or the app is shutting down), meaning any process exit is expected and normal.
 */
function isStopOrRestartExpected(serviceId: string): boolean {
  if (isShuttingDown) return true
  const rec = serviceIntents.get(serviceId)
  if (!rec) return true
  if (rec.expected === 'stopped' || rec.expected === 'restarting') {
    return true
  }
  return false
}

/**
 * Marks all services as stopping prior to application exit / quit.
 */
export function markAppShuttingDown() {
  isShuttingDown = true
  pendingUnexpectedExits.clear()
  if (crashFlushTimer) {
    clearTimeout(crashFlushTimer)
    crashFlushTimer = null
  }
  const allIds = new Set<string>([
    ...Object.keys(BUILTIN_SERVICE_NAMES),
    ...serviceDisplayNames.keys(),
    ...currentlyRunningServices,
    ...knownRunningServices,
  ])
  for (const id of allIds) {
    setServiceIntent(id, 'stopped')
  }
}

// ---------------------------------------------------------------------------
// Storage & Subscription Helpers
// ---------------------------------------------------------------------------

function loadUnresolvedIssues(): Set<string> {
  if (typeof window === 'undefined') return new Set()
  try {
    const raw = localStorage.getItem(UNRESOLVED_ISSUES_STORAGE_KEY)
    if (!raw) return new Set()
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? new Set(parsed.filter((x): x is string => typeof x === 'string')) : new Set()
  } catch {
    return new Set()
  }
}

function saveUnresolvedIssues() {
  if (typeof window === 'undefined') return
  try {
    localStorage.setItem(UNRESOLVED_ISSUES_STORAGE_KEY, JSON.stringify([...unresolvedIssueIds]))
  } catch {
    // ignore storage errors
  }
}

function emitStateChange() {
  stateListeners.forEach((fn) => fn())
}

function requestDiagnosticsRescan() {
  rescanListeners.forEach((fn) => fn())
}

export function getServiceDisplayName(id: string): string {
  return serviceDisplayNames.get(id) ?? BUILTIN_SERVICE_NAMES[id] ?? id
}

function resolveServiceIdFromProcessName(name: string): string | null {
  const lower = name.trim().toLowerCase()
  if (NAME_TO_SERVICE_ID[lower]) return NAME_TO_SERVICE_ID[lower]
  for (const [id, displayName] of Object.entries(BUILTIN_SERVICE_NAMES)) {
    if (id.toLowerCase() === lower || displayName.toLowerCase() === lower) {
      return id
    }
  }
  for (const [id, disp] of serviceDisplayNames.entries()) {
    if (disp.toLowerCase() === lower || id.toLowerCase() === lower) {
      return id
    }
  }
  return null
}

function logSourceForService(id: string): string {
  if ((WEB_SERVER_IDS as readonly string[]).includes(id)) {
    return `web:${id}:error`
  }
  if (id === 'web') {
    return `web:${defaultWebServerId}:error`
  }
  return `service:${id}`
}

/** Returns the effective list of service IDs configured for auto-start. */
function getEffectiveAutostartIds(): string[] {
  const result = new Set<string>()
  for (const id of autostartIds) {
    if (launchAutostartIds && !launchAutostartIds.has(id)) continue
    if (id === 'web') {
      result.add(defaultWebServerId)
    } else {
      result.add(id)
    }
  }
  return [...result]
}

/**
 * Returns the set of service IDs currently in the `"starting"` state (Rule 2):
 * - Any service being started manually by the user, plus
 * - While within `STARTUP_GRACE_PERIOD_MS`, any service configured in `autostart_services`
 *   that has not yet reached healthy running state and has not failed or been stopped by the user.
 */
export function getStartingServiceIds(): Set<string> {
  const starting = new Set<string>(userStartingServices)
  const withinWindow =
    !autostartCompletedEventReceived && Date.now() - appLaunchTimeMs < STARTUP_GRACE_PERIOD_MS

  if (withinWindow && startupSettingsLoaded) {
    for (const id of getEffectiveAutostartIds()) {
      const intent = serviceIntents.get(id)
      const explicitlyStopped =
        explicitlyStoppedThisSession.has(id) ||
        (intent?.expected === 'stopped' && intent.updatedAtMs > 0)
      if (
        !currentlyHealthyRunningServices.has(id) &&
        !everRunningThisSession.has(id) &&
        !autostartFailedServices.has(id) &&
        !explicitlyStopped
      ) {
        starting.add(id)
        if ((WEB_SERVER_IDS as readonly string[]).includes(id)) {
          starting.add('web')
        }
      }
    }
  }

  for (const id of userStartingServices) {
    if ((WEB_SERVER_IDS as readonly string[]).includes(id)) {
      starting.add('web')
    }
  }

  return starting
}

/**
 * Rule 2: Whether the startup grace period is currently active.
 * Ends when auto-start of configured services finishes (all running or failed)
 * OR after `STARTUP_GRACE_PERIOD_MS` (~45s), whichever ends first.
 */
export function isInStartupGracePeriod(): boolean {
  if (autostartCompletedEventReceived || Date.now() - appLaunchTimeMs >= STARTUP_GRACE_PERIOD_MS) {
    return false
  }
  if (!startupSettingsLoaded || !liveDashboardLoaded) {
    return true
  }
  const effective = getEffectiveAutostartIds()
  if (effective.length === 0) {
    return false
  }
  const anyStillStarting = effective.some((id) => {
    const intent = serviceIntents.get(id)
    const explicitlyStopped =
      explicitlyStoppedThisSession.has(id) ||
      (intent?.expected === 'stopped' && intent.updatedAtMs > 0)
    return (
      !currentlyHealthyRunningServices.has(id) &&
      !everRunningThisSession.has(id) &&
      !autostartFailedServices.has(id) &&
      !explicitlyStopped
    )
  })
  return anyStillStarting
}

/** React hook returning the live set of `"starting"` service IDs. */
export function useStartingServices(): Set<string> {
  const [, setTick] = useState(0)
  useEffect(() => {
    const listener = () => setTick((t) => t + 1)
    stateListeners.add(listener)
    const timer = setInterval(listener, 1000)
    return () => {
      stateListeners.delete(listener)
      clearInterval(timer)
    }
  }, [])
  return getStartingServiceIds()
}

/** Subscribe to requests for an immediate diagnostics re-scan (e.g. when auto-start completes). */
export function subscribeDiagnosticsRescan(fn: () => void): () => void {
  rescanListeners.add(fn)
  return () => {
    rescanListeners.delete(fn)
  }
}

// ---------------------------------------------------------------------------
// Per-Service Operation Serialization & Live Readiness Verification
// ---------------------------------------------------------------------------

const serviceOpQueues = new Map<string, Promise<unknown>>()
const inFlightStartPromises = new Map<string, Promise<unknown>>()

/**
 * Serializes start/stop/restart operations per service ID:
 * - A second start while a start is pending joins the first one and does not start a second process.
 * - A start for an already running and ready service is a no-op and never an error.
 * - A start after a stop waits for the stop to finish.
 */
export async function runSerializedServiceOp<T>(
  serviceId: string,
  op: 'start' | 'stop' | 'restart' | 'reload',
  fn: () => Promise<T>,
): Promise<T> {
  const canonicalId = serviceId === 'web' ? defaultWebServerId : serviceId

  if (op === 'start') {
    const existingStart = inFlightStartPromises.get(canonicalId)
    if (existingStart) {
      return (await existingStart) as T
    }
    if (currentlyHealthyRunningServices.has(canonicalId)) {
      resolveNotificationsForService(canonicalId)
      return undefined as T
    }
  }

  const prevOp = serviceOpQueues.get(canonicalId) ?? Promise.resolve()
  const runNext = async (): Promise<T> => {
    await prevOp.catch(() => undefined)
    if (op === 'start' && currentlyHealthyRunningServices.has(canonicalId)) {
      resolveNotificationsForService(canonicalId)
      return undefined as T
    }
    return await fn()
  }

  const nextPromise = runNext()
  serviceOpQueues.set(canonicalId, nextPromise)
  if (op === 'start') {
    inFlightStartPromises.set(canonicalId, nextPromise)
  }

  try {
    return await nextPromise
  } finally {
    if (inFlightStartPromises.get(canonicalId) === nextPromise) {
      inFlightStartPromises.delete(canonicalId)
    }
    if (serviceOpQueues.get(canonicalId) === nextPromise) {
      serviceOpQueues.delete(canonicalId)
    }
  }
}

/**
 * Re-checks the current live state of a service before reporting any failure.
 * If the service is running and ready now (or is currently alive and still binding its port
 * within the readiness timeout), treats the start as a success and returns `true` (meaning
 * the service IS ready and no failure notification should be shown).
 */
async function isServiceReadyOnLiveRecheck(serviceId: string): Promise<boolean> {
  const canonicalId = serviceId === 'web' ? defaultWebServerId : serviceId
  if (currentlyHealthyRunningServices.has(canonicalId)) {
    resolveNotificationsForService(canonicalId)
    return true
  }

  const deadline = Date.now() + getServiceReadyTimeoutMs(canonicalId)
  while (Date.now() < deadline) {
    const res = await runCommand({ type: 'list_services' }).catch(() => null)
    if (res?.type !== 'services') {
      return currentlyHealthyRunningServices.has(canonicalId)
    }
    updateFromServicesList(res.services)
    const svc = res.services.find((s) => s.id === canonicalId)
    if (!svc) return false
    if (svc.running && svc.healthy !== false) {
      resolveNotificationsForService(canonicalId)
      return true
    }
    // If the process is not running at all (and no start is in flight), it has already exited
    if (!svc.running && !inFlightStartPromises.has(canonicalId)) {
      return false
    }
    await new Promise((r) => setTimeout(r, SERVICE_READY_POLL_INTERVAL_MS))
  }

  return currentlyHealthyRunningServices.has(canonicalId)
}

async function fetchUsefulFailureReason(serviceId: string, fallbackReason: string): Promise<string> {
  const cleaned = cleanFailureReason(fallbackReason)
  const isGeneric =
    !cleaned ||
    cleaned.toLowerCase() === `${getServiceDisplayName(serviceId).toLowerCase()} failed to start.` ||
    cleaned.toLowerCase() === `failed to start ${getServiceDisplayName(serviceId).toLowerCase()}.` ||
    cleaned.toLowerCase() === 'see its output on the logs page.'

  if (!isGeneric) {
    return cleaned
  }

  const logRes = await runCommand({
    type: 'read_log',
    source: logSourceForService(serviceId),
    max_lines: 12,
  }).catch(() => null)

  if (logRes?.type === 'log_lines' && logRes.lines.length > 0) {
    const failedLine = logRes.lines.find((l) =>
      l.toLowerCase().startsWith('last start attempt failed:'),
    )
    if (failedLine) {
      const extracted = cleanFailureReason(failedLine)
      if (extracted) return extracted
    }
    const { lastLine } = pickLastErrorLine(logRes.lines)
    if (lastLine) return lastLine
  }

  return cleaned || `${getServiceDisplayName(serviceId)} could not be started.`
}

// ---------------------------------------------------------------------------
// Event Notifications: Start Failure & Unexpected Exit (with Grouping & Actions)
// ---------------------------------------------------------------------------

function cleanFailureReason(raw: string): string {
  return raw
    .replace(/^last start attempt failed:\s*/i, '')
    .replace(/^Web server error:\s*/i, '')
    .replace(/^Service error:\s*/i, '')
    .trim()
}

function recordAutostartFailure(serviceId: string, rawReason: string) {
  const canonicalId = serviceId === 'web' ? defaultWebServerId : serviceId
  if (currentlyHealthyRunningServices.has(canonicalId)) {
    resolveNotificationsForService(canonicalId)
    return
  }
  const intent = serviceIntents.get(canonicalId)
  if (
    autostartFailedServices.has(canonicalId) ||
    activeCrashNotifiedServices.has(canonicalId) ||
    startFailureNotifiedServices.has(canonicalId) ||
    (intent?.expected === 'stopped' && intent.updatedAtMs > 0) ||
    isShuttingDown
  ) {
    return
  }

  autostartFailedServices.add(canonicalId)
  activeCrashNotifiedServices.add(canonicalId)
  startFailureNotifiedServices.add(canonicalId)

  void (async () => {
    // Before reporting a failure, re-check the live state of the service.
    // If it is running and ready now, treat the start as success and do NOT notify.
    if (await isServiceReadyOnLiveRecheck(canonicalId)) {
      autostartFailedServices.delete(canonicalId)
      activeCrashNotifiedServices.delete(canonicalId)
      startFailureNotifiedServices.delete(canonicalId)
      return
    }
    if (isShuttingDown || currentlyHealthyRunningServices.has(canonicalId)) {
      return
    }

    userStartingServices.delete(canonicalId)
    const rec = getOrCreateIntent(canonicalId)
    rec.expected = 'stopped'
    emitStateChange()
    requestDiagnosticsRescan()

    const name = getServiceDisplayName(canonicalId)
    const reason = await fetchUsefulFailureReason(canonicalId, rawReason)
    if (currentlyHealthyRunningServices.has(canonicalId)) {
      resolveNotificationsForService(canonicalId)
      return
    }

    const isPortConflict = /port\s+\d+\s+is\s+(already\s+)?in\s+use|bind\(\)|address already in use/i.test(reason)
    const conditionKey = isPortConflict ? `port-conflict:${canonicalId}` : `start-failed:${canonicalId}`

    notify.error(`${name} failed to start.`, {
      id: `start-failed:${canonicalId}`,
      conditionKey,
      conditionKeys: [`start-failed:${canonicalId}`, `port-conflict:${canonicalId}`],
      resolveWhen: () => currentlyHealthyRunningServices.has(canonicalId),
      title: 'Service failed to start',
      details: reason || `${name} could not be started during startup.`,
      logSource: logSourceForService(canonicalId),
    })
  })()
}

/**
 * Rule 4: Notify once when a user-requested service start (manual or "Start all") fails.
 * Before reporting a failure, re-checks the current live state of the service; if it is
 * running and ready now, treats the start as success and does NOT notify.
 */
export function notifyManualStartFailure(
  serviceId: string,
  serviceName: string,
  diag: Pick<Diagnostic, 'problem' | 'cause' | 'fix'>,
) {
  const canonicalId = serviceId === 'web' ? defaultWebServerId : serviceId
  serviceDisplayNames.set(canonicalId, serviceName)

  if (currentlyHealthyRunningServices.has(canonicalId)) {
    resolveNotificationsForService(canonicalId)
    return
  }

  void (async () => {
    if (await isServiceReadyOnLiveRecheck(canonicalId)) {
      return
    }
    if (isShuttingDown || currentlyHealthyRunningServices.has(canonicalId)) {
      return
    }

    userStartingServices.delete(canonicalId)
    knownRunningServices.delete(canonicalId)
    activeCrashNotifiedServices.add(canonicalId)
    startFailureNotifiedServices.add(canonicalId)
    pendingUnexpectedExits.delete(canonicalId)

    const rec = getOrCreateIntent(canonicalId)
    rec.expected = 'stopped'
    rec.holdUntilMs = Date.now() + INTENT_HOLD_TIMEOUT_MS
    emitStateChange()

    const rawCause = diag.cause || diag.problem || ''
    const usefulReason = await fetchUsefulFailureReason(canonicalId, rawCause)
    if (currentlyHealthyRunningServices.has(canonicalId)) {
      resolveNotificationsForService(canonicalId)
      return
    }

    const problem = diag.problem || `${serviceName} failed to start.`
    const details = usefulReason
      ? `${usefulReason}${diag.fix ? `\nFix: ${diag.fix}` : ''}`
      : undefined
    const isPortConflict = /port\s+\d+\s+is\s+(already\s+)?in\s+use|bind\(\)|address already in use/i.test(
      `${problem} ${usefulReason}`,
    )
    const conditionKey = isPortConflict ? `port-conflict:${canonicalId}` : `start-failed:${canonicalId}`

    notify.error(problem, {
      id: `start-failed:${canonicalId}`,
      conditionKey,
      conditionKeys: [`start-failed:${canonicalId}`, `port-conflict:${canonicalId}`],
      resolveWhen: () => currentlyHealthyRunningServices.has(canonicalId),
      title: 'Service failed to start',
      details,
      logSource: logSourceForService(canonicalId),
    })
  })()
}

// ---------------------------------------------------------------------------
// Unexpected Exit Grouping, Log Extraction & Restart Action
// ---------------------------------------------------------------------------

interface PendingUnexpectedExit {
  serviceId: string
  detectedAtMs: number
  exitCodeHint?: number | null
  reasonHint?: string
}

interface ResolvedExitInfo {
  serviceId: string
  serviceName: string
  bodyLine: string
  extraDetails: string | null
  logSource: string
}

const pendingUnexpectedExits = new Map<string, PendingUnexpectedExit>()
let crashFlushTimer: ReturnType<typeof setTimeout> | null = null

let lastCrashNotificationId: string | null = null
let lastCrashNotificationAtMs = 0
let lastCrashBatchItems: ResolvedExitInfo[] = []

async function restartExitedServices(serviceIds: string[]) {
  for (const id of serviceIds) {
    try {
      setServiceIntent(id, 'running', { starting: true })
      emitStateChange()
      await runCommand({ type: 'start_service', id })
      await waitForService(id, 'running')
    } catch (err) {
      const diag = err as Diagnostic
      notifyManualStartFailure(id, getServiceDisplayName(id), {
        problem: diag?.problem || `Failed to restart ${getServiceDisplayName(id)}.`,
        cause: diag?.cause || '',
        fix: diag?.fix ?? null,
      })
    }
  }
  await runCommand({ type: 'get_dashboard' }).catch(() => null)
}

/**
 * Extracts the last meaningful error line from a service's log lines (single line, trimmed).
 */
function pickLastErrorLine(lines: string[]): { lastLine: string | null; extraLines: string | null } {
  const cleaned = lines
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.toLowerCase().startsWith('last start attempt failed:'))

  if (cleaned.length === 0) {
    return { lastLine: null, extraLines: null }
  }

  // Prefer the last line that looks like an error/fatal/warn message, or fall back to the very last line
  const errorRegex = /(error|fatal|emerg|crit|alert|panic|exception|failed|refused|in use|bind\(\)|denied|corrupt|abort)/i
  let chosenIdx = cleaned.length - 1
  for (let i = cleaned.length - 1; i >= Math.max(0, cleaned.length - 8); i--) {
    if (errorRegex.test(cleaned[i])) {
      chosenIdx = i
      break
    }
  }

  const rawLine = cleaned[chosenIdx]
    // Strip leading timestamps like `2026-11-02 04:00:00 [error] ` when present to keep the one-line body concise
    .replace(/^\[\d{4}[-/]\d{2}[-/]\d{2}[T\s]\d{2}:\d{2}:\d{2}[^\]]*\]\s*/, '')
    .replace(/^\d{4}[-/]\d{2}[-/]\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\s+/, '')
    .trim()

  const singleLine = rawLine.length > 180 ? `${rawLine.slice(0, 177)}...` : rawLine
  const tailExcerpt = cleaned.slice(-5).join('\n')
  const extraLines =
    cleaned.length > 1 && tailExcerpt.trim() !== singleLine.trim() ? tailExcerpt : null

  return {
    lastLine: singleLine || null,
    extraLines,
  }
}

async function resolveExitInfo(item: PendingUnexpectedExit): Promise<ResolvedExitInfo> {
  const serviceId = item.serviceId
  const serviceName = getServiceDisplayName(serviceId)
  const logSource = logSourceForService(serviceId)

  const proc = latestProcessByService.get(serviceId)
  const exitCode =
    item.exitCodeHint !== undefined && item.exitCodeHint !== null
      ? item.exitCodeHint
      : (proc?.exit_code ?? null)

  const logRes = await runCommand({
    type: 'read_log',
    source: logSource,
    max_lines: 12,
  }).catch(() => null)

  const logLines = logRes?.type === 'log_lines' ? logRes.lines : []
  const { lastLine, extraLines } = pickLastErrorLine(logLines)

  const hasFatalErrorInLog =
    lastLine !== null &&
    /(fatal|emerg|crit|panic|bind\(\)|address already in use|syntax error|access is denied|segmentation)/i.test(
      lastLine,
    )

  // A service that was running with expected === 'running' and exited without an internal fatal log
  // error (e.g. killed via Task Manager / taskkill on Windows) was stopped outside OLS.
  const externalStopLabel = !hasFatalErrorInLog ? 'stopped outside OLS' : null

  let bodyLine: string
  if (exitCode !== null && lastLine) {
    bodyLine = externalStopLabel
      ? `Exit code ${exitCode} (${externalStopLabel}): ${lastLine}`
      : `Exit code ${exitCode}: ${lastLine}`
  } else if (exitCode !== null) {
    bodyLine = `Exit code ${exitCode} (stopped outside OLS)`
  } else if (lastLine) {
    bodyLine = externalStopLabel ? `Stopped outside OLS: ${lastLine}` : lastLine
  } else if (item.reasonHint) {
    bodyLine = item.reasonHint
  } else {
    bodyLine = 'Stopped outside OLS while running.'
  }

  return {
    serviceId,
    serviceName,
    bodyLine,
    extraDetails: extraLines,
    logSource,
  }
}

async function flushUnexpectedExits() {
  crashFlushTimer = null
  if (isShuttingDown || pendingUnexpectedExits.size === 0) {
    pendingUnexpectedExits.clear()
    return
  }

  // Filter out any service that became stopped/restarting by user intent while buffered
  const entries = [...pendingUnexpectedExits.values()].filter(
    (e) => !isStopOrRestartExpected(e.serviceId) && !startFailureNotifiedServices.has(e.serviceId),
  )
  pendingUnexpectedExits.clear()

  if (entries.length === 0) return

  // Refresh process snapshot once so exit codes are up to date
  const procRes = await runCommand({ type: 'list_processes' }).catch(() => null)
  if (procRes?.type === 'processes') {
    for (const p of procRes.processes) {
      const sid = resolveServiceIdFromProcessName(p.name)
      if (sid) {
        latestProcessByService.set(sid, p)
      }
    }
  }

  if (isShuttingDown) return

  const resolvedBatch: ResolvedExitInfo[] = []
  for (const entry of entries) {
    if (isStopOrRestartExpected(entry.serviceId) || currentlyHealthyRunningServices.has(entry.serviceId)) {
      continue
    }
    resolvedBatch.push(await resolveExitInfo(entry))
  }

  if (resolvedBatch.length === 0 || isShuttingDown) return

  const now = Date.now()
  const canMergeWithRecent =
    lastCrashNotificationId !== null &&
    now - lastCrashNotificationAtMs <= CRASH_GROUP_MERGE_WINDOW_MS &&
    lastCrashBatchItems.length > 0

  let combinedItems: ResolvedExitInfo[]
  let targetNotificationId: string

  if (canMergeWithRecent && lastCrashNotificationId) {
    const map = new Map<string, ResolvedExitInfo>()
    for (const item of lastCrashBatchItems) map.set(item.serviceId, item)
    for (const item of resolvedBatch) map.set(item.serviceId, item)
    combinedItems = [...map.values()]
    targetNotificationId = lastCrashNotificationId
  } else {
    combinedItems = resolvedBatch
    targetNotificationId =
      combinedItems.length === 1
        ? `crashed:${combinedItems[0].serviceId}`
        : `crashed:${combinedItems.map((i) => i.serviceId).sort().join(',')}`
  }

  lastCrashNotificationId = targetNotificationId
  lastCrashNotificationAtMs = now
  lastCrashBatchItems = combinedItems

  if (combinedItems.length === 1) {
    const single = combinedItems[0]
    const conditionKey = `crashed:${single.serviceId}`
    notify.error(single.bodyLine, {
      id: conditionKey,
      conditionKey,
      conditionKeys: [conditionKey],
      resolveWhen: () => currentlyHealthyRunningServices.has(single.serviceId),
      title: `${single.serviceName} stopped unexpectedly`,
      details: single.extraDetails,
      logSource: single.logSource,
      action: {
        label: 'Restart',
        primary: true,
        onClick: () => restartExitedServices([single.serviceId]),
      },
    })
  } else {
    const count = combinedItems.length
    const names = combinedItems.map((i) => i.serviceName).join(', ')
    const detailsList = combinedItems.map((i) => `${i.serviceName} — ${i.bodyLine}`).join('\n')
    const allIds = combinedItems.map((i) => i.serviceId)
    const conditionKeys = allIds.map((id) => `crashed:${id}`)
    const groupKey = `crashed:${[...allIds].sort().join(',')}`
    notify.error(`${names} stopped unexpectedly.`, {
      id: targetNotificationId,
      conditionKey: groupKey,
      conditionKeys,
      resolveWhen: () => allIds.every((id) => currentlyHealthyRunningServices.has(id)),
      title: `${count} services stopped unexpectedly`,
      details: detailsList,
      logSource: combinedItems[0].logSource,
      action: {
        label: 'Restart all',
        primary: true,
        onClick: () => restartExitedServices(allIds),
      },
    })
  }
}

function recordUnexpectedServiceExit(
  serviceId: string,
  opts: { exitCode?: number | null; reasonHint?: string } = {},
) {
  const canonicalId = serviceId === 'web' ? defaultWebServerId : serviceId

  // Never notify if stop or restart was expected, or if start failed, or if already notified
  if (
    isStopOrRestartExpected(canonicalId) ||
    activeCrashNotifiedServices.has(canonicalId) ||
    startFailureNotifiedServices.has(canonicalId) ||
    autostartFailedServices.has(canonicalId) ||
    userStartingServices.has(canonicalId)
  ) {
    return
  }

  // Must have been known to be running (and expected === 'running')
  if (!knownRunningServices.has(canonicalId)) {
    // If it was an auto-start service during the startup grace period, report as start failure
    if (
      Date.now() - appLaunchTimeMs < STARTUP_GRACE_PERIOD_MS &&
      getEffectiveAutostartIds().includes(canonicalId)
    ) {
      recordAutostartFailure(
        canonicalId,
        opts.reasonHint || `${getServiceDisplayName(canonicalId)} exited during startup.`,
      )
    }
    return
  }

  activeCrashNotifiedServices.add(canonicalId)
  knownRunningServices.delete(canonicalId)
  currentlyRunningServices.delete(canonicalId)
  currentlyHealthyRunningServices.delete(canonicalId)

  const rec = getOrCreateIntent(canonicalId)
  rec.exitObserved = true

  emitStateChange()
  requestDiagnosticsRescan()

  pendingUnexpectedExits.set(canonicalId, {
    serviceId: canonicalId,
    detectedAtMs: Date.now(),
    exitCodeHint: opts.exitCode,
    reasonHint: opts.reasonHint,
  })

  if (crashFlushTimer) {
    clearTimeout(crashFlushTimer)
  }
  crashFlushTimer = setTimeout(() => {
    void flushUnexpectedExits()
  }, CRASH_GROUP_BUFFER_MS)
}

// ---------------------------------------------------------------------------
// State updates from live backend responses & commands
// ---------------------------------------------------------------------------

function updateFromStartupSettings(settings: StartupSettings) {
  const prevLoaded = startupSettingsLoaded
  const prevKey = autostartIds.join(',')
  autostartIds = [...(settings.autostart_services ?? [])]
  if (launchAutostartIds === null) {
    launchAutostartIds = new Set(autostartIds)
    saveSessionSet(SESSION_LAUNCH_AUTOSTART_KEY, launchAutostartIds)
  }
  startupSettingsLoaded = true
  if (!prevLoaded || prevKey !== autostartIds.join(',')) {
    emitStateChange()
    requestDiagnosticsRescan()
  }
}

function updateFromServicesList(services: ServiceStatus[], webData?: DashboardData['web']) {
  const now = Date.now()
  let runningSetChanged = false

  if (webData?.default_server) {
    defaultWebServerId = webData.default_server
  }
  if (webData !== undefined) {
    isWebCurrentlyRunning = webData.running
  }

  for (const s of services) {
    if (s.name) {
      serviceDisplayNames.set(s.id, s.name)
      NAME_TO_SERVICE_ID[s.name.toLowerCase()] = s.id
    }

    const wasRunning = currentlyRunningServices.has(s.id)
    const wasHealthyRunning = currentlyHealthyRunningServices.has(s.id)
    const isHealthyRunning = s.running && s.healthy !== false
    const intent = getOrCreateIntent(s.id)

    if (s.running) {
      // Race protection: if a stop was requested and we are still inside the hold window
      // (e.g. an in-flight poll started right before or during the stop), ignore this stale
      // `running: true` observation so the stop intent is NEVER cleared early!
      if (intent.expected === 'stopped' && (intent.inFlightOps > 0 || now < intent.holdUntilMs)) {
        continue
      }

      currentlyRunningServices.add(s.id)
      if (isHealthyRunning) {
        currentlyHealthyRunningServices.add(s.id)
      } else {
        currentlyHealthyRunningServices.delete(s.id)
      }

      if (isHealthyRunning) {
        // Resolve any active condition-based notifications for this service as soon as it is running & ready!
        resolveNotificationsForService(s.id)

        // If restarting and command is still in flight, wait until command finishes before locking 'running'
        if (intent.expected === 'restarting' && intent.inFlightOps > 0) {
          continue
        }
        intent.expected = 'running'
        intent.exitObserved = false
        knownRunningServices.add(s.id)
        if (!everRunningThisSession.has(s.id)) {
          everRunningThisSession.add(s.id)
          saveSessionSet(SESSION_EVER_RUNNING_KEY, everRunningThisSession)
        }
        if (explicitlyStoppedThisSession.delete(s.id)) {
          saveSessionSet(SESSION_STOPPED_KEY, explicitlyStoppedThisSession)
        }
        userStartingServices.delete(s.id)
        autostartFailedServices.delete(s.id)
        activeCrashNotifiedServices.delete(s.id)
        startFailureNotifiedServices.delete(s.id)
        pendingUnexpectedExits.delete(s.id)
      }
    } else {
      currentlyRunningServices.delete(s.id)
      currentlyHealthyRunningServices.delete(s.id)

      if (s.desired_state === 'stopped') {
        intent.expected = 'stopped'
        intent.exitObserved = true
        pendingUnexpectedExits.delete(s.id)
      } else if (intent.expected === 'stopped') {
        // Normal expected stop: record that exit has been observed, do NOT notify.
        intent.exitObserved = true
      } else if (intent.expected === 'restarting') {
        // During restart or within post-restart hold window, exit is expected.
        intent.exitObserved = true
        if (intent.inFlightOps === 0 && now >= intent.holdUntilMs) {
          // E.g. switching default web server stopped the previous server
          intent.expected = 'stopped'
          userStartingServices.delete(s.id)
          knownRunningServices.delete(s.id)
        }
      } else if (intent.expected === 'running') {
        // If still starting (not yet confirmed running), do not fire unexpected exit
        if (userStartingServices.has(s.id) && now < intent.holdUntilMs) {
          continue
        }
        if (knownRunningServices.has(s.id) && !isShuttingDown) {
          const portConflictReason =
            s.port_status === 'in_use' && s.port
              ? `Port ${s.port} is in use or the process exited.`
              : undefined
          recordUnexpectedServiceExit(s.id, { reasonHint: portConflictReason })
        }
      }
    }

    if (wasRunning !== s.running || wasHealthyRunning !== isHealthyRunning) {
      runningSetChanged = true
    }
  }

  // Check web port conflicts for auto-start web server
  if (webData && startupSettingsLoaded && !webData.running && webData.port_conflicts.length > 0) {
    const effective = getEffectiveAutostartIds()
    const targetWeb = effective.find((id) => (WEB_SERVER_IDS as readonly string[]).includes(id))
    if (targetWeb && !autostartFailedServices.has(targetWeb) && !isStopOrRestartExpected(targetWeb)) {
      recordAutostartFailure(targetWeb, webData.port_conflicts.join('\n'))
    }
  }

  if (runningSetChanged) {
    evaluateNotificationConditions()
    emitStateChange()
    requestDiagnosticsRescan()
  }
}

let checkingAutostartHealth = false

async function checkAutostartAndProcessHealth() {
  if (checkingAutostartHealth || isShuttingDown) return
  checkingAutostartHealth = true
  try {
    // Ensure we have a live services snapshot before evaluating process or autostart failures
    // so we never treat a running service as stopped before the first dashboard poll finishes.
    if (!liveDashboardLoaded) {
      const svcRes = await runCommand({ type: 'list_services' }).catch(() => null)
      if (svcRes?.type === 'services') {
        liveDashboardLoaded = true
        updateFromServicesList(svcRes.services)
      }
    }

    // 1. Inspect supervised processes for any crashed / failed state
    const procRes = await runCommand({ type: 'list_processes' }).catch(() => null)
    if (procRes?.type === 'processes') {
      handleProcessSnapshot(procRes.processes)
    }

    // 2. For any configured auto-start service not yet running, check if its start attempt failed
    if (startupSettingsLoaded && liveDashboardLoaded) {
      const effective = getEffectiveAutostartIds()
      const graceExpired = Date.now() - appLaunchTimeMs >= STARTUP_GRACE_PERIOD_MS

      for (const id of effective) {
        const intent = serviceIntents.get(id)
        const explicitlyStopped = intent?.expected === 'stopped' && intent.updatedAtMs > 0
        if (
          currentlyRunningServices.has(id) ||
          currentlyHealthyRunningServices.has(id) ||
          autostartFailedServices.has(id) ||
          explicitlyStopped
        ) {
          continue
        }

        const logRes = await runCommand({
          type: 'read_log',
          source: `service:${id}`,
          max_lines: 5,
        }).catch(() => null)

        if (logRes?.type === 'log_lines' && logRes.lines.length > 0) {
          const failedLine = logRes.lines.find((l) =>
            l.toLowerCase().startsWith('last start attempt failed:'),
          )
          if (failedLine) {
            recordAutostartFailure(id, failedLine)
            continue
          }
        }

        if (
          (graceExpired ||
            (autostartCompletedEventReceived &&
              Date.now() - appLaunchTimeMs >= STARTUP_GRACE_PERIOD_MS)) &&
          liveDashboardLoaded &&
          !currentlyRunningServices.has(id)
        ) {
          recordAutostartFailure(
            id,
            `${getServiceDisplayName(id)} did not finish starting within the startup grace period.`,
          )
        }
      }
    }
  } finally {
    checkingAutostartHealth = false
  }
}

function handleProcessSnapshot(processes: ProcessInfo[]) {
  // First identify which service IDs have a live ('running' | 'starting' | 'restarting') process
  // in the snapshot so a stale failed ProcessInfo record from an earlier or overlapping start attempt
  // never triggers a false failure while the service is actually running!
  const aliveServicesInSnapshot = new Set<string>()
  for (const p of processes) {
    const sid = resolveServiceIdFromProcessName(p.name)
    if (!sid) continue
    if (p.state === 'running' || p.state === 'starting' || p.state === 'restarting') {
      aliveServicesInSnapshot.add(sid)
      latestProcessByService.set(sid, p)
    } else if (!aliveServicesInSnapshot.has(sid)) {
      latestProcessByService.set(sid, p)
    }
  }

  for (const p of processes) {
    if (p.state !== 'crashed' && p.state !== 'failed') continue
    if (notifiedProcessIds.has(p.id)) continue
    notifiedProcessIds.add(p.id)

    const serviceId = resolveServiceIdFromProcessName(p.name)
    if (!serviceId) continue

    // If another process for this service is alive in the snapshot or the service is currently running,
    // ignore this historical failed process record completely.
    if (
      aliveServicesInSnapshot.has(serviceId) ||
      currentlyHealthyRunningServices.has(serviceId) ||
      currentlyRunningServices.has(serviceId)
    ) {
      continue
    }

    // If stop or restart was requested by user/app, ignore process exit completely
    if (isStopOrRestartExpected(serviceId)) {
      continue
    }

    // If start failed, start-failure notification handles it (do not emit "stopped unexpectedly")
    if (startFailureNotifiedServices.has(serviceId)) {
      continue
    }

    if (knownRunningServices.has(serviceId)) {
      recordUnexpectedServiceExit(serviceId, {
        exitCode: p.exit_code,
      })
    } else if (
      startupSettingsLoaded &&
      getEffectiveAutostartIds().includes(serviceId) &&
      !userStartingServices.has(serviceId)
    ) {
      const codeDetail =
        p.exit_code !== null && p.exit_code !== undefined
          ? `${p.name} failed to start (exit code ${p.exit_code}).`
          : `${p.name} failed to start.`
      recordAutostartFailure(serviceId, codeDetail)
    }
  }
}

// ---------------------------------------------------------------------------
// Command Lifecycle Hook: Set Intent From Every Stop / Restart / Start Path
// ---------------------------------------------------------------------------

function markAllWebServersRestarting() {
  for (const id of WEB_SERVER_IDS) {
    setServiceIntent(id, 'restarting', { inFlightDelta: 1, starting: false })
  }
}

function finishAllWebServersRestarting(succeeded: boolean) {
  for (const id of WEB_SERVER_IDS) {
    finishServiceCommandHold(id, succeeded)
  }
}

/**
 * Commands that re-apply web config or restart web servers as a side effect.
 */
function isWebReapplyCommand(command: CoreCommand): boolean {
  switch (command.type) {
    case 'apply_web':
    case 'restart_site_app':
    case 'write_web_config':
    case 'restore_config_history':
    case 'regenerate_certificate':
    case 'issue_lets_encrypt':
    case 'add_domain':
    case 'update_domain':
    case 'remove_domain':
    case 'delete_site':
    case 'set_domain_enabled':
    case 'bulk_set_domains_enabled':
    case 'bulk_remove_domains':
    case 'bulk_delete_sites':
    case 'bulk_set_domain_server':
    case 'rename_domain':
    case 'duplicate_domain':
    case 'relocate_project':
    case 'apply_repair':
    case 'import_sites':
    case 'import_environment':
    case 'clone_environment':
    case 'restore_snapshot':
    case 'restore_settings':
    case 'set_php_extension':
    case 'install_php_extension':
    case 'set_xdebug':
    case 'set_custom_install':
    case 'scan_php_folder':
      return true
    default:
      return false
  }
}

export function handleBeforeCommand(command: CoreCommand) {
  switch (command.type) {
    // 1. Direct service stop
    case 'stop_service': {
      setServiceIntent(command.id, 'stopped', { inFlightDelta: 1 })
      emitStateChange()
      break
    }

    // 2. Web stop
    case 'stop_web': {
      for (const id of WEB_SERVER_IDS) {
        setServiceIntent(id, 'stopped', { inFlightDelta: 1 })
      }
      isWebCurrentlyRunning = false
      emitStateChange()
      break
    }

    // 3. Stop all
    case 'stop_all': {
      const allIds = new Set<string>([
        ...Object.keys(BUILTIN_SERVICE_NAMES),
        ...serviceDisplayNames.keys(),
        ...currentlyRunningServices,
        ...knownRunningServices,
      ])
      for (const id of allIds) {
        setServiceIntent(id, 'stopped', { inFlightDelta: 1 })
      }
      isWebCurrentlyRunning = false
      emitStateChange()
      break
    }

    // 4. Process / worker / tunnel stop
    case 'stop_process': {
      for (const [sid, proc] of latestProcessByService.entries()) {
        if (proc.id === command.id) {
          setServiceIntent(sid, 'stopped', { inFlightDelta: 1 })
        }
      }
      emitStateChange()
      break
    }
    case 'stop_worker':
    case 'stop_project_workers':
    case 'stop_tunnel':
    case 'remove_worker':
    case 'remove_tunnel': {
      break
    }

    // 5. Start service
    case 'start_service': {
      // If starting a web server, other running web servers may be re-evaluated
      setServiceIntent(command.id, 'running', { inFlightDelta: 1, starting: true })
      emitStateChange()
      break
    }

    // 6. Restart service
    case 'restart_service': {
      setServiceIntent(command.id, 'restarting', { inFlightDelta: 1, starting: true })
      emitStateChange()
      break
    }

    // 7. Uninstall / delete service or runtime or project
    case 'remove_runtime':
    case 'remove_custom_service':
    case 'remove_custom_install': {
      setServiceIntent(command.id, 'stopped', { inFlightDelta: 1 })
      if ((WEB_SERVER_IDS as readonly string[]).includes(command.id) || command.id === 'php') {
        markAllWebServersRestarting()
      }
      emitStateChange()
      break
    }
    case 'remove_project':
    case 'delete_project': {
      markAllWebServersRestarting()
      emitStateChange()
      break
    }

    // 8. Switching default web server, web settings, or runtime version changes
    case 'set_setting': {
      if (command.key === 'web.default_server' && typeof command.value === 'string') {
        defaultWebServerId = command.value
        markAllWebServersRestarting()
        emitStateChange()
      } else if (command.key.startsWith('web.')) {
        markAllWebServersRestarting()
        emitStateChange()
      } else if (command.key.startsWith('runtime.')) {
        const parts = command.key.split('.')
        const runtimeId = parts[1]
        if (runtimeId) {
          setServiceIntent(runtimeId, 'restarting', { inFlightDelta: 1, starting: false })
        }
        markAllWebServersRestarting()
        emitStateChange()
      }
      break
    }

    // 9. Resource limits changes
    case 'set_resource_limits': {
      for (const id of currentlyRunningServices) {
        setServiceIntent(id, 'restarting', { inFlightDelta: 1, starting: true })
      }
      emitStateChange()
      break
    }

    default: {
      if (isWebReapplyCommand(command)) {
        markAllWebServersRestarting()
        emitStateChange()
      }
      break
    }
  }
}

export function handleCommandSuccess(command: CoreCommand, response: CoreResponse) {
  switch (command.type) {
    case 'stop_service': {
      finishServiceCommandHold(command.id, true)
      break
    }
    case 'stop_web': {
      for (const id of WEB_SERVER_IDS) {
        finishServiceCommandHold(id, true)
      }
      break
    }
    case 'stop_all': {
      for (const id of serviceIntents.keys()) {
        finishServiceCommandHold(id, true)
      }
      break
    }
    case 'start_service':
    case 'restart_service': {
      finishServiceCommandHold(command.id, true)
      const rec = getOrCreateIntent(command.id)
      rec.expected = 'running'
      rec.exitObserved = false
      userStartingServices.delete(command.id)
      currentlyRunningServices.add(command.id)
      currentlyHealthyRunningServices.add(command.id)
      knownRunningServices.add(command.id)
      activeCrashNotifiedServices.delete(command.id)
      startFailureNotifiedServices.delete(command.id)
      autostartFailedServices.delete(command.id)
      pendingUnexpectedExits.delete(command.id)
      resolveNotificationsForService(command.id)
      evaluateNotificationConditions()
      emitStateChange()
      break
    }
    case 'remove_runtime':
    case 'remove_custom_service':
    case 'remove_custom_install': {
      finishServiceCommandHold(command.id, true)
      if ((WEB_SERVER_IDS as readonly string[]).includes(command.id) || command.id === 'php') {
        finishAllWebServersRestarting(true)
      }
      break
    }
    case 'remove_project':
    case 'delete_project': {
      finishAllWebServersRestarting(true)
      break
    }
    case 'set_setting': {
      if (command.key.startsWith('web.')) {
        finishAllWebServersRestarting(true)
      } else if (command.key.startsWith('runtime.')) {
        const runtimeId = command.key.split('.')[1]
        if (runtimeId) finishServiceCommandHold(runtimeId, true)
        finishAllWebServersRestarting(true)
      }
      break
    }
    case 'set_resource_limits': {
      for (const id of serviceIntents.keys()) {
        finishServiceCommandHold(id, true)
      }
      break
    }
    default: {
      if (isWebReapplyCommand(command)) {
        finishAllWebServersRestarting(true)
        if (command.type === 'apply_web' && response.type === 'applied') {
          for (const rep of response.reports) {
            if (rep.started || rep.reloaded) {
              const rec = getOrCreateIntent(rep.server)
              rec.expected = 'running'
              rec.exitObserved = false
              userStartingServices.delete(rep.server)
              currentlyRunningServices.add(rep.server)
              currentlyHealthyRunningServices.add(rep.server)
              knownRunningServices.add(rep.server)
              activeCrashNotifiedServices.delete(rep.server)
              startFailureNotifiedServices.delete(rep.server)
              autostartFailedServices.delete(rep.server)
              pendingUnexpectedExits.delete(rep.server)
              resolveNotificationsForService(rep.server)
            }
          }
          for (const id of WEB_SERVER_IDS) {
            userStartingServices.delete(id)
          }
          isWebCurrentlyRunning = true
          evaluateNotificationConditions()
          emitStateChange()
        }
      }
      break
    }
  }

  if (response.type === 'startup') {
    updateFromStartupSettings(response.settings)
    void checkAutostartAndProcessHealth()
  } else if (response.type === 'dashboard') {
    liveDashboardLoaded = true
    updateFromServicesList(response.data.services, response.data.web)
    void checkAutostartAndProcessHealth()
  } else if (response.type === 'services') {
    updateFromServicesList(response.services)
  } else if (response.type === 'processes') {
    handleProcessSnapshot(response.processes)
  }
}

export function handleCommandError(command: CoreCommand, _error: Diagnostic) {
  switch (command.type) {
    case 'stop_service': {
      finishServiceCommandHold(command.id, false)
      break
    }
    case 'stop_web': {
      for (const id of WEB_SERVER_IDS) {
        finishServiceCommandHold(id, false)
      }
      break
    }
    case 'stop_all': {
      for (const id of serviceIntents.keys()) {
        finishServiceCommandHold(id, false)
      }
      break
    }
    case 'start_service':
    case 'restart_service': {
      finishServiceCommandHold(command.id, false)
      const rec = getOrCreateIntent(command.id)
      rec.expected = 'stopped'
      rec.holdUntilMs = Date.now() + INTENT_HOLD_TIMEOUT_MS
      userStartingServices.delete(command.id)
      knownRunningServices.delete(command.id)
      activeCrashNotifiedServices.add(command.id)
      startFailureNotifiedServices.add(command.id)
      pendingUnexpectedExits.delete(command.id)
      emitStateChange()
      break
    }
    case 'remove_runtime':
    case 'remove_custom_service':
    case 'remove_custom_install': {
      finishServiceCommandHold(command.id, false)
      if ((WEB_SERVER_IDS as readonly string[]).includes(command.id) || command.id === 'php') {
        finishAllWebServersRestarting(false)
      }
      break
    }
    case 'remove_project':
    case 'delete_project': {
      finishAllWebServersRestarting(false)
      break
    }
    case 'set_setting': {
      if (command.key.startsWith('web.')) {
        finishAllWebServersRestarting(false)
      } else if (command.key.startsWith('runtime.')) {
        const runtimeId = command.key.split('.')[1]
        if (runtimeId) finishServiceCommandHold(runtimeId, false)
        finishAllWebServersRestarting(false)
      }
      break
    }
    case 'set_resource_limits': {
      for (const id of serviceIntents.keys()) {
        finishServiceCommandHold(id, false)
      }
      break
    }
    default: {
      if (isWebReapplyCommand(command)) {
        finishAllWebServersRestarting(false)
        for (const id of WEB_SERVER_IDS) {
          userStartingServices.delete(id)
        }
        emitStateChange()
      }
      break
    }
  }
}

// ---------------------------------------------------------------------------
// Diagnostics Bar Formatting & Notification Evaluation (Rules 1, 2, 3, 4, 5, 6, 7)
// ---------------------------------------------------------------------------

/**
 * Rule 4: Returns true if a finding is purely a stopped-service state
 * ("A service that was not started this session, or that I stopped myself: state only.
 * It is shown in the Diagnostics bar and the Services card, with NO toast, NO history entry
 * and NO unread badge.")
 */
function isStateOnlyStoppedFinding(findingId: string): boolean {
  if (findingId === 'web_stopped' || findingId === 'health:web_running') {
    return true
  }
  if (findingId.startsWith('project_service_stopped:')) {
    return true
  }
  // When the web server is stopped, hosts entries are not yet applied; that depends on web_stopped.
  if ((findingId === 'hosts_missing' || findingId === 'health:hosts') && !isWebCurrentlyRunning) {
    return true
  }
  return false
}

/**
 * Rule 2: Returns true if a finding depends on a service that is currently in `"starting"` state.
 */
function dependsOnStartingService(findingId: string, startingIds: Set<string>): boolean {
  if (startingIds.size === 0) return false
  if (
    (findingId === 'web_stopped' ||
      findingId === 'health:web_running' ||
      findingId === 'hosts_missing' ||
      findingId === 'health:hosts') &&
    (startingIds.has('web') || WEB_SERVER_IDS.some((id) => startingIds.has(id)))
  ) {
    return true
  }
  if (findingId.startsWith('project_service_stopped:')) {
    const parts = findingId.split(':')
    const svcId = parts[2]
    if (svcId && startingIds.has(svcId)) return true
  }
  if (findingId.startsWith('service_unhealthy:')) {
    const svcId = findingId.slice('service_unhealthy:'.length)
    if (startingIds.has(svcId)) return true
  }
  if (findingId.startsWith('health:service:')) {
    const svcId = findingId.slice('health:service:'.length)
    if (startingIds.has(svcId)) return true
  }
  return false
}

/**
 * Returns true if the finding is for a service whose crash/start failure was already notified as an event.
 */
function isAlreadyNotifiedServiceEvent(findingId: string): boolean {
  if (findingId.startsWith('service_unhealthy:')) {
    const svcId = findingId.slice('service_unhealthy:'.length)
    return (
      activeCrashNotifiedServices.has(svcId) ||
      autostartFailedServices.has(svcId) ||
      startFailureNotifiedServices.has(svcId)
    )
  }
  if (findingId.startsWith('health:service:')) {
    const svcId = findingId.slice('health:service:'.length)
    return (
      activeCrashNotifiedServices.has(svcId) ||
      autostartFailedServices.has(svcId) ||
      startFailureNotifiedServices.has(svcId)
    )
  }
  return false
}

/**
 * Rule 5: Formats findings for immediate display in the Diagnostics bar, deduplicating
 * overlapping `health:*` items and using `"Starting…"` wording for services still starting.
 */
export function buildDiagnosticsBarFindings(
  findings: Finding[],
  healthProblems: HealthItem[],
  startingIds: Set<string>,
): Finding[] {
  // Security issues appear only in the Security section, not in Diagnostics.
  const diagnosticsFindings = findings.filter((f) => !f.id.startsWith('eol:'))
  const findingIds = new Set(diagnosticsFindings.map((f) => f.id))

  const syntheticHealthFindings: Finding[] = healthProblems
    .filter((h) => h.status !== 'ok')
    .filter((h) => {
      if (h.id === 'web_running' && findingIds.has('web_stopped')) return false
      if (h.id === 'ca' && findingIds.has('ca_not_trusted')) return false
      if (h.id === 'hosts' && findingIds.has('hosts_missing')) return false
      if (h.id.startsWith('service:')) {
        const svcId = h.id.slice('service:'.length)
        if (findingIds.has(`service_unhealthy:${svcId}`)) return false
      }
      return true
    })
    .map((h) => ({
      id: `health:${h.id}`,
      problem: h.label,
      cause: h.detail,
      fix: h.fix || 'Check service status in the Services panel.',
      severity: (h.status === 'warn' ? 'warning' : 'error') as 'warning' | 'error',
      details: [],
      ignored: false,
      auto_fixable: false,
      fix_command: null,
    }))

  const combined = [...diagnosticsFindings, ...syntheticHealthFindings]

  return combined.map((f) => {
    if (f.ignored) return f

    if (
      (f.id === 'web_stopped' || f.id === 'health:web_running') &&
      (startingIds.has('web') || WEB_SERVER_IDS.some((id) => startingIds.has(id)))
    ) {
      return {
        ...f,
        problem: 'Starting web server…',
        severity: 'warning',
      }
    }

    if (f.id.startsWith('project_service_stopped:')) {
      const svcId = f.id.split(':')[2]
      if (svcId && startingIds.has(svcId)) {
        return {
          ...f,
          problem: `Starting ${getServiceDisplayName(svcId)}…`,
          severity: 'warning',
        }
      }
    }

    if (f.id.startsWith('service_unhealthy:') || f.id.startsWith('health:service:')) {
      const svcId = f.id.startsWith('service_unhealthy:')
        ? f.id.slice('service_unhealthy:'.length)
        : f.id.slice('health:service:'.length)
      if (startingIds.has(svcId)) {
        return {
          ...f,
          problem: `Starting ${getServiceDisplayName(svcId)}…`,
          severity: 'warning',
        }
      }
    }

    return f
  })
}

/**
 * Evaluates a diagnostics scan result against Rules 1–7.
 * Never notifies on the first baseline check or during the startup grace period,
 * ignores state-only stopped-service findings, and requires 2 consecutive checks
 * at least ~10 seconds apart after the grace period before creating a notification.
 */
export function evaluateDiagnosticsScan(findings: Finding[], healthProblems: HealthItem[]): void {
  const now = Date.now()
  const startingIds = getStartingServiceIds()
  const inGracePeriod = isInStartupGracePeriod()

  const barFindings = buildDiagnosticsBarFindings(findings, healthProblems, startingIds)
  const activeBarFindings = barFindings.filter((f) => !f.ignored)
  const currentActiveIds = new Set(activeBarFindings.map((f) => f.id))

  // Rule 1 & Rule 2: Silent baseline on first check(s) and throughout the startup grace period.
  if (!hasCompletedBaseline || inGracePeriod) {
    for (const f of activeBarFindings) {
      if (!dependsOnStartingService(f.id, startingIds)) {
        baselineIssueIds.add(f.id)
        unresolvedIssueIds.add(f.id)
      }
    }
    saveUnresolvedIssues()

    // Lock in baseline completion once both initial startup settings and live dashboard have loaded
    if (startupSettingsLoaded && liveDashboardLoaded) {
      hasCompletedBaseline = true
    }
    return
  }

  // Clear resolved issues from baseline / unresolved sets and from pending candidates (Rule 3 & Rule 6)
  // Also auto-resolve any condition-based diagnostics notifications whose issue cleared (Fix 2)
  let unresolvedChanged = false
  for (const id of [...unresolvedIssueIds]) {
    if (!currentActiveIds.has(id)) {
      unresolvedIssueIds.delete(id)
      resolveNotificationByKey(`diagnostic:${id}`)
      unresolvedChanged = true
    }
  }
  if (unresolvedChanged) {
    saveUnresolvedIssues()
  }

  for (const id of [...baselineIssueIds]) {
    if (!currentActiveIds.has(id)) {
      baselineIssueIds.delete(id)
      resolveNotificationByKey(`diagnostic:${id}`)
    }
  }

  for (const id of [...pendingCandidates.keys()]) {
    if (!currentActiveIds.has(id)) {
      pendingCandidates.delete(id)
    }
  }

  // Filter findings eligible to create a diagnostics notification
  const eligibleFindings = activeBarFindings.filter((f) => {
    if (isStateOnlyStoppedFinding(f.id)) return false
    if (dependsOnStartingService(f.id, startingIds)) return false
    if (isAlreadyNotifiedServiceEvent(f.id)) return false
    if (baselineIssueIds.has(f.id)) return false
    if (unresolvedIssueIds.has(f.id)) return false
    return true
  })

  const confirmedToNotify: Finding[] = []

  for (const f of eligibleFindings) {
    const existing = pendingCandidates.get(f.id)
    if (!existing) {
      // First observation after grace period: record candidate, do not notify yet (Rule 3)
      pendingCandidates.set(f.id, {
        firstSeenMs: now,
        lastSeenMs: now,
        checkCount: 1,
        finding: f,
      })
    } else {
      existing.checkCount += 1
      existing.lastSeenMs = now
      existing.finding = f

      if (
        existing.checkCount >= PERSISTENCE_CONSECUTIVE_CHECKS &&
        now - existing.firstSeenMs >= PERSISTENCE_MIN_INTERVAL_MS
      ) {
        pendingCandidates.delete(f.id)
        unresolvedIssueIds.add(f.id)
        confirmedToNotify.push(f)
      }
    }
  }

  if (confirmedToNotify.length > 0) {
    saveUnresolvedIssues()
    const issueSummary = confirmedToNotify
      .slice(0, 3)
      .map((f) => f.problem)
      .join('\n')
    const count = confirmedToNotify.length
    const diagKeys = confirmedToNotify.map((f) => `diagnostic:${f.id}`)
    const primaryKey =
      diagKeys.length === 1 ? diagKeys[0] : `diagnostic:${confirmedToNotify.map((f) => f.id).sort().join(',')}`
    notify.recordHistory(
      'warning',
      `Diagnostics: ${count} ${count === 1 ? 'issue requires' : 'issues require'} attention.`,
      {
        id: primaryKey,
        conditionKey: primaryKey,
        conditionKeys: diagKeys,
        title: 'Diagnostics',
        details: issueSummary || undefined,
        logSource: 'diagnostics',
        read: false,
        needsAttention: true,
      },
    )
  }
}

// ---------------------------------------------------------------------------
// Global Initialization
// ---------------------------------------------------------------------------

let monitorInitialized = false

setCommandLifecycleHook({
  onBeforeCommand: handleBeforeCommand,
  onCommandSuccess: handleCommandSuccess,
  onCommandError: handleCommandError,
  onBeforeExit: markAppShuttingDown,
})

export function initDiagnosticsNotificationMonitor() {
  if (monitorInitialized || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
    return
  }
  monitorInitialized = true

  setCommandLifecycleHook({
    onBeforeCommand: handleBeforeCommand,
    onCommandSuccess: handleCommandSuccess,
    onCommandError: handleCommandError,
    onBeforeExit: markAppShuttingDown,
  })

  window.addEventListener('beforeunload', () => {
    markAppShuttingDown()
  })

  // Eagerly load startup settings and initial service state so grace period is accurate immediately
  void runCommand({ type: 'get_startup_settings' }).catch(() => null)
  void runCommand({ type: 'get_dashboard' }).catch(() => null)

  // Periodically check process health & autostart status even when not on Dashboard
  setInterval(() => {
    void checkAutostartAndProcessHealth()
  }, 3000)

  {
    void listen('ols:exiting', () => {
      markAppShuttingDown()
    })

    void listen('ols:autostart-complete', () => {
      autostartCompletedEventReceived = true
      try {
        sessionStorage.setItem(SESSION_AUTOSTART_DONE_KEY, '1')
      } catch {
        // ignore
      }
      void runCommand({ type: 'get_dashboard' })
        .then(() => checkAutostartAndProcessHealth())
        .finally(() => {
          emitStateChange()
          requestDiagnosticsRescan()
        })
    })

    // Safe problems fixed automatically (the desktop toast may be silenced by Windows).
    void listen<{ ok: boolean; problem: string; detail: string }>('ols:auto-fix-result', (event) => {
      const r = event.payload
      if (r.ok) notify.success(`Fixed: ${r.problem}`)
      else notify.error(`Automatic fix failed: ${r.problem}`, { details: r.detail })
      requestDiagnosticsRescan()
    })

    void listen('service-status-event', () => {
      requestDiagnosticsRescan()
    })

    void listen('ols:stop-all-requested', () => {
      handleExpectedStopAll()
    })

    void listen<string>('ols:service-stop-requested', (event) => {
      handleExpectedServiceStop(event.payload)
    })

    void listen<{ kind: string; id: number; state?: string; name?: string }>(
      'process-event',
      (event) => {
        const p = event.payload
        if (p.kind === 'stop_requested' && p.name) {
          const serviceId = resolveServiceIdFromProcessName(p.name)
          if (serviceId) handleExpectedServiceStop(serviceId)
        }
        if (
          p.kind === 'state_changed' &&
          (p.state === 'crashed' || p.state === 'failed' || p.state === 'stopped')
        ) {
          void checkAutostartAndProcessHealth()
        }
      },
    )
  }
}
