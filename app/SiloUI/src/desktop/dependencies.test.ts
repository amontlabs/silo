import { waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { createNativeDependencyStore, validateDependencyReport } from "./dependencies"

const checks = [
  { id: "system-os", title: "Supported OS", status: "pass", detail: "Supported.", remediation: null },
  { id: "system-virtualization", title: "Virtualization", status: "pass", detail: "Available.", remediation: null },
  { id: "runtime-microsandbox", title: "Computer runtime", status: "pass", detail: "0.7.6", remediation: null },
  { id: "tool-git", title: "Git", status: "pass", detail: "2.53.0", remediation: null },
  { id: "tool-git-lfs", title: "Git LFS", status: "pass", detail: "3.7.1", remediation: null },
] as const

describe("native dependency report validation", () => {
  it("does not invoke native checks when disposed before the queued request starts", async () => {
    vi.useFakeTimers()
    const invokeChecks = vi.fn().mockResolvedValue(undefined)
    const store = createNativeDependencyStore(invokeChecks)
    const listener = vi.fn()
    store.subscribe(listener)
    try {
      store.retry()
      store.dispose()
      const before = store.getSnapshot()
      await vi.runAllTimersAsync()
      store.retry()
      expect(invokeChecks).not.toHaveBeenCalled()
      expect(store.getSnapshot()).toBe(before)
      expect(listener).toHaveBeenCalledOnce()
      expect(vi.getTimerCount()).toBe(0)
    } finally { store.dispose(); vi.useRealTimers() }
  })

  it("drops a queued Retry when disposed before the in-flight request rejects", async () => {
    vi.useFakeTimers()
    let reject!: (error: Error) => void
    const invokeChecks = vi.fn(() => new Promise((_resolve, fail) => { reject = fail }))
    const store = createNativeDependencyStore(invokeChecks)
    try {
      store.retry()
      await vi.advanceTimersByTimeAsync(0)
      expect(invokeChecks).toHaveBeenCalledOnce()
      store.retry()
      store.dispose()
      const before = store.getSnapshot()
      reject(new Error("Bridge closed"))
      await vi.runAllTimersAsync()
      expect(invokeChecks).toHaveBeenCalledOnce()
      expect(store.getSnapshot()).toBe(before)
      expect(vi.getTimerCount()).toBe(0)
    } finally { store.dispose(); vi.useRealTimers() }
  })

  it("accepts one current result for every required check", () => {
    expect(validateDependencyReport({ schemaVersion: 1, requestId: "current", checkedAtMs: 10_000, checks }, "current", 10_100)).toHaveLength(5)
  })

  it("rejects missing, stale, and mismatched results", () => {
    expect(() => validateDependencyReport({ schemaVersion: 1, requestId: "current", checkedAtMs: 10_000, checks: checks.slice(1) }, "current", 10_100)).toThrow()
    expect(() => validateDependencyReport({ schemaVersion: 1, requestId: "old", checkedAtMs: 10_000, checks }, "current", 10_100)).toThrow(/stale request/)
    expect(() => validateDependencyReport({ schemaVersion: 1, requestId: "current", checkedAtMs: 10_000, checks }, "current", 50_100)).toThrow(/stale result/)
  })

  it("preserves specific timeout and unavailable states", () => {
    const exceptional = checks.map((check) => check.id === "system-virtualization" ? { ...check, status: "timeout" as const, detail: "Timed out." } : check)
    expect(validateDependencyReport({ schemaVersion: 1, requestId: "current", checkedAtMs: 10_000, checks: exceptional }, "current", 10_100)[1]).toMatchObject({ status: "timeout", detail: "Timed out." })
  })

  it("uses the production store to clear a bridge error and recover on Retry", async () => {
    const invokeChecks = vi.fn()
      .mockRejectedValueOnce(new Error("bridge down"))
      .mockImplementationOnce((_command, { requestId }) => Promise.resolve({ schemaVersion: 1, requestId, checkedAtMs: Date.now(), checks }))
    const store = createNativeDependencyStore(invokeChecks)
    store.retry()
    await waitFor(() => expect(store.getSnapshot()[0].status).toBe("unavailable"))
    expect(store.getSnapshot().every(({ remediation }) => remediation?.startsWith("Retry checks.") && !/reinstall/i.test(remediation))).toBe(true)
    store.retry()
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    await waitFor(() => expect(store.getSnapshot().every(({ status }) => status === "pass")).toBe(true))
    expect(invokeChecks).toHaveBeenCalledTimes(2)
    store.dispose()
  })

  it("retains completed checks and specific failures when native probes exhaust their shared budget", async () => {
    vi.useFakeTimers()
    const partial = checks.map((check) => check.id === "tool-git" ? {
      ...check, status: "timeout" as const, detail: "The signature check timed out.", remediation: "Retry checks.",
    } : check)
    const invokeChecks = vi.fn((_command, { requestId }) => new Promise((resolve) => {
      setTimeout(() => resolve({ schemaVersion: 1, requestId, checkedAtMs: Date.now(), checks: partial }), 13_000)
    }))
    const store = createNativeDependencyStore(invokeChecks)
    try {
      store.retry()
      await vi.advanceTimersByTimeAsync(13_000)
      expect(store.getSnapshot()).toEqual(partial)
      await vi.advanceTimersByTimeAsync(2_000)
      expect(store.getSnapshot()).toEqual(partial)
    } finally {
      store.dispose()
      vi.useRealTimers()
    }
  })

  it("keeps watchdog timeout terminal when the native result arrives late", async () => {
    vi.useFakeTimers()
    let resolveRequest!: (value: unknown) => void
    const invokeChecks = vi.fn((_command, { requestId }) => new Promise((resolve) => {
      resolveRequest = () => resolve({ schemaVersion: 1, requestId, checkedAtMs: Date.now(), checks })
    }))
    const store = createNativeDependencyStore(invokeChecks)

    store.retry()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)
    expect(store.getSnapshot().every(({ remediation }) => remediation?.startsWith("Retry checks.") && !/reinstall/i.test(remediation))).toBe(true)
    resolveRequest(undefined)
    await vi.runAllTimersAsync()
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)

    store.dispose()
    vi.useRealTimers()
  })

  it("coalesces repeated Retry requests and ignores the stale in-flight result", async () => {
    const requests: Array<{ requestId: string; resolve: (value: unknown) => void }> = []
    const invokeChecks = vi.fn((_command, { requestId }) => new Promise((resolve) => requests.push({ requestId, resolve })))
    const store = createNativeDependencyStore(invokeChecks)

    store.retry()
    await waitFor(() => expect(requests).toHaveLength(1))
    store.retry()
    store.retry()
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    expect(invokeChecks).toHaveBeenCalledOnce()

    requests[0].resolve({ schemaVersion: 1, requestId: requests[0].requestId, checkedAtMs: Date.now(), checks })
    await waitFor(() => expect(requests).toHaveLength(2))
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    const unavailable = checks.map((check) => check.id === "system-virtualization" ? { ...check, status: "unavailable" as const } : check)
    requests[1].resolve({ schemaVersion: 1, requestId: requests[1].requestId, checkedAtMs: Date.now(), checks: unavailable })
    await waitFor(() => expect(store.getSnapshot()[1].status).toBe("unavailable"))
    expect(invokeChecks).toHaveBeenCalledTimes(2)
    store.dispose()
  })

  it("keeps a queued Retry bounded when the previous bridge request never settles", async () => {
    vi.useFakeTimers()
    const invokeChecks = vi.fn(() => new Promise(() => undefined))
    const store = createNativeDependencyStore(invokeChecks)

    store.retry()
    await Promise.resolve()
    store.retry()
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)
    expect(invokeChecks).toHaveBeenCalledOnce()

    store.dispose()
    vi.useRealTimers()
  })

  it("starts a new request on Retry after the watchdog abandons a bridge call that never settles", async () => {
    vi.useFakeTimers()
    const invokeChecks = vi.fn()
      .mockImplementationOnce(() => new Promise(() => undefined))
      .mockImplementationOnce((_command, { requestId }) => Promise.resolve({ schemaVersion: 1, requestId, checkedAtMs: Date.now(), checks }))
    const store = createNativeDependencyStore(invokeChecks)

    store.retry()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)
    store.retry()
    await vi.advanceTimersByTimeAsync(0)
    expect(invokeChecks).toHaveBeenCalledTimes(2)
    expect(store.getSnapshot().every(({ status }) => status === "pass")).toBe(true)

    store.dispose()
    vi.useRealTimers()
  })

  it("ignores an abandoned call that settles late without disturbing the next request", async () => {
    vi.useFakeTimers()
    const requests: Array<{ requestId: string; resolve: (value: unknown) => void }> = []
    const invokeChecks = vi.fn((_command, { requestId }) => new Promise((resolve) => requests.push({ requestId, resolve })))
    const store = createNativeDependencyStore(invokeChecks)

    store.retry()
    await Promise.resolve()
    store.retry()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)
    store.retry()
    await vi.advanceTimersByTimeAsync(0)
    expect(requests).toHaveLength(2)
    // The abandoned first call answers now: it must neither publish nor clear the
    // second request's watchdog or in-flight state.
    requests[0].resolve({ schemaVersion: 1, requestId: requests[0].requestId, checkedAtMs: Date.now(), checks })
    await vi.advanceTimersByTimeAsync(0)
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    store.retry()
    await vi.advanceTimersByTimeAsync(0)
    expect(invokeChecks).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(store.getSnapshot().every(({ status }) => status === "timeout")).toBe(true)

    store.dispose()
    vi.useRealTimers()
  })

  it("disposes listeners and suppresses late results", async () => {
    let resolveRequest!: (value: unknown) => void
    let requestId = ""
    const listener = vi.fn()
    const invokeChecks = vi.fn((_command, args) => new Promise((resolve) => {
      requestId = args.requestId
      resolveRequest = resolve
    }))
    const store = createNativeDependencyStore(invokeChecks)
    store.subscribe(listener)
    store.retry()
    await waitFor(() => expect(invokeChecks).toHaveBeenCalledOnce())
    store.dispose()
    resolveRequest({ schemaVersion: 1, requestId, checkedAtMs: Date.now(), checks })
    await Promise.resolve()
    await Promise.resolve()
    expect(store.getSnapshot().every(({ status }) => status === "pending")).toBe(true)
    expect(listener).toHaveBeenCalledOnce()
  })
})
