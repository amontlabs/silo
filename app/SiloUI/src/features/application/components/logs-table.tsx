import { formatLocalTimeOfDay, formatMonthDay } from "@/lib/format-date"
import { useEffect, useEffectEvent, useLayoutEffect, useMemo, useRef, useState } from "react"
import { CopyButton } from "@/components/copy-button"
import { DisclosureIndicator, disclosureTriggerStateClass } from "@/components/disclosure-indicator"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { ComputerBadge } from "./application-ui"
import type { ApplicationComputer } from "../model/application-source"
import { formatLog, logEntryKey, LOG_ROW_HEIGHT as ROW_HEIGHT, type LogEntry } from "../model/logs"

export interface LogRow { entry: LogEntry; computer: ApplicationComputer }
interface LogsTableProps {
  rows: LogRow[]
  loading: boolean
  loadingOlder: boolean
  hasOlder: boolean
  active: boolean
  scrollTop: number
  /** Changes whenever the owner sets `scrollTop`, even to the value it already held. */
  scrollEpoch?: number
  /** `notify: false` reports plain scrolling, which needs no re-render of the owner. */
  onScrollTopChange: (scrollTop: number, notify?: boolean) => void
  onLoadOlder: () => void
  expandedRows: ReadonlyMap<string, number>
  onExpandedRowsChange: (update: (current: ReadonlyMap<string, number>) => ReadonlyMap<string, number>) => void
}

const HEADER_HEIGHT = 32
const OVERSCAN = 8
const PREFETCH_DISTANCE = ROW_HEIGHT * 6
const ESTIMATED_DETAILS_HEIGHT = 120
function rowKey({ entry }: LogRow) { return logEntryKey(entry) }

function rowAtOffset(offsets: number[], offset: number) {
  let low = 0, high = offsets.length - 1
  while (low < high) {
    const middle = Math.ceil((low + high) / 2)
    if (offsets[middle] <= offset) low = middle
    else high = middle - 1
  }
  return Math.min(low, Math.max(0, offsets.length - 2))
}

function LogRecord({ row, rowIndex, open, onOpenChange, onHeightChange }: {
  row: LogRow; rowIndex: number; open: boolean
  onOpenChange: (open: boolean) => void; onHeightChange: (height: number) => void
}) {
  const { entry, computer } = row
  const element = useRef<HTMLTableSectionElement>(null)
  const reportHeight = useEffectEvent(() => {
    const height = element.current?.getBoundingClientRect().height ?? 0
    if (open && height > ROW_HEIGHT) onHeightChange(height)
  })
  useLayoutEffect(() => {
    if (!open || !element.current) return
    reportHeight()
    const observer = new ResizeObserver(() => reportHeight())
    observer.observe(element.current)
    return () => observer.disconnect()
  }, [open, entry.line])
  const embedded = /^(\d{2}:\d{2}:\d{2})\s{2,}(.*)$/.exec(entry.line)
  const timestamp = new Date(entry.occurredAt)
  const time = embedded?.[1] ?? formatLocalTimeOfDay(timestamp)
  const label = `log from ${computer.configuration.name} at ${time}`
  return <Collapsible asChild open={open} onOpenChange={onOpenChange}>
    <tbody ref={element} role="rowgroup" className="collapsible-motion">
      <tr role="row" aria-rowindex={rowIndex} style={{ height: ROW_HEIGHT }} className="group/log-row border-b border-border row-hover">
        <td role="cell" title={entry.guestTimestamp ? `${entry.occurredAt} (time reported by the computer)` : entry.occurredAt} className="px-3 font-mono whitespace-nowrap text-muted-foreground"><time dateTime={entry.occurredAt}>{time}</time><span className="block text-caption">{formatMonthDay(timestamp)}</span></td>
        <td role="cell" title={`${entry.source}${entry.session ? ` · session ${entry.session}` : ""}\n${entry.line}`} className="max-w-0 truncate px-3 font-mono">{embedded?.[2] ?? entry.line}</td>
        <td role="cell" className="px-3 whitespace-nowrap"><ComputerBadge name={computer.configuration.name} state={computer.state} device={computer.device} /></td>
        <td role="cell" className="px-3 whitespace-nowrap text-muted-foreground">{entry.source}</td>
        <td role="cell" className="px-3"><div className="flex w-12 items-center justify-end">
          <CopyButton size="icon-xs" variant="ghost" className="opacity-0 group-hover/log-row:opacity-100 group-focus-within/log-row:opacity-100" value={() => formatLog(entry)} labels={{ idle: `Copy log line from ${computer.configuration.name} at ${time}`, copied: "Log line copied", failed: "Copy log line failed" }} />
          <CollapsibleTrigger asChild><Button size="icon-xs" variant="ghost" className={disclosureTriggerStateClass} title={open ? "Collapse log" : "Expand log"} aria-label={`${open ? "Collapse" : "Expand"} ${label}`}><DisclosureIndicator /></Button></CollapsibleTrigger>
        </div></td>
      </tr>
      <tr role="row" aria-rowindex={open ? rowIndex + 1 : undefined} aria-hidden={!open}>
        <td role="cell" colSpan={5} className="max-w-0 p-0">
          <CollapsibleContent role="region" aria-label={`Log details from ${computer.configuration.name} at ${time}`} className="overflow-hidden">
            <div className="border-b border-border bg-muted/20 px-3 py-3">
              <p className="mb-2 text-caption text-muted-foreground">{entry.occurredAt}{entry.guestTimestamp ? " (time reported by the computer)" : ""} · {entry.source}{entry.session ? ` · Session ${entry.session}` : ""}</p>
              <pre role="group" tabIndex={0} aria-label={`Log message from ${computer.configuration.name} at ${time}`} className="max-h-80 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-5 select-text focus-ring-inset">{entry.line}</pre>
            </div>
          </CollapsibleContent>
        </td>
      </tr>
    </tbody>
  </Collapsible>
}

