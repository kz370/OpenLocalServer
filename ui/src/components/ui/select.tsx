import {
  autoUpdate,
  flip,
  FloatingPortal,
  offset,
  shift,
  size,
  useDismiss,
  useFloating,
  useInteractions,
} from '@floating-ui/react'
import { Check, ChevronDown, Search, X } from 'lucide-react'
import * as React from 'react'

import { cn } from '@/lib/utils'

export interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'size' | 'defaultValue'> {
  className?: string
  children?: React.ReactNode
  triggerClassName?: string
  placeholder?: string
  defaultValue?: string | number
  searchable?: boolean
  searchPlaceholder?: string
}

interface ParsedOption {
  type: 'option'
  value: string
  label: string
  rawChildren?: React.ReactNode
  disabled?: boolean
  group?: string
}

interface ParsedGroup {
  type: 'group'
  label: string
}

type ParsedItem = ParsedOption | ParsedGroup

function extractText(node: React.ReactNode): string {
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(extractText).join('')
  if (React.isValidElement(node)) {
    return extractText((node.props as { children?: React.ReactNode }).children)
  }
  return ''
}

function parseSelectChildren(children: React.ReactNode, groupName?: string): ParsedItem[] {
  const items: ParsedItem[] = []

  React.Children.forEach(children, (child) => {
    if (!React.isValidElement(child)) return

    const childProps = child.props as {
      label?: string
      value?: string | number
      disabled?: boolean
      children?: React.ReactNode
    }

    if (child.type === React.Fragment) {
      items.push(...parseSelectChildren(childProps.children, groupName))
      return
    }

    if (child.type === 'optgroup' || (typeof child.type === 'string' && child.type.toLowerCase() === 'optgroup')) {
      const label = childProps.label || ''
      items.push({ type: 'group', label })
      items.push(...parseSelectChildren(childProps.children, label))
      return
    }

    if (child.type === 'option' || (typeof child.type === 'string' && child.type.toLowerCase() === 'option')) {
      const value = childProps.value !== undefined ? String(childProps.value) : extractText(childProps.children)
      const label = extractText(childProps.children) || value
      items.push({
        type: 'option',
        value,
        label,
        rawChildren: childProps.children,
        disabled: Boolean(childProps.disabled),
        group: groupName,
      })
      return
    }

    if (childProps && childProps.children) {
      items.push(...parseSelectChildren(childProps.children, groupName))
    }
  })

  return items
}

