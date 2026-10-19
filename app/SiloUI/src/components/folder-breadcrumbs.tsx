import { useLayoutEffect, useRef, useState } from "react"
import { ChevronRight, MoreHorizontal } from "lucide-react"
import { DropdownMenu } from "radix-ui"

import { useReduceMotion } from "@/components/ui/reduce-motion"
import { visibleText } from "@/lib/visible-text"

const crumbClass = "min-w-0 truncate rounded px-1 py-1 text-muted-foreground hover:bg-muted focus-ring-inset"

export function FolderBreadcrumbs({ segments, onNavigate }: {
  segments: string[]
  onNavigate: (segments: string[]) => void
}) {
  const container = useRef<HTMLElement>(null)
  const measurement = useRef<HTMLDivElement>(null)
  const selectedAncestor = useRef(false)
  const reduceMotion = useReduceMotion()
  const [layout, setLayout] = useState({ collapsed: false, rootVisible: true })
  // Folder names come from the guest: reveal invisible and bidirectional characters.
  const labels = ["/workspace", ...segments.map(visibleText)]
  const path = labels.join("/")

  useLayoutEffect(() => {
    const nav = container.current!
    const full = measurement.current!
    function measure() {
      const width = nav.clientWidth
      const rootWidth = full.firstElementChild?.getBoundingClientRect().width ?? 0
      // Leave at least 80px for the current folder beside the menu and separators.
      const next = { collapsed: full.scrollWidth > width, rootVisible: width >= rootWidth + 36 + 32 + 80 }
      setLayout((previous) => previous.collapsed === next.collapsed && previous.rootVisible === next.rootVisible ? previous : next)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(nav)
    observer.observe(full)
    measure()
    return () => observer.disconnect()
  }, [path])

  const last = labels.length - 1
  const hidden = layout.collapsed ? labels.slice(layout.rootVisible ? 1 : 0, last) : []
  function crumb(index: number) {
    return <button type="button" className={`${crumbClass} ${index === last ? "flex-1 text-left" : "shrink-0"}`}
      title={labels[index]} aria-current={index === last ? "location" : undefined}
      onClick={() => onNavigate(segments.slice(0, index))}>{labels[index]}</button>
  }
  const separator = <ChevronRight className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />

  return <nav ref={container} aria-label="Folder path" className="relative min-w-0 overflow-hidden text-caption">
    <div ref={measurement} aria-hidden="true" className="pointer-events-none invisible absolute flex w-max items-center gap-0.5 whitespace-nowrap">
      {labels.map((label, index) => <span key={index} className="flex shrink-0 items-center gap-0.5">
        {index > 0 && separator}<span className="px-1 py-1">{label}</span>
      </span>)}
    </div>
    <div className="flex min-w-0 items-center gap-0.5">
      {last === 0 ? crumb(0) : <>
        {(!layout.collapsed || layout.rootVisible) && <>{crumb(0)}{separator}</>}
        {hidden.length > 0 && <>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger className="shrink-0 rounded px-1 py-1 text-muted-foreground hover:bg-muted focus-ring-inset" aria-label="Show parent folders">
              <MoreHorizontal className="size-4" />
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content align="start" sideOffset={4} collisionPadding={8}
                onCloseAutoFocus={(event) => {
                  if (selectedAncestor.current) event.preventDefault()
                  selectedAncestor.current = false
                }}
                data-reduce-motion={reduceMotion || undefined}
                className="silo-window z-50 max-h-[var(--radix-dropdown-menu-content-available-height)] max-w-[calc(100vw-16px)] overflow-y-auto overflow-x-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md">
                {hidden.map((label, offset) => {
                  const index = offset + (layout.rootVisible ? 1 : 0)
                  const ancestor = labels.slice(0, index + 1).join("/")
                  return <DropdownMenu.Item key={ancestor} title={ancestor} onSelect={() => { selectedAncestor.current = true; onNavigate(segments.slice(0, index)) }}
                    className="cursor-default rounded px-2 py-1.5 text-xs outline-none data-[highlighted]:bg-accent">
                    <span className="block max-w-64 truncate" title={label}>{label}</span>
                  </DropdownMenu.Item>
                })}
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
          {separator}
        </>}
        {!layout.collapsed && segments.slice(0, -1).map((_, index) => <span key={index} className="flex shrink-0 items-center gap-0.5">{crumb(index + 1)}{separator}</span>)}
        {crumb(last)}
      </>}
    </div>
  </nav>
}
