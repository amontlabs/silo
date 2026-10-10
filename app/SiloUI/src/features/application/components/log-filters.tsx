import { useId, useRef, useState } from "react"
import { CalendarDays } from "lucide-react"
import { formatDateTime } from "@/lib/format-date"
import { errorMessage } from "@/lib/error-message"
import { FilterCombobox, type FilterOption } from "@/components/filter-combobox"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover"
import { restoreFocus } from "@/lib/focus"

export interface LogFilterValues { source: string; since: string; until: string }
const sources = ["stdout", "stderr", "output", "system", "runtime", "kernel"] as const
const dateOption = "date"

/** Dates are optional absolute bounds; opening the editor never adds a default range. */
export function LogFilters({ source, since, until, onChange }: LogFilterValues & { onChange: (filters: LogFilterValues) => void }) {
  const [open, setOpen] = useState(false)
  const opener = useRef<HTMLElement | null>(null)
  const interactedOutside = useRef(false)
  const [fromDate, setFromDate] = useState("")
  const [fromTime, setFromTime] = useState("")
  const [toDate, setToDate] = useState("")
  const [toTime, setToTime] = useState("")
  const [error, setError] = useState("")
  const errorId = useId()
  const titleId = useId()
  const hasDates = Boolean(since || until)
  const selected = new Set<string>([...(source ? [source] : []), ...(hasDates ? [dateOption] : [])])
  const options: FilterOption<string>[] = [
    ...sources.map(value => ({ value, label: `Source: ${value}` })),
    { value: dateOption, label: "Date filter" },
  ]

  function editDates() {
    const from = localFields(since), to = localFields(until)
    setFromDate(from.date); setFromTime(from.time)
    setToDate(to.date); setToTime(to.time)
    setError(""); setOpen(true)
  }
  function changeSelection(values: Set<string>) {
    const addedSource = [...values].find(value => value !== dateOption && value !== source)
    const nextSource = addedSource ?? (values.has(source) ? source : "")
    if (values.has(dateOption) && !hasDates) { editDates(); return }
    onChange({ source: nextSource, since: values.has(dateOption) ? since : "", until: values.has(dateOption) ? until : "" })
  }
  function applyPreset(hours: number | "today") {
    const now = new Date()
    const start = hours === "today" ? new Date(now.getFullYear(), now.getMonth(), now.getDate()) : new Date(now.getTime() - hours * 3600000)
    onChange({ source, since: start.toISOString(), until: "" })
    setOpen(false)
  }
  function applyCustom() {
    try {
      const from = boundary(fromDate, fromTime, false)
      const to = boundary(toDate, toTime, true)
      if (!from && !to) throw new Error("Enter a start or end date, or choose a preset.")
      if (from && to && from > to) throw new Error("The start date is after the end date. Change the date filter to see logs.")
      onChange({ source, since: from, until: to })
      setError(""); setOpen(false)
    } catch (cause) { setError(errorMessage(cause)) }
  }
  const fields = [
    { label: "From", date: fromDate, time: fromTime, setDate: setFromDate, setTime: setFromTime },
    { label: "To", date: toDate, time: toTime, setDate: setToDate, setTime: setToTime },
  ]

  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverAnchor asChild>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <FilterCombobox
          options={options} selectedValues={selected} onChange={changeSelection}
          label="Log filters" inputLabel="Filter logs" placeholder="Add filter…"
          listLabel="Available log filters" selectedLabel="Selected log filters"
          emptyMessage="No matching filters." compact className="min-w-0 flex-1"
        />
        {hasDates && <Button type="button" size="xs" variant="ghost" aria-label="Edit date filter" onClick={editDates} className="max-w-full text-muted-foreground">
          <CalendarDays aria-hidden="true" className="size-3.5" /><span className="truncate" title={rangeLabel(since, until)}>{rangeLabel(since, until)}</span>
        </Button>}
      </div>
    </PopoverAnchor>
    <PopoverContent className="w-[min(22rem,calc(100vw-2rem))] p-3" aria-labelledby={titleId}
      onOpenAutoFocus={() => {
        opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
        interactedOutside.current = false
      }}
      onInteractOutside={() => { interactedOutside.current = true }}
      onCloseAutoFocus={event => {
        event.preventDefault()
        if (!interactedOutside.current) restoreFocus(opener.current)
      }}>
      <div className="mb-3 flex items-center gap-2"><CalendarDays className="size-4 text-muted-foreground" aria-hidden="true" /><h3 id={titleId} className="text-sm font-medium">Filter by date</h3></div>
      <div className="mb-3 grid grid-cols-2 gap-1.5" aria-label="Date presets">
        <Button type="button" variant="outline" size="xs" onClick={() => applyPreset(1)}>Last hour</Button>
        <Button type="button" variant="outline" size="xs" onClick={() => applyPreset(24)}>Last 24 hours</Button>
        <Button type="button" variant="outline" size="xs" onClick={() => applyPreset("today")}>Today</Button>
        <Button type="button" variant="outline" size="xs" onClick={() => applyPreset(168)}>Last 7 days</Button>
      </div>
      <form className="space-y-3 border-t border-border pt-3" onSubmit={event => { event.preventDefault(); applyCustom() }}>
        <p className="text-xs font-medium">Custom range <span className="font-normal text-muted-foreground">· local time</span></p>
        {fields.map(field => <fieldset key={field.label} className="grid grid-cols-[minmax(0,1fr)_5.5rem] gap-2">
          <legend className="mb-1 text-xs text-muted-foreground">{field.label}</legend>
          <Input technical aria-label={`${field.label} date`} placeholder="YYYY-MM-DD" value={field.date} autoComplete="off" spellCheck={false} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} onChange={event => { field.setDate(event.target.value); setError("") }} className="h-8 font-mono text-xs" />
          <Input technical aria-label={`${field.label} time`} placeholder="HH:mm" value={field.time} autoComplete="off" spellCheck={false} aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined} onChange={event => { field.setTime(event.target.value); setError("") }} className="h-8 font-mono text-xs" />
        </fieldset>)}
        <p className="text-caption leading-relaxed text-muted-foreground">Either date can be left blank. Leave times blank to include the full day.</p>
        {error && <p id={errorId} role="alert" className="text-xs text-destructive">{error}</p>}
        <div className="flex justify-end gap-2"><Button type="button" size="xs" variant="ghost" onClick={() => setOpen(false)}>Cancel</Button><Button type="submit" size="xs">Apply</Button></div>
      </form>
    </PopoverContent>
  </Popover>
}