export const Select = React.forwardRef<HTMLSelectElement, SelectProps>(
  ({ className, triggerClassName, children, value, defaultValue, onChange, disabled, placeholder, ...props }, ref) => {
    const [internalValue, setInternalValue] = React.useState<string | number>(defaultValue ?? '')
    const currentValue = value !== undefined ? value : internalValue

    const parsedItems = React.useMemo(() => parseSelectChildren(children), [children])
    const options = React.useMemo(
      () => parsedItems.filter((i): i is ParsedOption => i.type === 'option'),
      [parsedItems],
    )

    const selectedOption = options.find((o) => o.value === String(currentValue))
    const displayLabel = selectedOption
      ? selectedOption.rawChildren || selectedOption.label
      : currentValue !== undefined && currentValue !== null && currentValue !== ''
      ? String(currentValue)
      : placeholder || '\u00A0'

    const [open, setOpen] = React.useState(false)
    const [activeIndex, setActiveIndex] = React.useState<number>(-1)
    const [searchTerm, setSearchTerm] = React.useState('')
    const listRef = React.useRef<HTMLDivElement>(null)
    const searchInputRef = React.useRef<HTMLInputElement>(null)
    const triggerRef = React.useRef<HTMLButtonElement>(null)
    const selectRef = React.useRef<HTMLSelectElement>(null)

    React.useEffect(() => {
      if (!open) {
        setSearchTerm('')
      } else if (props.searchable) {
        setTimeout(() => searchInputRef.current?.focus(), 50)
      }
    }, [open, props.searchable])

    const filteredItems = React.useMemo(() => {
      if (!props.searchable || !searchTerm.trim()) {
        return parsedItems
      }
      const term = searchTerm.trim().toLowerCase()
      const matchingOptions = options.filter((o) => o.label.toLowerCase().includes(term))
      const matchingGroups = new Set(matchingOptions.map((o) => o.group).filter(Boolean))

      return parsedItems.filter((item) => {
        if (item.type === 'group') {
          return matchingGroups.has(item.label)
        }
        return matchingOptions.includes(item)
      })
    }, [parsedItems, options, props.searchable, searchTerm])

    React.useImperativeHandle(ref, () => selectRef.current as HTMLSelectElement)

    const { refs, floatingStyles, context, isPositioned } = useFloating({
      open,
      onOpenChange: (isOpen) => {
        if (disabled) return
        setOpen(isOpen)
      },
      strategy: 'fixed',
      placement: 'bottom-start',
      whileElementsMounted: autoUpdate,
      middleware: [
        offset(4),
        flip({ padding: 8 }),
        shift({ padding: 8 }),
        size({
          padding: 8,
          apply({ availableHeight, elements, rects }) {
            Object.assign(elements.floating.style, {
              maxHeight: `${Math.min(320, Math.max(96, Math.floor(availableHeight)))}px`,
              minWidth: `${rects.reference.width}px`,
            })
          },
        }),
      ],
    })

    const dismiss = useDismiss(context, {
      outsidePress: true,
      outsidePressEvent: 'mousedown',
      escapeKey: true,
    })

    const { getReferenceProps, getFloatingProps } = useInteractions([dismiss])

    const handleSelect = React.useCallback(
      (optValue: string) => {
        if (disabled) return
        if (value === undefined) {
          setInternalValue(optValue)
        }
        if (selectRef.current) {
          selectRef.current.value = optValue
        }
        if (onChange) {
          const syntheticEvent = {
            target: { value: optValue, name: props.name || '' },
            currentTarget: { value: optValue, name: props.name || '' },
            preventDefault: () => {},
            stopPropagation: () => {},
          } as unknown as React.ChangeEvent<HTMLSelectElement>
          onChange(syntheticEvent)
        }
        setOpen(false)
        triggerRef.current?.focus()
      },
      [disabled, value, onChange, props.name],
    )

    const handleKeyDown = (e: React.KeyboardEvent) => {
      if (disabled) return

      if (!open) {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          setOpen(true)
          const currentIndex = options.findIndex((o) => o.value === String(currentValue))
          setActiveIndex(currentIndex >= 0 ? currentIndex : 0)
        }
        return
      }

      if (e.key === 'Escape') {
        e.preventDefault()
        setOpen(false)
        triggerRef.current?.focus()
        return
      }

      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActiveIndex((prev) => {
          let next = prev + 1
          while (next < options.length && options[next].disabled) next++
          return next < options.length ? next : prev
        })
        return
      }

      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIndex((prev) => {
          let prevIdx = prev - 1
          while (prevIdx >= 0 && options[prevIdx].disabled) prevIdx--
          return prevIdx >= 0 ? prevIdx : prev
        })
        return
      }

      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        if (activeIndex >= 0 && activeIndex < options.length) {
          const targetOption = options[activeIndex]
          if (!targetOption.disabled) {
            handleSelect(targetOption.value)
          }
        }
        return
      }

      if (e.key === 'Home') {
        e.preventDefault()
        const first = options.findIndex((o) => !o.disabled)
        if (first >= 0) setActiveIndex(first)
        return
      }

      if (e.key === 'End') {
        e.preventDefault()
        for (let i = options.length - 1; i >= 0; i--) {
          if (!options[i].disabled) {
            setActiveIndex(i)
            break
          }
        }
        return
      }

      // Typeahead character matching
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const char = e.key.toLowerCase()
        const matchIndex = options.findIndex(
          (o, idx) => idx > activeIndex && !o.disabled && o.label.toLowerCase().startsWith(char),
        )
        if (matchIndex >= 0) {
          setActiveIndex(matchIndex)
        } else {
          const wrapIndex = options.findIndex((o) => !o.disabled && o.label.toLowerCase().startsWith(char))
          if (wrapIndex >= 0) setActiveIndex(wrapIndex)
        }
      }
    }

    React.useEffect(() => {
      if (open && isPositioned && activeIndex >= 0 && listRef.current) {
        const activeEl = listRef.current.querySelector(`[data-index="${activeIndex}"]`)
        if (activeEl) {
          activeEl.scrollIntoView({ block: 'nearest' })
        }
      }
    }, [open, isPositioned, activeIndex])

    return (
      <div className={cn('relative min-w-0', className)}>
        {/* Hidden native select for form data & ref forwarding */}
        <select
          ref={selectRef}
          tabIndex={-1}
          aria-hidden="true"
          value={currentValue ?? ''}
          onChange={() => {}}
          name={props.name}
          id={props.id}
          disabled={disabled}
          className="sr-only pointer-events-none absolute -m-px h-px w-px overflow-hidden whitespace-nowrap border-0 p-0"
        >
          {children}
        </select>

        {/* Custom Styled Trigger Button */}
        <button
          ref={(el) => {
            triggerRef.current = el
            refs.setReference(el)
          }}
          type="button"
          role="combobox"
          aria-expanded={open}
          aria-haspopup="listbox"
          aria-label={props['aria-label'] || props.title}
          disabled={disabled}
          title={props.title}
          onClick={() => !disabled && setOpen(!open)}
          onKeyDown={handleKeyDown}
          {...getReferenceProps()}
          className={cn(
            'flex h-9 w-full items-center justify-between rounded-lg border border-border/80 dark:border-white/15 bg-input/60 px-3.5 py-1 text-sm text-foreground transition-colors hover:border-muted-foreground/40 dark:hover:border-white/25 hover:bg-input/80 shadow-xs cursor-pointer select-none focus-visible:border-ring focus-visible:bg-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50',
            open && 'border-ring ring-2 ring-ring/30 bg-background',
            triggerClassName,
          )}
        >
          <span className="truncate pr-2">{displayLabel}</span>
          <ChevronDown
            aria-hidden="true"
            className={cn(
              'size-4 shrink-0 text-muted-foreground transition-transform duration-200 pointer-events-none',
              open && 'rotate-180 text-foreground',
            )}
          />
        </button>

        {/* Floating Popover Menu */}
        {open && (
          <FloatingPortal>
            <div
              ref={(el) => {
                refs.setFloating(el)
              }}
              role="listbox"
              style={{
                ...floatingStyles,
                visibility: isPositioned ? undefined : 'hidden',
              }}
              onKeyDown={handleKeyDown}
              {...getFloatingProps()}
              className="z-[99] flex flex-col rounded-lg border border-border/80 dark:border-white/15 bg-white dark:bg-[#0b1426] text-sm text-popover-foreground shadow-2xl select-none focus:outline-none overflow-hidden"
            >
              {props.searchable && (
                <div className="border-b border-border/70 bg-white dark:bg-[#0b1426] p-1.5 shrink-0">
                  <div className="relative">
                    <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground pointer-events-none" />
                    <input
                      ref={searchInputRef}
                      type="text"
                      value={searchTerm}
                      onChange={(e) => setSearchTerm(e.target.value)}
                      placeholder={props.searchPlaceholder || 'Search options...'}
                      className="h-8 w-full rounded-md bg-muted/60 pl-8 pr-7 text-xs text-foreground placeholder:text-muted-foreground focus:bg-background focus:outline-none focus:ring-1 focus:ring-ring"
                      onClick={(e) => e.stopPropagation()}
                      onKeyDown={(e) => {
                        e.stopPropagation()
                        if (e.key === 'Escape') {
                          setOpen(false)
                        }
                      }}
                    />
                    {searchTerm && (
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation()
                          setSearchTerm('')
                        }}
                        className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 text-muted-foreground hover:text-foreground cursor-pointer"
                      >
                        <X className="size-3" />
                      </button>
                    )}
                  </div>
                </div>
              )}

              <div
                ref={listRef}
                className="overflow-y-auto max-h-64 p-1 flex flex-col gap-0.5 bg-white dark:bg-[#0b1426]"
              >
                {filteredItems.length === 0 ? (
                  <div className="py-4 text-center text-xs text-muted-foreground">
                    No options found
                  </div>
                ) : (
                  filteredItems.map((item, index) => {
                    if (item.type === 'group') {
                      return (
                        <div
                          key={`group-${item.label}-${index}`}
                          className="sticky -top-1 -mx-1 z-10 bg-white dark:bg-[#0b1426] px-3.5 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground border-b border-border/80 shadow-xs"
                        >
                          {item.label}
                        </div>
                      )
                    }

                    const isSelected = item.value === String(currentValue)
                    const optionIdx = options.indexOf(item)
                    const isFocused = optionIdx === activeIndex

                    return (
                      <button
                        key={`opt-${item.value}-${index}`}
                        type="button"
                        role="option"
                        data-index={optionIdx}
                        aria-selected={isSelected}
                        disabled={item.disabled}
                        onClick={() => handleSelect(item.value)}
                        onMouseEnter={() => !item.disabled && setActiveIndex(optionIdx)}
                        className={cn(
                          'flex w-full cursor-pointer items-center justify-between rounded-md px-2.5 py-1.5 text-left text-xs transition-colors',
                          isSelected
                            ? 'bg-primary/15 text-primary font-medium'
                            : isFocused
                            ? 'bg-accent/80 text-accent-foreground'
                            : 'text-foreground hover:bg-accent/80 hover:text-accent-foreground',
                          item.disabled && 'opacity-50 cursor-not-allowed pointer-events-none',
                        )}
                      >
                        <span className="truncate mr-2">{item.rawChildren || item.label}</span>
                        {isSelected && <Check className="size-3.5 text-primary shrink-0" />}
                      </button>
                    )
                  })
                )}
              </div>
            </div>
          </FloatingPortal>
        )}
      </div>
    )
  },
)

Select.displayName = 'Select'
