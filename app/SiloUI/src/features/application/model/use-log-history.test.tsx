import { act, renderHook, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { fixtureLogPage, logEntryKey, type LogPage, type LogQuery } from "./logs"
import { useLogHistory } from "./use-log-history"

function fixture() {
  const computer = structuredClone(applicationSourceForScenario("running").computers[0])
  computer.logs = ["10", "09", "08", "07"].map(hour => ({ occurredAt: `2026-09-18T${hour}:00:00Z`, line: `record ${hour}` }))
  const loader = vi.fn(async (request: LogQuery) => fixtureLogPage(computer, { ...request, limit: 2 }))
  return { computer, loader, options: { computers: [computer], loader, active: true, query: "", source: "", since: "", until: "", invalidRange: false } }
}
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}
afterEach(() => vi.useRealTimers())

describe("cached log history", () => {
  it.each([false, true])("names the owning devices for equally named computer failures (older page: %s)", async older => {
    const { options, computer } = fixture()
    const remote = { ...computer, configuration: { ...computer.configuration, id: "remote" }, device: { id: "office", name: "Office Mac", address: "owner@office", connected: true, computerId: "vm-1" } }
    options.computers = [computer, remote]
    let fail = !older
    options.loader = vi.fn(async request => {
      if (fail) throw new Error("Connection lost")
      return fixtureLogPage(request.deviceId ? remote : computer, { ...request, limit: 2 })
    })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    if (older) { fail = true; await act(() => view.result.current.loadOlder()) }
    expect(view.result.current.error).toBe(`${computer.configuration.name} (This device): Connection lost; ${computer.configuration.name} (Office Mac): Connection lost`)
  })

  it.each([false, true])("preserves a structured failure message and retries the failed read (older page: %s)", async older => {
    const { options, computer, loader } = fixture()
    const message = "The connection was lost. Reconnect this device, then retry."
    if (!older) loader.mockRejectedValueOnce({ code: "internal", message })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    if (older) {
      loader.mockRejectedValueOnce({ code: "internal", message })
      await act(() => view.result.current.loadOlder())
    }
    expect(view.result.current.error).toBe(`${computer.configuration.name}: ${message}`)
    await act(() => view.result.current.retry())
    expect(view.result.current.error).toBe("")
    expect(view.result.current.rows).toHaveLength(older ? 4 : 2)
  })

  it("does not reorder log entries when computer presentation refreshes", async () => {
    const { options, computer } = fixture()
    const page = fixtureLogPage(computer, { computerId: computer.configuration.id })
    const reads = vi.fn(() => page.entries[0].occurredAt)
    const occurredAt = page.entries[0].occurredAt
    reads.mockImplementation(() => occurredAt)
    Object.defineProperty(page.entries[0], "occurredAt", { get: reads })
    options.loader = vi.fn(async () => page)
    const view = renderHook(({ computers }) => useLogHistory({ ...options, computers }), { initialProps: { computers: [computer] } })
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    reads.mockClear()
    const renamed = { ...computer, configuration: { ...computer.configuration, name: "Renamed computer" } }
    view.rerender({ computers: [renamed] })
    expect(view.result.current.rows[0].computer.configuration.name).toBe("Renamed computer")
    expect(reads).not.toHaveBeenCalled()
    expect(options.loader).toHaveBeenCalledOnce()
  })

  it("pages through 50,000 records while retaining at most 5,000, including across navigation", async () => {
    const { options, computer } = fixture()
    options.loader = vi.fn(async request => {
      const offset = Number(request.cursor ?? 0)
      return {
        entries: Array.from({ length: Math.min(200, 50_000 - offset) }, (_, index) => ({ id: String(offset + index), line: `record ${offset + index}`, occurredAt: new Date(1700000000000 - (offset + index) * 1000).toISOString(), computerId: computer.configuration.id, deviceId: "local", source: "output" })),
        nextCursor: offset + 200 < 50_000 ? String(offset + 200) : null,
        totalMatches: 50_000, timestampEstimated: false, oldestAvailableTimestamp: null, newestAvailableTimestamp: null,
      }
    })
    let view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    const seen = new Set<string>()
    for (let page = 0; page < 250; page++) {
      if (page) await act(() => view.result.current.loadOlder())
      const retained = view.result.current.results.flatMap(result => result.page.entries)
      expect(retained.length).toBeLessThanOrEqual(5_000)
      for (const { entry } of view.result.current.rows) seen.add(entry.id)
      if (page === 24) {
        act(() => {
          view.result.current.setScrollTop(5000 * 52 - 520)
          view.result.current.setExpandedRows(() => new Map([[logEntryKey({ deviceId: "local", computerId: computer.configuration.id, id: "0" }), 172]]))
        })
      }
      if (page === 25) {
        expect(view.result.current.scrollTop).toBe(5000 * 52 - 520 - 200 * 52 - 120)
        expect(view.result.current.expandedRows.size).toBe(0)
        view.unmount()
        view = renderHook(() => useLogHistory(options))
        expect(options.loader).toHaveBeenCalledTimes(26)
      }
    }
    expect(seen.size).toBe(50_000)
    expect(view.result.current.hasOlder).toBe(false)
    expect(view.result.current.results[0].request).not.toHaveProperty("cursor")
    options.loader.mockRejectedValueOnce(new Error("Disconnected"))
    await act(() => view.result.current.refresh())
    expect(view.result.current.historyLimited).toBe(true)
    expect(view.result.current.rows).toHaveLength(5000)
    await act(() => view.result.current.refresh())
    expect(view.result.current.rows[0].entry.id).toBe("0")
    expect(view.result.current.scrollTop).toBe(0)
  })

  it("bounds retained log text before reaching the record limit", async () => {
    const { options, computer } = fixture()
    computer.logs = Array.from({ length: 100 }, (_, index) => ({ line: "x".repeat(64 * 1024), occurredAt: new Date(1700000000000 + index * 1000).toISOString() }))
    options.loader = vi.fn(async request => fixtureLogPage(computer, { ...request, limit: 10 }))
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    const seen = new Set<string>()
    for (let page = 0; page < 10; page++) {
      if (page) await act(() => view.result.current.loadOlder())
      const retained = view.result.current.results.flatMap(result => result.page.entries)
      expect(retained.reduce((sum, entry) => sum + 2 * entry.line.length, 0)).toBeLessThanOrEqual(8 * 1024 * 1024)
      for (const { entry } of view.result.current.rows) seen.add(entry.id)
    }
    expect(seen.size).toBe(100)
  })

  it.each([false, true])("keeps quiet-owner records buffered while a busy owner's bounded window advances (timestamp ties: %s)", async sameTime => {
    const { options, computer } = fixture()
    const quiet = { ...computer, configuration: { ...computer.configuration, id: "quiet" } }
    options.computers = [computer, quiet]
    options.loader = vi.fn(async request => {
      const offset = Number(request.cursor ?? 0)
      const quietOwner = request.computerId === "quiet"
      const total = quietOwner ? 1000 : 6000
      return {
        entries: Array.from({ length: Math.min(200, total - offset) }, (_, index) => ({ id: sameTime ? String(7000 - offset - index - (quietOwner ? 6000 : 0)).padStart(6, "0") : `${request.computerId}:${offset + index}`, line: "record", occurredAt: new Date(1700000000000 - (sameTime ? 0 : offset + index + (quietOwner ? 6000 : 0)) * 1000).toISOString(), computerId: request.computerId, deviceId: "local", source: "output" })),
        nextCursor: offset + 200 < total ? String(offset + 200) : null,
        totalMatches: total, timestampEstimated: false, oldestAvailableTimestamp: null, newestAvailableTimestamp: null,
      }
    })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    const seen = new Set(view.result.current.rows.map(row => row.entry.id))
    for (let page = 0; page < 33; page++) {
      await act(() => view.result.current.loadOlder())
      expect(view.result.current.results.reduce((sum, result) => sum + result.page.entries.length, 0)).toBeLessThanOrEqual(5000)
      for (const { entry } of view.result.current.rows) seen.add(entry.id)
      if (page < 28) expect(options.loader.mock.calls.filter(([request]) => request.computerId === "quiet")).toHaveLength(1)
    }
    expect(view.result.current.hasOlder).toBe(false)
    expect(seen.size).toBe(7000)
  })

  it("rejects a cursor cycle after its original records have left the window", async () => {
    const { options, computer } = fixture()
    options.loader = vi.fn(async request => {
      const offset = Number(request.cursor ?? 0)
      return {
        entries: [{ id: String(offset), line: "record", occurredAt: new Date(1700000000000 - offset * 1000).toISOString(), computerId: computer.configuration.id, deviceId: "local", source: "output" }],
        nextCursor: String(offset + 1), totalMatches: 10_000, timestampEstimated: false, oldestAvailableTimestamp: null, newestAvailableTimestamp: null,
      }
    })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    options.loader.mockImplementation(async request => {
      const offset = Number(request.cursor ?? 0)
      return { ...fixtureLogPage(computer, request), entries: Array.from({ length: 200 }, (_, index) => ({ id: String(offset + index), line: "record", occurredAt: new Date(1700000000000 - (offset + index) * 1000).toISOString(), computerId: computer.configuration.id, deviceId: "local", source: "output" })), nextCursor: String(offset + 200) }
    })
    for (let page = 0; page < 70; page++) await act(() => view.result.current.loadOlder())
    expect(view.result.current.rows.some(row => row.entry.id === "0")).toBe(false)
    const cycle = { ...fixtureLogPage(computer, { computerId: computer.configuration.id }), entries: [{ id: "0", line: "old cycle", occurredAt: new Date(1700000000000).toISOString(), computerId: computer.configuration.id, deviceId: "local", source: "output" }], nextCursor: "1" }
    options.loader.mockResolvedValueOnce(cycle)
    await act(() => view.result.current.loadOlder())
    expect(view.result.current.hasOlder).toBe(false)
    expect(view.result.current.error).toContain("did not advance")
  })

  it("discards a single oversized record and retains the query and older cursor", async () => {
    const { options, computer } = fixture()
    const page = fixtureLogPage(computer, { computerId: computer.configuration.id, limit: 2 })
    page.entries[0].line = "x".repeat(4 * 1024 * 1024)
    options.loader = vi.fn(async () => page)
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    expect(view.result.current.rows).toHaveLength(1)
    expect(view.result.current.historyLimited).toBe(true)
    expect(view.result.current.hasOlder).toBe(true)
    expect(view.result.current.results[0].request).not.toHaveProperty("cursor")
    expect(view.result.current.results[0].page.entries[0].line).toBe("record 09")
  })

  it("restores older pages and scroll position immediately after navigation without fetching", async () => {
    const { options, loader } = fixture()
    const first = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(first.result.current.ready).toBe(true))
    await act(() => first.result.current.loadOlder())
    act(() => first.result.current.setScrollTop(520))
    const rows = first.result.current.rows
    first.unmount()
    const second = renderHook(() => useLogHistory(options))
    expect(second.result.current.rows).toHaveLength(4)
    expect(second.result.current.scrollTop).toBe(520)
    expect(second.result.current.busy).toBe(false)
    expect(loader).toHaveBeenCalledTimes(2)
    const restoredRows = second.result.current.rows
    act(() => second.result.current.setScrollTop(700))
    expect(second.result.current.rows).toBe(restoredRows)
    expect(rows.map(row => row.entry.id)).toEqual(second.result.current.rows.map(row => row.entry.id))
    second.rerender()
    expect(loader).toHaveBeenCalledTimes(2)
  })

  it("deduplicates an unfinished initial request across unmount and remount", async () => {
    const { options, computer } = fixture()
    const pending = deferred<LogPage>()
    options.loader = vi.fn(() => pending.promise)
    const first = renderHook(() => useLogHistory(options))
    expect(first.result.current.busy).toBe(true)
    first.unmount()
    const second = renderHook(() => useLogHistory(options))
    expect(options.loader).toHaveBeenCalledTimes(1)
    await act(async () => pending.resolve(fixtureLogPage(computer, { computerId: computer.configuration.id })))
    expect(second.result.current.rows).toHaveLength(4)
  })

  it("caches filters independently and ignores late responses from a previous query", async () => {
    const { options, computer } = fixture()
    const pending = deferred<LogPage>()
    options.loader = vi.fn(request => request.query === "slow" ? pending.promise : Promise.resolve(fixtureLogPage(computer, request)))
    const view = renderHook(({ query }) => useLogHistory({ ...options, query }), { initialProps: { query: "slow" } })
    view.rerender({ query: "record 08" })
    await waitFor(() => expect(view.result.current.rows).toHaveLength(1))
    await act(async () => pending.resolve(fixtureLogPage(computer, { computerId: computer.configuration.id })))
    expect(view.result.current.rows[0].entry.line).toBe("record 08")
    view.rerender({ query: "slow" })
    expect(view.result.current.rows).toHaveLength(4)
    expect(options.loader).toHaveBeenCalledTimes(2)
  })

  it("keeps loaded records visible during refresh and does not refresh on activation", async () => {
    const { options, computer, loader } = fixture()
    const view = renderHook(({ active }) => useLogHistory({ ...options, active }), { initialProps: { active: true } })
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    view.rerender({ active: false })
    view.rerender({ active: true })
    expect(loader).toHaveBeenCalledTimes(1)
    act(() => view.result.current.setScrollTop(520))
    const pending = deferred<LogPage>()
    loader.mockImplementationOnce(() => pending.promise)
    let refresh!: Promise<void>
    act(() => { refresh = view.result.current.refresh() })
    expect(view.result.current.rows).toHaveLength(2)
    expect(view.result.current.busy).toBe(true)
    expect(view.result.current.scrollTop).toBe(520)
    await act(async () => { pending.resolve(fixtureLogPage(computer, { computerId: computer.configuration.id })); await refresh })
    expect(view.result.current.rows).toHaveLength(4)
    expect(view.result.current.scrollTop).toBe(0)
  })

  it("retains healthy owners and successful older pages while retrying only a failed older cursor", async () => {
    const { options, computer } = fixture()
    const remote = { ...computer, configuration: { ...computer.configuration, id: "remote", name: "remote computer" } }
    let fail = true
    options.computers = [computer, remote]
    options.loader = vi.fn(async request => {
      if (request.computerId === "remote" && request.cursor && fail) throw new Error("Offline")
      return fixtureLogPage(request.computerId === "remote" ? remote : computer, { ...request, limit: 2 })
    })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    await act(() => view.result.current.loadOlder())
    expect(view.result.current.results.map(result => result.page.entries.length)).toEqual([4, 2])
    expect(view.result.current.error).toContain("remote computer: Offline")
    fail = false
    await act(() => view.result.current.retry())
    expect(options.loader).toHaveBeenCalledTimes(5)
    expect(options.loader).toHaveBeenLastCalledWith(expect.objectContaining({ computerId: "remote", cursor: "2" }))
    expect(view.result.current.rows).toHaveLength(8)
    expect(view.result.current.error).toBe("")
  })

  it("buffers older quiet-owner records until all unfinished owners reach them", async () => {
    const { options, computer } = fixture()
    const quiet = { ...computer, configuration: { ...computer.configuration, id: "quiet" }, logs: [{ occurredAt: "2026-09-18T06:00:00Z", line: "quiet 06" }] }
    options.computers = [computer, quiet]
    options.loader = vi.fn(async request => fixtureLogPage(request.computerId === "quiet" ? quiet : computer, { ...request, limit: 2 }))
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    expect(view.result.current.rows.map(row => row.entry.line)).toEqual(["record 10", "record 09"])
    await act(() => view.result.current.loadOlder())
    expect(view.result.current.rows.map(row => row.entry.line)).toEqual(["record 10", "record 09", "record 08", "record 07", "quiet 06"])
  })

  it("preserves loaded history, scroll and an unavailable owner's error when later paging succeeds", async () => {
    const { options, computer } = fixture()
    const offline = { ...computer, configuration: { ...computer.configuration, id: "offline", name: "offline computer" } }
    options.computers = [computer, offline]
    options.loader = vi.fn(async request => {
      if (request.computerId === "offline") throw new Error("Disconnected")
      return fixtureLogPage(computer, { ...request, limit: 2 })
    })
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    await act(() => view.result.current.loadOlder())
    expect(view.result.current.rows).toHaveLength(4)
    expect(view.result.current.error).toContain("offline computer: Disconnected")
    act(() => view.result.current.setScrollTop(100))
    options.loader.mockRejectedValue(new Error("All disconnected"))
    await act(() => view.result.current.refresh())
    expect(view.result.current.rows).toHaveLength(4)
    expect(view.result.current.scrollTop).toBe(100)
    expect(view.result.current.error).toContain("All disconnected")
  })

  it("deduplicates concurrent paging and stops a repeated cursor", async () => {
    const { options, computer, loader } = fixture()
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    const pending = deferred<LogPage>()
    loader.mockImplementationOnce(() => pending.promise)
    let older!: Promise<void>
    act(() => { older = view.result.current.loadOlder(); void view.result.current.loadOlder() })
    expect(loader).toHaveBeenCalledTimes(2)
    await act(async () => { pending.resolve({ ...fixtureLogPage(computer, { computerId: computer.configuration.id, cursor: "2" }), nextCursor: "2" }); await older })
    expect(view.result.current.rows).toHaveLength(4)
    expect(view.result.current.hasOlder).toBe(false)
    expect(view.result.current.error).toContain("did not advance")
    await act(() => view.result.current.loadOlder())
    expect(loader).toHaveBeenCalledTimes(2)
  })

  it("refreshes on Retry when a new cursor adds no unique records", async () => {
    const { options, computer, loader } = fixture()
    const view = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    const firstPage = fixtureLogPage(computer, { computerId: computer.configuration.id, limit: 2 })
    loader.mockResolvedValueOnce({ ...firstPage, nextCursor: "different-cursor" })

    await act(() => view.result.current.loadOlder())

    expect(loader).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: "2" }))
    expect(view.result.current.rows).toHaveLength(2)
    expect(view.result.current.hasOlder).toBe(false)
    expect(view.result.current.error).toContain("Log history did not advance. Refresh to continue.")
    loader.mockResolvedValueOnce(fixtureLogPage(computer, { computerId: computer.configuration.id }))

    await act(() => view.result.current.retry())

    expect(loader).toHaveBeenCalledTimes(3)
    expect(loader).toHaveBeenLastCalledWith(expect.not.objectContaining({ cursor: expect.any(String) }))
    expect(view.result.current.rows).toHaveLength(4)
    expect(view.result.current.error).toBe("")
    expect(view.result.current.busy).toBe(false)
  })

  it("makes no initial or explicit reads while inactive or the date range is invalid", async () => {
    const { options, loader } = fixture()
    const view = renderHook(({ active, invalidRange }) => useLogHistory({ ...options, active, invalidRange }), {
      initialProps: { active: false, invalidRange: false },
    })
    const tryReads = async () => {
      await act(async () => {
        await view.result.current.refresh()
        await view.result.current.follow()
        await view.result.current.retry()
        await view.result.current.loadOlder()
      })
    }
    await tryReads()
    expect(loader).not.toHaveBeenCalled()
    view.rerender({ active: true, invalidRange: true })
    await tryReads()
    expect(loader).not.toHaveBeenCalled()

    view.rerender({ active: true, invalidRange: false })
    await waitFor(() => expect(view.result.current.ready).toBe(true))
    expect(view.result.current.rows).toHaveLength(2)
    view.rerender({ active: false, invalidRange: false })
    await tryReads()
    expect(loader).toHaveBeenCalledOnce()
    expect(view.result.current.rows).toHaveLength(2)
  })

  it.each([10 * 60_000 - 1, 10 * 60_000, 10 * 60_000 + 1])("expires inactive views before retained backend cursors expire (%i ms)", async elapsed => {
    const now = new Date("2026-10-02T12:00:00Z").getTime()
    vi.useFakeTimers({ now, toFake: ["Date"] })
    const { options, loader } = fixture()
    const first = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(first.result.current.ready).toBe(true))
    first.unmount()
    vi.setSystemTime(now + elapsed)
    const second = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(second.result.current.ready).toBe(true))
    expect(loader).toHaveBeenCalledTimes(elapsed < 10 * 60_000 ? 1 : 2)
  })

  it("evicts the least recently used inactive view after the cache fills", async () => {
    const now = new Date("2026-10-02T12:00:00Z").getTime()
    vi.useFakeTimers({ now, toFake: ["Date"] })
    const { options, loader } = fixture()
    for (let index = 0; index < 10; index++) {
      vi.setSystemTime(now + index * 1000)
      const view = renderHook(() => useLogHistory({ ...options, query: String(index) }))
      await waitFor(() => expect(view.result.current.ready).toBe(true))
      view.unmount()
    }
    const last = renderHook(() => useLogHistory({ ...options, query: "9" }))
    expect(last.result.current.ready).toBe(true)
    expect(loader).toHaveBeenCalledTimes(10)
    last.unmount()
    const oldest = renderHook(() => useLogHistory({ ...options, query: "0" }))
    await waitFor(() => expect(oldest.result.current.ready).toBe(true))
    expect(loader).toHaveBeenCalledTimes(11)
  })

  it("bounds the combined text of inactive histories", async () => {
    const { options, computer, loader } = fixture()
    computer.logs = Array.from({ length: 40 }, (_, index) => ({ line: "x".repeat(64 * 1024), occurredAt: new Date(1700000000000 + index * 1000).toISOString() }))
    const first = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(first.result.current.ready).toBe(true))
    for (let page = 1; page < 20; page++) await act(() => first.result.current.loadOlder())
    expect(first.result.current.rows).toHaveLength(40)
    first.unmount()
    const other = renderHook(() => useLogHistory({ ...options, query: "x" }))
    await waitFor(() => expect(other.result.current.ready).toBe(true))
    for (let page = 1; page < 20; page++) await act(() => other.result.current.loadOlder())
    other.unmount()
    const second = renderHook(() => useLogHistory(options))
    await waitFor(() => expect(second.result.current.ready).toBe(true))
    expect(loader).toHaveBeenCalledTimes(41)
  })
})