function localFields(iso: string): { date: string; time: string } {
  if (!iso) return { date: "", time: "" }
  const date = new Date(iso)
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString()
  return { date: local.slice(0, 10), time: local.slice(11, 16) }
}
function boundary(date: string, time: string, end: boolean): string {
  if (!date.trim()) {
    if (time.trim()) throw new Error("Enter a date for each time.")
    return ""
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error("Use YYYY-MM-DD for dates.")
  if (time && !/^\d{2}:\d{2}$/.test(time)) throw new Error("Use HH:mm for times.")
  const [year, month, day] = date.split("-").map(Number)
  const [hour, minute] = (time || (end ? "23:59" : "00:00")).split(":").map(Number)
  const value = new Date(year, month - 1, day, hour, minute, end && !time ? 59 : 0, end && !time ? 999 : 0)
  if (value.getFullYear() !== year || value.getMonth() !== month - 1 || value.getDate() !== day || value.getHours() !== hour || value.getMinutes() !== minute) throw new Error("Enter a valid local date and time.")
  return value.toISOString()
}
function rangeLabel(since: string, until: string): string {
  const format = (value: string) => formatDateTime(value)
  return since && until ? `${format(since)} → ${format(until)}` : since ? `Since ${format(since)}` : `Until ${format(until)}`
}
