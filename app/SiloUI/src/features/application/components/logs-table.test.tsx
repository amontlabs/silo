import { useState, type ComponentProps } from "react"
import { act, fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { LogsTable as ControlledLogsTable, type LogRow } from "./logs-table"

function LogsTable(props: Omit<ComponentProps<typeof ControlledLogsTable>, "expandedRows" | "onExpandedRowsChange">) {
  const [expandedRows, setExpandedRows] = useState<ReadonlyMap<string, number>>(() => new Map())
  return <ControlledLogsTable {...props} expandedRows={expandedRows} onExpandedRowsChange={setExpandedRows} />
}

const computer = applicationSourceForScenario("running").computers[0]
function rows(count: number): LogRow[] {
  return Array.from({ length: count }, (_, index) => ({ computer, entry: {
    id: String(index), line: `record ${index}`, occurredAt: "2026-09-22T10:00:00Z",
    deviceId: "local", computerId: computer.configuration.id, source: "runtime",
  } }))
}
function props(entries = rows(1)) {
  return {
    rows: entries, loading: false, loadingOlder: false, hasOlder: false, active: true,
    scrollTop: 0, onScrollTopChange: vi.fn(), onLoadOlder: vi.fn(),
  }
}
function viewport() { return screen.getByRole("table", { name: "Logs" }).querySelector('[data-table-scroll="logs"]') as HTMLDivElement }
function measure(height: number, scrollHeight: number) {
  Object.defineProperties(viewport(), { clientHeight: { configurable: true, value: height }, scrollHeight: { configurable: true, value: scrollHeight } })
}
let resize: (() => void)[] = []
beforeEach(() => {
  resize = []
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize.push(callback) }
    observe() {}
    unobserve() {}
    disconnect() {}
  })
})
afterEach(() => vi.unstubAllGlobals())

