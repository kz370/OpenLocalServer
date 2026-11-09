import { AlertCircle, AlertTriangle, BrushCleaning, Check, CheckCircle2, Copy, FileText, Info, RotateCcw, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { Spinner } from '@/components/Spinner'
import { Button } from '@/components/ui/button'
import {
  type NotificationItem,
  dismissAllNotifications,
  dismissNewestNotification,
  dismissNotification,
  initGlobalNotificationListeners,
  subscribeNotifications,
} from '@/lib/notify'
import { cn } from '@/lib/utils'

export function NotificationHost({ onNavigateLogs }: { onNavigateLogs?: (source?: string | null) => void }) {
  const [items, setItems] = useState<NotificationItem[]>([])

  useEffect(() => {
    initGlobalNotificationListeners()
    const unsubscribe = subscribeNotifications((next) => setItems(next))
    return unsubscribe
  }, [])

  // Esc dismisses the newest toast
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        dismissNewestNotification()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  // Stack at most 4 visible; older ones queue
  const visible = items.slice(0, 4)
  // Progress toasts track running tasks, so Dismiss all leaves them alone.
  const dismissibleCount = items.filter((item) => item.type !== 'progress').length

  if (visible.length === 0) return null

  return (
    <div
      aria-label="Notifications"
      // Positioned by the corner stack in OverlayHosts, which also holds the overlay ErrorCards.
      className="flex max-w-sm w-full flex-col-reverse gap-2.5 pointer-events-none"
    >
      {/* First child of a column-reverse stack, so it sits at the bottom, below the toasts. */}
      {dismissibleCount >= 2 && (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => dismissAllNotifications()}
            aria-label={`Dismiss all ${dismissibleCount} notifications`}
            title="Dismiss all notifications"
            // Card pill tinted with theme primary, foreground text, primary border and outer glow
            // that strengthens on hover/focus.
            className="pointer-events-auto inline-flex cursor-pointer items-center gap-2 rounded-full border border-primary/80 bg-[color-mix(in_srgb,var(--color-primary)_22%,var(--color-card))] px-4 py-1.5 text-xs font-semibold text-foreground shadow-[0_0_10px_-2px_var(--color-primary)] backdrop-blur-xl transition-all duration-200 hover:border-primary hover:bg-[color-mix(in_srgb,var(--color-primary)_32%,var(--color-card))] hover:shadow-[0_0_16px_0_var(--color-primary)] focus-visible:outline-none focus-visible:shadow-[0_0_16px_0_var(--color-primary)]"
          >
            <BrushCleaning className="size-3.5 text-primary" /> Dismiss all ({dismissibleCount})
          </button>
        </div>
      )}
      {visible.map((item) => (
        <ToastItem key={item.id} item={item} onNavigateLogs={onNavigateLogs} />
      ))}
    </div>
  )
}