function SkeletonRow() {
  return <tr aria-hidden="true" data-log-skeleton style={{ height: ROW_HEIGHT }} className="border-b border-border">
    <td className="px-3"><div className="h-3 w-14 animate-pulse rounded bg-muted motion-reduce:animate-none" /></td>
    <td className="px-3"><div className="h-3 w-3/4 animate-pulse rounded bg-muted motion-reduce:animate-none" /></td>
    <td className="px-3"><div className="h-5 w-16 animate-pulse rounded-full bg-muted motion-reduce:animate-none" /></td>
    <td className="px-3"><div className="h-3 w-14 animate-pulse rounded bg-muted motion-reduce:animate-none" /></td>
    <td className="px-3"><div className="ml-auto size-4 animate-pulse rounded bg-muted motion-reduce:animate-none" /></td>
  </tr>
}

function Spacer({ height }: { height: number }) {
  return height > 0 ? <tr aria-hidden="true"><td colSpan={5} style={{ height }} className="border-0 p-0" /></tr> : null
}

export function LogsTable({ rows, loading, loadingOlder, hasOlder, active, scrollTop, scrollEpoch = 0, onScrollTopChange, onLoadOlder, expandedRows, onExpandedRowsChange }: LogsTableProps) {
  const viewport = useRef<HTMLDivElement>(null)
  const requestedRows = useRef<LogRow[] | undefined>(undefined)
  const [viewportHeight, setViewportHeight] = useState(520)
  // The window of rendered rows follows the scroll position; state changes only when the first visible row does.
  const [windowTop, setWindowTop] = useState(scrollTop)
  const [seenScroll, setSeenScroll] = useState({ scrollTop, scrollEpoch })
  if (seenScroll.scrollTop !== scrollTop || seenScroll.scrollEpoch !== scrollEpoch) { setSeenScroll({ scrollTop, scrollEpoch }); setWindowTop(scrollTop) }
  const liveTop = useRef(scrollTop)
  useLayoutEffect(() => { liveTop.current = scrollTop }, [scrollTop, scrollEpoch])
  const keys = useMemo(() => rows.map(rowKey), [rows])
  const layout = useMemo(() => {
    const offsets = [0], positions: number[] = []
    let details = 0
    keys.forEach((key, index) => {
      positions.push(index + details + 2)
      const open = expandedRows.has(key)
      offsets.push(offsets[index] + (expandedRows.get(key) ?? ROW_HEIGHT))
      if (open) details++
    })
    return { offsets, positions, rowCount: keys.length + details + 1 }
  }, [keys, expandedRows])
  function preserveAnchor(index: number, delta: number) {
    const top = viewport.current?.scrollTop ?? liveTop.current
    if (index < rowAtOffset(layout.offsets, Math.max(0, top - HEADER_HEIGHT))) onScrollTopChange(Math.max(0, top + delta))
  }
  function toggleRow(key: string, index: number, open: boolean) {
    preserveAnchor(index, (open ? 1 : -1) * ((expandedRows.get(key) ?? ROW_HEIGHT + ESTIMATED_DETAILS_HEIGHT) - ROW_HEIGHT))
    onExpandedRowsChange(current => { const next = new Map(current); if (open) next.set(key, ROW_HEIGHT + ESTIMATED_DETAILS_HEIGHT); else next.delete(key); return next })
  }
  function measureRow(key: string, index: number, height: number) {
    const previous = expandedRows.get(key) ?? ROW_HEIGHT + ESTIMATED_DETAILS_HEIGHT
    if (Math.abs(previous - height) < 0.5) return
    preserveAnchor(index, height - previous)
    onExpandedRowsChange(current => current.has(key) ? new Map(current).set(key, height) : current)
  }
  const loadNearEnd = useEffectEvent(() => {
    const element = viewport.current
    if (!element || element.clientHeight <= 0 || !active || loading || loadingOlder || !hasOlder) return
    // The known virtual height also guards against a stale layout measurement
    // immediately after appending a page.
    const contentHeight = Math.max(element.scrollHeight, HEADER_HEIGHT + layout.offsets[rows.length])
    if (contentHeight - element.scrollTop - element.clientHeight > PREFETCH_DISTANCE || requestedRows.current === rows) return
    // Scroll and resize can arrive before React commits the loading state.
    requestedRows.current = rows
    onLoadOlder()
  })
  const measure = useEffectEvent(() => {
    const height = viewport.current?.clientHeight ?? 0
    if (height > 0) setViewportHeight(height)
    loadNearEnd()
  })
  useLayoutEffect(() => {
    if (viewport.current && viewport.current.scrollTop !== scrollTop) viewport.current.scrollTop = scrollTop
  }, [scrollTop, scrollEpoch, active, loading])
  useEffect(() => {
    const element = viewport.current
    if (!element) return
    const observer = new ResizeObserver(() => measure())
    const onScroll = () => measure()
    observer.observe(element)
    element.addEventListener("scroll", onScroll, { passive: true })
    return () => { observer.disconnect(); element.removeEventListener("scroll", onScroll) }
  }, [])
  useEffect(() => { measure() }, [active, hasOlder, loading, loadingOlder, rows, layout])

  function scrolled(top: number) {
    liveTop.current = top
    onScrollTopChange(top, false)
    const firstRow = (offset: number) => rowAtOffset(layout.offsets, Math.max(0, offset - HEADER_HEIGHT))
    setWindowTop(current => firstRow(current) === firstRow(top) ? current : top)
  }

  const visibleCount = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2
  const start = Math.max(0, Math.min(rows.length - visibleCount, rowAtOffset(layout.offsets, Math.max(0, windowTop - HEADER_HEIGHT)) - OVERSCAN))
  const visible = rows.slice(start, start + visibleCount)
  return <div role="table" aria-label="Logs" aria-busy={loading || loadingOlder} aria-rowcount={loading ? undefined : layout.rowCount} className="flex max-h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border border-border text-xs">
    {(loading || loadingOlder) && <span role="status" className="sr-only">{loading ? "Loading logs" : "Loading older logs"}</span>}
    <div ref={viewport} onScroll={event => scrolled(event.currentTarget.scrollTop)} style={{ overflowAnchor: "none" }} className="min-h-0 overflow-x-auto overflow-y-auto overscroll-contain bg-card" data-table-scroll="logs">
      <table role="presentation" className="w-full min-w-[40rem] border-collapse text-left">
        <thead role="rowgroup" className="sticky top-0 z-10 bg-muted">
          <tr role="row" aria-rowindex={1} style={{ height: HEADER_HEIGHT }} className="shrink-0 border-b border-border font-medium text-muted-foreground">
            <th role="columnheader" className="px-3 font-medium whitespace-nowrap">Time</th>
            <th role="columnheader" className="w-full min-w-64 px-3 font-medium">Message</th>
            <th role="columnheader" className="px-3 font-medium whitespace-nowrap">Computer</th>
            <th role="columnheader" className="px-3 font-medium whitespace-nowrap">Source</th>
            <th role="columnheader" className="px-3 font-medium"><span className="sr-only">Actions</span></th>
          </tr>
        </thead>
        {loading ? <tbody role="rowgroup">{Array.from({ length: 8 }, (_, index) => <SkeletonRow key={index} />)}</tbody> : <>
          <tbody aria-hidden="true"><Spacer height={layout.offsets[start]} /></tbody>
          {visible.map((row, offset) => {
            const index = start + offset
            const key = keys[index]
            return <LogRecord key={key} row={row} rowIndex={layout.positions[index]} open={expandedRows.has(key)} onOpenChange={open => toggleRow(key, index, open)} onHeightChange={height => measureRow(key, index, height)} />
          })}
          <tbody aria-hidden="true"><Spacer height={layout.offsets[rows.length] - layout.offsets[start + visible.length]} /></tbody>
          {loadingOlder && <tbody aria-hidden="true"><SkeletonRow /></tbody>}
        </>}
      </table>
    </div>
  </div>
}