describe("logs table", () => {
  it("lets keyboard users focus the expanded message scroll container", async () => {
    const user = userEvent.setup()
    const properties = props()
    properties.rows[0].entry.line = Array.from({ length: 50 }, (_, index) => `Diagnostic line ${index}`).join("\n")
    render(<LogsTable {...properties} />)
    await user.click(screen.getByRole("button", { name: /^Expand log from dev/ }))
    await user.tab()
    const output = screen.getByRole("region", { name: /^Log details from dev/ }).querySelector("pre")!
    expect(output).toHaveFocus()
    expect(output).toHaveAccessibleName(/^Log message from dev at /)
    expect(output).toHaveTextContent("Diagnostic line 49")
  })

  it("fills the table with skeleton rows while the first page loads", () => {
    render(<LogsTable {...props([])} loading />)
    const table = screen.getByRole("table", { name: "Logs" })
    expect(table).toHaveAttribute("aria-busy", "true")
    expect(within(table).getAllByRole("columnheader").map(header => header.textContent)).toEqual(["Time", "Message", "Computer", "Source", "Actions"])
    expect(table.querySelectorAll('[data-log-skeleton]')).toHaveLength(8)
    expect(within(table).queryByRole("button")).not.toBeInTheDocument()
  })

  it("keeps metadata readable and expands the complete log directly below its row", () => {
    const properties = props()
    properties.rows[0].entry.line = 'First line\nSecond line with the complete diagnostic'
    render(<LogsTable {...properties} />)
    const row = within(screen.getByRole("table", { name: "Logs" })).getAllByRole("row")[1]
    const cells = within(row).getAllByRole("cell")
    expect(cells).toHaveLength(5)
    expect(cells[3]).toHaveTextContent("runtime")
    expect(cells[2]).not.toHaveTextContent("This device")
    const buttons = within(row).getAllByRole("button")
    expect(buttons[0]).toHaveAccessibleName(/^Copy log line/)
    expect(buttons[1]).toHaveAccessibleName(/^Expand log from dev at /)
    expect(buttons[1]).toHaveAttribute("aria-expanded", "false")
    fireEvent.click(buttons[1])
    const details = screen.getByRole("region", { name: /^Log details from dev at / })
    expect(details.querySelector("pre")?.textContent).toBe(properties.rows[0].entry.line)
    expect(row.nextElementSibling).toContainElement(details)
    expect(buttons[1]).toHaveAttribute("aria-expanded", "true")
    expect(buttons[1]).toHaveAccessibleName(/^Collapse log from dev at /)
    expect(buttons[1]).toHaveAttribute("aria-controls", details.id)
    fireEvent.click(buttons[1])
    expect(screen.queryByRole("region", { name: /^Log details from dev at / })).not.toBeInTheDocument()
    expect(buttons[1]).toHaveAttribute("aria-expanded", "false")
    expect(properties.onLoadOlder).not.toHaveBeenCalled()
    expect(screen.queryByRole("button", { name: "Load older" })).not.toBeInTheDocument()
  })

  it("distinguishes a same-named remote computer and reveals its device on hover or focus", async () => {
    const entries = rows(2)
    entries[1].computer = { ...computer, device: { id: "office", name: "Office Mac", address: "office.local", connected: true, computerId: "remote-dev" } }
    entries[1].entry = { ...entries[1].entry, deviceId: "office", computerId: "remote-dev" }
    const user = userEvent.setup()
    render(<LogsTable {...props(entries)} />)
    expect(screen.queryByRole("columnheader", { name: "Device" })).not.toBeInTheDocument()
    const local = screen.getByLabelText("dev, Running")
    const remote = screen.getByLabelText("dev, Running, on Office Mac")
    expect(local).toHaveTextContent(/^dev$/)
    expect(remote).toHaveTextContent("dev · Office Mac")
    expect(remote.querySelector('[data-computer-state-dot="running"]')).not.toBeNull()
    expect(remote).toHaveAttribute("tabindex", "0")
    await user.hover(remote)
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Office Mac")
    await user.unhover(remote)
    fireEvent.focus(remote)
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Office Mac")
    expect(screen.queryByRole("region", { name: /^Log details/ })).not.toBeInTheDocument()
  })

  it("restores the saved scroll position and renders enough rows for a tall viewport", () => {
    const entries = rows(10_000)
    function Table() {
      const [scrollTop, setScrollTop] = useState(52_000)
      return <LogsTable {...props(entries)} scrollTop={scrollTop} onScrollTopChange={setScrollTop} />
    }
    render(<Table />)
    expect(viewport().scrollTop).toBe(52_000)
    measure(4_160, 520_032)
    act(() => resize.forEach(callback => callback()))
    expect(screen.getByText("record 1078")).toBeInTheDocument()
    expect(screen.queryByText("record 0")).not.toBeInTheDocument()
    expect(within(screen.getByRole("table")).getAllByRole("row").length).toBeLessThan(100)
    fireEvent.scroll(viewport(), { target: { scrollTop: 104_000 } })
    expect(screen.getByText("record 2078")).toBeInTheDocument()
    expect(screen.queryByText("record 1000")).not.toBeInTheDocument()
    expect(screen.getByRole("table")).toHaveAttribute("aria-rowcount", "10001")
  })

  it("accounts for expanded height while virtualizing and retains the disclosure when scrolling back", () => {
    const entries = rows(10_000)
    function Table() {
      const [scrollTop, setScrollTop] = useState(0)
      return <LogsTable {...props(entries)} scrollTop={scrollTop} onScrollTopChange={setScrollTop} />
    }
    render(<Table />)
    const trigger = screen.getAllByRole("button", { name: /^Expand log/ })[0]
    fireEvent.click(trigger)
    const group = trigger.closest("tbody")!
    vi.spyOn(group, "getBoundingClientRect").mockReturnValue({ height: 312 } as DOMRect)
    measure(520, 520_292)
    act(() => resize.forEach(callback => callback()))
    fireEvent.scroll(viewport(), { target: { scrollTop: 52_032 } })
    const rendered = within(screen.getByRole("table")).getAllByRole("row")
    expect(rendered[1]).toHaveTextContent("record 987")
    expect(rendered.length).toBeLessThan(40)
    expect(screen.queryByRole("region")).not.toBeInTheDocument()
    fireEvent.scroll(viewport(), { target: { scrollTop: 0 } })
    expect(screen.getByRole("region")).toHaveTextContent("record 0")
    expect(screen.getByRole("button", { name: /^Collapse log/ })).toHaveAttribute("aria-expanded", "true")
  })

  it("includes expanded details in the infinite-scroll threshold", () => {
    const properties = props(rows(200))
    render(<LogsTable {...properties} hasOlder />)
    const trigger = screen.getAllByRole("button", { name: /^Expand log/ })[0]
    fireEvent.click(trigger)
    vi.spyOn(trigger.closest("tbody")!, "getBoundingClientRect").mockReturnValue({ height: 312 } as DOMRect)
    measure(520, 10_432)
    act(() => resize.forEach(callback => callback()))
    fireEvent.scroll(viewport(), { target: { scrollTop: 9800 } })
    expect(properties.onLoadOlder).not.toHaveBeenCalled()
    fireEvent.scroll(viewport(), { target: { scrollTop: 10300 } })
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(1)
  })

  it("prefetches near the end once per page and preserves loaded rows while waiting", () => {
    const properties = props(rows(200))
    const view = render(<LogsTable {...properties} hasOlder />)
    measure(520, 10_432)
    fireEvent.scroll(viewport(), { target: { scrollTop: 9000 } })
    expect(properties.onLoadOlder).not.toHaveBeenCalled()
    fireEvent.scroll(viewport(), { target: { scrollTop: 9800 } })
    fireEvent.scroll(viewport())
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(1)
    view.rerender(<LogsTable {...properties} hasOlder loadingOlder />)
    expect(screen.getByText("record 199")).toBeInTheDocument()
    expect(screen.getByRole("table").querySelectorAll('[data-log-skeleton]')).toHaveLength(1)
    fireEvent.scroll(viewport())
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(1)
    view.rerender(<LogsTable {...properties} rows={rows(400)} hasOlder />)
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(1)
    measure(520, 20_832)
    fireEvent.scroll(viewport(), { target: { scrollTop: 20_000 } })
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(2)
  })

  it("reports plain scrolling without notifying the owner and renders the rows at the new offset", () => {
    const properties = props(rows(200))
    render(<LogsTable {...properties} />)
    measure(520, 10_432)
    expect(screen.getByText("record 0")).toBeInTheDocument()
    fireEvent.scroll(viewport(), { target: { scrollTop: 5000 } })
    expect(properties.onScrollTopChange).toHaveBeenLastCalledWith(5000, false)
    expect(screen.queryByText("record 0")).not.toBeInTheDocument()
    expect(screen.getByText("record 96")).toBeInTheDocument()
  })

  it("keeps short result sets visible when a previous scroll offset exceeds their height", () => {
    render(<LogsTable {...props(rows(5))} scrollTop={10_000} />)
    expect(screen.getByText("record 0")).toBeInTheDocument()
    expect(screen.getByText("record 4")).toBeInTheDocument()
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(6)
  })

  it("fills a short viewport after layout and waits for the next page before loading again", () => {
    const properties = props(rows(2))
    const view = render(<LogsTable {...properties} hasOlder />)
    expect(properties.onLoadOlder).not.toHaveBeenCalled()
    measure(520, 136)
    act(() => resize.forEach(callback => callback()))
    act(() => resize.forEach(callback => callback()))
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(1)
    view.rerender(<LogsTable {...properties} rows={rows(4)} hasOlder />)
    expect(properties.onLoadOlder).toHaveBeenCalledTimes(2)
  })

  it.each([{ active: false }, { loading: true }, { loadingOlder: true }, { hasOlder: false }])("does not request older pages when unavailable: %o", unavailable => {
    const properties = props(rows(2))
    render(<LogsTable {...properties} hasOlder {...unavailable} />)
    measure(520, 136)
    fireEvent.scroll(viewport())
    act(() => resize.forEach(callback => callback()))
    expect(properties.onLoadOlder).not.toHaveBeenCalled()
  })
})
