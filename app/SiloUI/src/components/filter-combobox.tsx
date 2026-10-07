import { useEffect, useId, useRef, useState } from "react"
import { Search, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover"
import { cn } from "@/lib/utils"
import { isRestoringFocus } from "@/lib/focus"

export interface FilterOption<Value extends string> {
  value: Value
  label: string
}

export function FilterCombobox<Value extends string>({
  options,
  selectedValues,
  onChange,
  label,
  inputLabel,
  placeholder,
  listLabel,
  selectedLabel,
  emptyMessage,
  inputInvalid,
  inputDescribedBy,
  compact = false,
  className,
}: {
  options: ReadonlyArray<FilterOption<Value>>
  selectedValues: ReadonlySet<Value>
  onChange: (values: Set<Value>) => void
  label: string
  inputLabel: string
  placeholder: string
  listLabel: string
  selectedLabel: string
  emptyMessage: string
  inputInvalid?: boolean
  inputDescribedBy?: string
  compact?: boolean
  className?: string
}) {
  const listboxId = useId()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [activeIndex, setActiveIndex] = useState(0)
  const activeOption = useRef<HTMLButtonElement>(null)
  const selectedOptions = options.filter(({ value }) => selectedValues.has(value))
  const results = options.filter(({ value, label: optionLabel }) => (
    !selectedValues.has(value)
    && optionLabel.toLowerCase().includes(query.trim().toLowerCase())
  ))
  const lastIndex = Math.max(0, results.length - 1)
  if (activeIndex > lastIndex) setActiveIndex(lastIndex)
  const activeValue = results[activeIndex]?.value
  useEffect(() => {
    if (open) activeOption.current?.scrollIntoView({ block: "nearest", inline: "nearest" })
  }, [open, activeIndex, activeValue])

  function addValue(value: Value) {
    onChange(new Set([...selectedValues, value]))
    setQuery("")
    setActiveIndex(0)
    setOpen(false)
  }

  function removeValue(value: Value) {
    const next = new Set(selectedValues)
    next.delete(value)
    onChange(next)
  }

  return (
    <div
      className={cn("flex min-w-0 flex-wrap items-center gap-2", compact ? "w-fit max-w-full" : "w-full", className)}
      role="group"
      aria-label={label}
    >
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverAnchor asChild>
          <div className={cn("relative shrink-0", compact ? "w-36" : "w-48")}>
            <Search aria-hidden="true" className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input technical
              role="combobox"
              aria-label={inputLabel}
              aria-invalid={inputInvalid}
              aria-describedby={inputDescribedBy}
              aria-autocomplete="list"
              aria-expanded={open}
              aria-controls={listboxId}
              aria-activedescendant={open && results[activeIndex] ? `${listboxId}-${activeIndex}` : undefined}
              size={compact ? "sm" : undefined}
              className={cn("pl-8", compact ? "w-36" : "w-48")}
              placeholder={placeholder}
              value={query}
              onFocus={event => { if (!isRestoringFocus(event.currentTarget)) setOpen(true) }}
              onClick={() => setOpen(true)}
              onChange={(event) => {
                setQuery(event.target.value)
                setActiveIndex(0)
                setOpen(true)
              }}
              onKeyDown={(event) => {
                if (event.nativeEvent.isComposing) return
                if (event.key === "ArrowDown") {
                  event.preventDefault()
                  setOpen(true)
                  setActiveIndex((current) => Math.min(current + 1, Math.max(0, results.length - 1)))
                } else if (event.key === "ArrowUp") {
                  event.preventDefault()
                  setActiveIndex((current) => Math.max(0, current - 1))
                } else if (event.key === "Enter") {
                  event.preventDefault()
                  if (open && results[activeIndex]) addValue(results[activeIndex].value)
                  else setOpen(true)
                } else if (event.key === "Escape" && open) {
                  event.preventDefault()
                  event.stopPropagation()
                  setOpen(false)
                }
              }}
            />
          </div>
        </PopoverAnchor>
        <PopoverContent
          id={listboxId}
          role="listbox"
          aria-label={listLabel}
          className="max-h-[min(15rem,var(--radix-popover-content-available-height))] w-[var(--radix-popover-trigger-width)] overflow-y-auto overscroll-contain p-1"
          onOpenAutoFocus={(event) => event.preventDefault()}
          onEscapeKeyDown={(event) => {
            event.preventDefault()
            if (!event.isComposing) setOpen(false)
          }}
        >
          {results.length > 0 ? results.map((option, index) => (
            <button
              ref={index === activeIndex ? activeOption : undefined}
              key={option.value}
              id={`${listboxId}-${index}`}
              type="button"
              tabIndex={-1}
              role="option"
              aria-selected={index === activeIndex}
              className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-accent focus:bg-accent aria-selected:bg-accent"
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => addValue(option.value)}
            >
              {option.label}
            </button>
          )) : (
            <p className="px-2 py-1.5 text-xs text-muted-foreground">{emptyMessage}</p>
          )}
        </PopoverContent>
      </Popover>

      <div className={cn("flex min-w-0 flex-wrap gap-1.5", !compact && "flex-1 basis-40")} aria-label={selectedLabel}>
        {selectedOptions.map((option) => (
          <span
            key={option.value}
            className={cn(
              "inline-flex max-w-full items-center gap-1 rounded-full border border-border bg-muted/55 pr-1 text-xs font-medium",
              compact ? "h-7 pl-2" : "h-8 pl-2.5",
            )}
          >
            <span className="truncate" title={option.label}>{option.label}</span>
            <button
              type="button"
              aria-label={`Remove ${option.label}`}
              onClick={() => removeValue(option.value)}
              className={cn(
                "grid shrink-0 place-items-center rounded-full text-muted-foreground hover:bg-background hover:text-foreground focus-ring",
                compact ? "size-5" : "size-6",
              )}
            >
              <X className={compact ? "size-3" : "size-3.5"} aria-hidden="true" />
            </button>
          </span>
        ))}
      </div>

      <Button type="button" className="order-last ml-auto" variant="ghost" size="xs" disabled={selectedValues.size === 0} onClick={() => onChange(new Set())}>Clear</Button>
    </div>
  )
}