function ToastItem({
  item,
  onNavigateLogs,
}: {
  item: NotificationItem
  onNavigateLogs?: (source?: string | null) => void
}) {
  const [isHovered, setIsHovered] = useState(false)
  const [isFocused, setIsFocused] = useState(false)
  const [copied, setCopied] = useState(false)
  const [showDetails, setShowDetails] = useState(false)
  const [windowFocused, setWindowFocused] = useState(true)

  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const remainingRef = useRef<number | null>(item.duration ?? null)
  const startTimeRef = useRef<number>(0)

  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  const startTimer = useCallback(
    (ms: number) => {
      clearTimer()
      startTimeRef.current = Date.now()
      remainingRef.current = ms
      timerRef.current = setTimeout(() => {
        dismissNotification(item.id)
      }, ms)
    },
    [clearTimer, item.id],
  )

  // Track window focus: pause when unfocused, resume with fresh 2000ms delay when window focused
  useEffect(() => {
    const handleBlur = () => {
      setWindowFocused(false)
      setIsHovered(false)
      clearTimer()
    }
    // Toasts sit at the window edge: leaving the window across one can skip its mouseleave,
    // which left the toast paused (hovered) forever. Clear hover when the pointer leaves.
    const handleWindowOut = (e: MouseEvent) => {
      if (!e.relatedTarget) setIsHovered(false)
    }
    const handleFocus = () => {
      setWindowFocused(true)
      if (item.duration !== undefined) {
        // Give user at least 2000ms to read toasts when returning to the window
        startTimer(Math.max(2000, remainingRef.current ?? item.duration))
      }
    }

    window.addEventListener('blur', handleBlur)
    window.addEventListener('focus', handleFocus)
    document.addEventListener('mouseout', handleWindowOut)
    return () => {
      window.removeEventListener('blur', handleBlur)
      window.removeEventListener('focus', handleFocus)
      document.removeEventListener('mouseout', handleWindowOut)
    }
  }, [clearTimer, item.duration, startTimer])

  const prevCountRef = useRef(item.count)
  useEffect(() => {
    if (item.count !== prevCountRef.current) {
      prevCountRef.current = item.count
      if (item.duration !== undefined) {
        remainingRef.current = item.duration
        startTimer(item.duration)
      }
    }
  }, [item.count, item.duration, startTimer])

  // Timer logic with hover & focus pauses
  useEffect(() => {
    if (item.duration === undefined) return // Persistent

    const isPaused = isHovered || isFocused || !windowFocused

    if (isPaused) {
      if (timerRef.current) {
        const elapsed = Date.now() - startTimeRef.current
        remainingRef.current = Math.max(0, (remainingRef.current ?? item.duration) - elapsed)
        clearTimer()
      }
    } else {
      const ms = remainingRef.current ?? item.duration
      if (ms > 0) {
        startTimer(ms)
      } else {
        dismissNotification(item.id)
      }
    }

    return clearTimer
  }, [clearTimer, isFocused, isHovered, item.duration, item.id, startTimer, windowFocused])

  const hasExtraDetails = Boolean(
    item.details &&
      item.details.trim().length > 0 &&
      item.details.trim() !== item.message.trim(),
  )

  const copyDetails = async () => {
    if (!item.details && !item.message && !item.title) return
    const header = item.title ? `${item.title}\n${item.message}` : item.message
    const text = hasExtraDetails ? `${header}\n\nDetails:\n${item.details}` : header
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // ignore
    }
  }

  const isAlert = item.type === 'error' || item.type === 'warning'
  const isProgress = item.type === 'progress'

  return (
    <div
      role={isAlert ? 'alert' : 'status'}
      aria-live={item.type === 'error' ? 'assertive' : 'polite'}
      tabIndex={0}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      // Only keyboard focus pauses: a mouse click (Copy, Details) leaves focus parked on the
      // toast, which used to pause it until the user happened to click somewhere else.
      onFocus={(e) => setIsFocused(e.target.matches(':focus-visible'))}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setIsFocused(false)
      }}
      className={cn(
        'pointer-events-auto relative flex flex-col gap-2 rounded-xl border p-3.5 shadow-xl transition-all duration-200 outline-none',
        'bg-card/95 backdrop-blur-xl text-card-foreground',
        item.type === 'success' && 'border-success/40 shadow-success/5',
        item.type === 'info' && 'border-info/40 shadow-info/5',
        item.type === 'warning' && 'border-warning/40 shadow-warning/5',
        item.type === 'error' && 'border-destructive/40 shadow-destructive/5',
        item.type === 'progress' && 'border-primary/40 shadow-primary/5',
        item.closing
          ? 'opacity-0 translate-y-1 scale-95 pointer-events-none'
          : 'animate-in fade-in slide-in-from-bottom-2 motion-reduce:animate-none',
      )}
    >
      <div className="flex items-start gap-2.5">
        <span className="mt-0.5 shrink-0">
          {item.type === 'success' && <CheckCircle2 className="size-4 text-success" />}
          {item.type === 'info' && <Info className="size-4 text-info" />}
          {item.type === 'warning' && <AlertTriangle className="size-4 text-warning" />}
          {item.type === 'error' && <AlertCircle className="size-4 text-destructive" />}
          {item.type === 'progress' && <Spinner className="size-4 text-primary" />}
        </span>

        <div className="flex flex-1 flex-col gap-1 min-w-0 pr-5">
          <div className="flex items-center gap-1.5 flex-wrap">
            {item.title && <span className="font-semibold text-xs text-foreground">{item.title}</span>}
            {item.count > 1 && (
              <span className="inline-flex items-center px-1.5 py-0.2 rounded-full text-[10px] font-bold bg-muted text-muted-foreground border border-border">
                x{item.count}
              </span>
            )}
          </div>

          <p className="text-xs leading-relaxed text-foreground/90 font-medium break-words">
            {item.message}
          </p>

          {isProgress && (
            <div className="mt-1 flex flex-col gap-1 w-full">
              {item.statusText && (
                <span className="text-[11px] text-muted-foreground truncate">{item.statusText}</span>
              )}
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                {item.progress !== null && item.progress !== undefined ? (
                  <div
                    className="h-full bg-primary transition-all duration-200"
                    style={{ width: `${Math.min(100, Math.max(0, item.progress))}%` }}
                  />
                ) : (
                  <div className="h-full w-1/3 bg-primary animate-[indeterminate_1.5s_infinite_linear] rounded-full" />
                )}
              </div>
            </div>
          )}

          {/* Error actions: Copy details, View logs, Details toggle */}
          {item.type === 'error' && (
            <div className="mt-1 flex items-center gap-2 flex-wrap text-xs">
              {hasExtraDetails && (
                <button
                  type="button"
                  onClick={() => setShowDetails((prev) => !prev)}
                  className="cursor-pointer text-[11px] font-medium text-muted-foreground hover:text-foreground underline underline-offset-2"
                >
                  {showDetails ? 'Hide details' : 'Show details'}
                </button>
              )}
              <button
                type="button"
                onClick={copyDetails}
                className="cursor-pointer inline-flex items-center gap-1 text-[11px] font-medium text-muted-foreground hover:text-foreground"
              >
                {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3" />}
                {copied ? 'Copied' : 'Copy details'}
              </button>
              {onNavigateLogs && (
                <button
                  type="button"
                  onClick={() => onNavigateLogs(item.logSource)}
                  className="cursor-pointer inline-flex items-center gap-1 text-[11px] font-medium text-info hover:underline underline-offset-2"
                >
                  <FileText className="size-3" /> View logs
                </button>
              )}
            </div>
          )}

          {showDetails && hasExtraDetails && (
            <pre className="mt-1.5 max-h-32 overflow-y-auto rounded-md bg-muted/60 p-2 text-[10px] text-muted-foreground font-mono whitespace-pre-wrap break-all border border-border/50">
              {item.details}
            </pre>
          )}

          {/* Action button (e.g. Undo, Retry, View details) */}
          {item.action && (
            <div className="mt-1.5 flex items-center gap-2">
              <Button
                size="sm"
                variant={item.action.primary ? 'default' : 'secondary'}
                onClick={() => {
                  void item.action?.onClick()
                  dismissNotification(item.id)
                }}
                className={cn(
                  'h-6 px-2.5 text-[11px] font-medium',
                  item.action.isUndo && 'gap-1 bg-warning/20 hover:bg-warning/30 text-warning-foreground border border-warning/40',
                )}
              >
                {item.action.isUndo && <RotateCcw className="size-3" />}
                {item.action.label}
              </Button>
            </div>
          )}
        </div>

        <button
          type="button"
          aria-label="Dismiss notification"
          onClick={() => dismissNotification(item.id)}
          className="absolute top-2.5 right-2.5 size-5 flex items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-muted/80 cursor-pointer transition-colors"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </div>
  )
}

