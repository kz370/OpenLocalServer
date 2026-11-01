import {
  AlertCircle,
  AlertTriangle,
  Bell,
  Check,
  CheckCircle2,
  Copy,
  FileText,
  Info,
  RotateCcw,
  Trash2,
  X,
} from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { timeAgo } from '@/lib/hooks'
import {
  type NotificationItem,
  clearNotificationHistory,
  markAllNotificationsRead,
  subscribeNotificationHistory,
} from '@/lib/notify'
import { cn } from '@/lib/utils'

export function NotificationsPopover({ onNavigateLogs }: { onNavigateLogs?: (source?: string | null) => void }) {
  const [open, setOpen] = useState(false)
  const [history, setHistory] = useState<NotificationItem[]>([])
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const popoverRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const unsub = subscribeNotificationHistory((items) => setHistory(items))
    return unsub
  }, [])

  // Close on outside click
  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (
        popoverRef.current &&
        !popoverRef.current.contains(e.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(e.target as Node)
      ) {
        setOpen(false)
      }
    }
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }

    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [open])

  const toggle = () => {
    const next = !open
    setOpen(next)
    if (next) {
      markAllNotificationsRead()
    }
  }

  const unreadCount = history.filter((h) => !h.read).length

  const handleCopy = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopiedId(id)
      setTimeout(() => setCopiedId(null), 2000)
    } catch {
      // ignore
    }
  }

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        type="button"
        onClick={toggle}
        title="Notifications"
        aria-label={`Notifications${unreadCount > 0 ? ` (${unreadCount} unread)` : ''}`}
        aria-expanded={open}
        className={cn(
          'relative flex size-7 cursor-pointer items-center justify-center rounded-md transition-colors',
          'text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
          open && 'bg-sidebar-accent text-sidebar-accent-foreground',
        )}
      >
        <Bell className="size-3.5" />
        {unreadCount > 0 && (
          <span className="absolute -top-0.5 -right-0.5 flex size-3.5 items-center justify-center rounded-full bg-destructive text-[9px] font-bold text-destructive-foreground ring-2 ring-sidebar">
            {unreadCount > 9 ? '9+' : unreadCount}
          </span>
        )}
      </button>

      {open && (
        <div
          ref={popoverRef}
          role="region"
          aria-label="Recent notifications"
          className={cn(
            'absolute right-0 top-full mt-1.5 w-84 sm:w-96 rounded-xl border border-border/80 shadow-2xl z-[150]',
            'bg-popover/95 backdrop-blur-xl text-popover-foreground flex flex-col max-h-[28rem] overflow-hidden',
            'animate-in fade-in zoom-in-95 duration-150',
          )}
        >
          {/* Header */}
          <div className="flex items-center justify-between border-b border-border/60 px-4 py-2.5 bg-muted/40">
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-foreground">Notifications</span>
              {history.length > 0 && (
                <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] font-medium text-muted-foreground border border-border/50">
                  {history.length}
                </span>
              )}
            </div>
            <div className="flex items-center gap-1">
              {history.length > 0 && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={clearNotificationHistory}
                  title="Clear all notifications"
                  className="h-6 px-2 text-[11px] text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="size-3 mr-1" /> Clear all
                </Button>
              )}
              <button
                type="button"
                onClick={() => setOpen(false)}
                aria-label="Close"
                className="flex size-6 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            </div>
          </div>

          {/* List */}
          <div className="flex-1 overflow-y-auto p-2 divide-y divide-border/40">
            {history.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-10 text-center text-muted-foreground">
                <Bell className="size-8 stroke-[1.2] text-muted-foreground/40 mb-2" />
                <p className="text-xs font-medium">No recent notifications</p>
                <p className="text-[11px] text-muted-foreground/70 mt-0.5">Alerts, errors, and task finishes will appear here.</p>
              </div>
            ) : (
              history.map((item) => {
                const fullText = item.details ? `${item.message}\n\n${item.details}` : item.message
                const isCopied = copiedId === item.id

                return (
                  <div
                    key={item.id}
                    className="flex items-start gap-2.5 p-2.5 rounded-lg hover:bg-muted/30 transition-colors text-xs"
                  >
                    <span className="mt-0.5 shrink-0">
                      {item.type === 'success' && <CheckCircle2 className="size-4 text-success" />}
                      {item.type === 'info' && <Info className="size-4 text-info" />}
                      {item.type === 'warning' && <AlertTriangle className="size-4 text-warning" />}
                      {item.type === 'error' && <AlertCircle className="size-4 text-destructive" />}
                    </span>

                    <div className="flex flex-1 flex-col gap-1 min-w-0">
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-medium text-foreground truncate">
                          {item.title || (item.type.charAt(0).toUpperCase() + item.type.slice(1))}
                        </span>
                        <span className="text-[10px] text-muted-foreground shrink-0">
                          {timeAgo(item.createdAt)}
                        </span>
                      </div>

                      <p className="text-muted-foreground leading-snug break-words">
                        {item.message}
                      </p>

                      {item.details && (
                        <pre className="mt-1 max-h-24 overflow-y-auto rounded bg-muted/70 p-1.5 text-[10px] font-mono text-muted-foreground whitespace-pre-wrap break-all border border-border/40">
                          {item.details}
                        </pre>
                      )}

                      <div className="mt-1 flex items-center gap-2 flex-wrap text-[11px]">
                        <button
                          type="button"
                          onClick={() => void handleCopy(item.id, fullText)}
                          className="cursor-pointer inline-flex items-center gap-1 text-muted-foreground hover:text-foreground"
                        >
                          {isCopied ? <Check className="size-2.5 text-success" /> : <Copy className="size-2.5" />}
                          {isCopied ? 'Copied' : 'Copy'}
                        </button>
                        {item.type === 'error' && onNavigateLogs && (
                          <button
                            type="button"
                            onClick={() => {
                              setOpen(false)
                              onNavigateLogs(item.logSource)
                            }}
                            className="cursor-pointer inline-flex items-center gap-1 text-info hover:underline"
                          >
                            <FileText className="size-2.5" /> View logs
                          </button>
                        )}
                        {item.action && (
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => {
                              void item.action?.onClick()
                              setOpen(false)
                            }}
                            className="h-5 px-2 text-[10px] font-medium"
                          >
                            {item.action.isUndo && <RotateCcw className="size-2.5 mr-1" />}
                            {item.action.label}
                          </Button>
                        )}
                      </div>
                    </div>
                  </div>
                )
              })
            )}
          </div>
        </div>
      )}
    </div>
  )
}

