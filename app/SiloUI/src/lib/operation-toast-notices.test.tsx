import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const delivered = vi.hoisted(() => vi.fn())
vi.mock("@/desktop/notices", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/desktop/notices")>()), deliverNotice: delivered }))

import { Toaster } from "@/components/ui/sonner"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { showBackendNotice } from "@/features/application/model/use-backend-notices"
import {
  dismissOperationToast,
  dismissComputerToasts,
  dismissComputerToastsById,
  LONG_OPERATION_MS,
  runWithOperationToast,
  showActionFailure,
  showOperationFailure,
  showOperationNotice,
  showOperationProgress,
  showOperationSuccess,
  showQuickConfirmation,
} from "@/lib/operation-toast"

function Host() { return <SettingsProvider initialSettings={{ theme: "light" }}><Toaster /></SettingsProvider> }
const tick = () => act(async () => { await Promise.resolve(); await vi.advanceTimersByTimeAsync(50) })
const computer = { id: "vm-1", name: "dev" }

beforeEach(() => { vi.useFakeTimers({ now: new Date("2026-10-02T12:00:00Z") }); delivered.mockClear() })
afterEach(() => { dismissOperationToast("op"); vi.useRealTimers() })

describe("system notice mirroring", () => {
  it.each(["busy", "future_error"])("shows a structured %s error message in action and system notifications", async (code) => {
    render(<Host />)
    const message = "Another computer operation is running. Wait for it to finish, then retry."
    act(() => showActionFailure("Could not open computer", { code, message }))
    await tick()
    expect(screen.getByText(message)).toBeInTheDocument()
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument()
    expect(delivered.mock.calls[0][0].body).toBe(message)
  })

  it("keeps the native error message when a background operation rejects", async () => {
    render(<Host />)
    const message = "This Silo version does not support that remote operation."
    await act(async () => {
      await runWithOperationToast("op", { loading: "Opening computer", success: "Computer opened", failure: "Could not open computer" },
        () => Promise.reject({ code: "unsupported_remote_operation", message }))
    })
    await tick()
    expect(screen.getByText(message)).toBeInTheDocument()
    expect(delivered.mock.calls[0][0].body).toBe(message)
  })

  it("mirrors a failure as a failures notice keyed by the toast id", () => {
    showOperationFailure("op", "Push failed", { description: "rejected", noticeComputer: computer })
    expect(delivered).toHaveBeenCalledExactlyOnceWith({ category: "failures", key: "op", title: "Push failed", body: "rejected", computer })
  })

  it("uses an empty body for a non-string description and no computer by default", () => {
    showOperationFailure("op", "Import failed", { description: <p>rich</p> })
    expect(delivered).toHaveBeenCalledExactlyOnceWith({ category: "failures", key: "op", title: "Import failed", body: "", computer: null })
  })

  it("mirrors an action failure with a key derived from its title", () => {
    showActionFailure("Could not open port 80", new Error("closed"), undefined, { noticeComputer: computer })
    showActionFailure("Could not open port 80", "again")
    expect(delivered).toHaveBeenNthCalledWith(1, { category: "failures", key: "action-failure:Could not open port 80", title: "Could not open port 80", body: "closed", computer })
    expect(delivered.mock.calls[1][0].key).toBe("action-failure:Could not open port 80")
  })

  it.each([LONG_OPERATION_MS - 1, LONG_OPERATION_MS, LONG_OPERATION_MS + 1])("mirrors a success only after progress shown for over 3 seconds (%i ms)", elapsed => {
    const start = Date.now()
    showOperationProgress("op", { title: "Working", startedAt: start })
    vi.setSystemTime(start + elapsed)
    showOperationSuccess("op", "Done", { description: "took a while", noticeComputer: computer })
    if (elapsed > LONG_OPERATION_MS) {
      expect(delivered).toHaveBeenCalledExactlyOnceWith({ category: "completions", key: "op", title: "Done", body: "took a while", computer })
    } else {
      expect(delivered).not.toHaveBeenCalled()
    }
  })

  it("does not mirror a quick success, a success without progress, a notice, progress, or a quick confirmation", () => {
    const start = Date.now()
    showOperationProgress("op", { title: "Working", startedAt: start })
    vi.setSystemTime(start + 1000)
    showOperationSuccess("op", "Done")
    showOperationSuccess("other", "Done without progress")
    showOperationNotice("op", "Cancelled")
    showQuickConfirmation("Copied")
    expect(delivered).not.toHaveBeenCalled()
  })

  it("does not mirror when native is false", () => {
    const start = Date.now()
    showOperationProgress("op", { title: "Working", startedAt: start })
    vi.setSystemTime(start + LONG_OPERATION_MS + 1)
    showOperationSuccess("op", "Done", { native: false })
    showOperationFailure("op", "Failed", { native: false })
    showActionFailure("Nope", "x", undefined, { native: false })
    expect(delivered).not.toHaveBeenCalled()
  })
})

describe("backend notices", () => {
  it("renders by category, replaces by key in place, tags the computer, and never re-delivers", async () => {
    render(<Host />)
    act(() => showBackendNotice({ category: "changes", key: "state:vm-1", title: "dev stopped unexpectedly", body: "", computer }))
    await tick()
    expect(screen.getByText("dev stopped unexpectedly")).toBeInTheDocument()
    act(() => showBackendNotice({ category: "changes", key: "state:vm-1", title: "dev is running again", body: "Recovered.", computer }))
    await tick()
    expect(screen.queryByText("dev stopped unexpectedly")).not.toBeInTheDocument()
    expect(screen.getAllByText("dev is running again")).toHaveLength(1)
    act(() => showBackendNotice({ category: "failures", key: "update", title: "Update failed", body: "offline", computer: null }))
    act(() => showBackendNotice({ category: "completions", key: "done", title: "Export finished", body: "", computer: null }))
    await tick()
    expect(screen.getByText("Update failed")).toBeInTheDocument()
    expect(screen.getByText("Export finished")).toBeInTheDocument()
    act(() => dismissComputerToastsById(computer.id))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.queryByText("dev is running again")).not.toBeInTheDocument()
    expect(delivered).not.toHaveBeenCalled()
  })
})

describe("toast computer ownership", () => {
  it.each([false, true])("does not dismiss an unrelated replacement after deleting its old computer (dismissed: %s)", async (dismissed) => {
    render(<Host />)
    const id = `reused-transfer-${dismissed}`
    act(() => showOperationSuccess(id, "Imported A", { computer: "imported-a", persist: true }))
    await tick()
    if (dismissed) {
      act(() => dismissOperationToast(id))
      await tick()
      await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    }
    act(() => showOperationSuccess(id, "Exported B", { action: { label: "Reveal B", onClick: vi.fn() } }))
    await tick()
    expect(screen.getByRole("button", { name: "Reveal B" })).toBeInTheDocument()
    act(() => dismissComputerToasts("imported-a"))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.getByRole("button", { name: "Reveal B" })).toBeInTheDocument()
    dismissOperationToast(id)
  })

  it("replaces ownership when a toast moves to another computer", async () => {
    render(<Host />)
    act(() => showOperationFailure("moved-owner", "Failed A", { computer: "owner-a", native: false }))
    await tick()
    act(() => showOperationFailure("moved-owner", "Failed B", { computer: "owner-b", native: false }))
    await tick()
    act(() => dismissComputerToasts("owner-a"))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.getByText("Failed B")).toBeInTheDocument()
    act(() => dismissComputerToasts("owner-b"))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.queryByText("Failed B")).not.toBeInTheDocument()
  })
})

it("keeps progress and cancellation available when Retry replaces the failure in place", async () => {
  render(<Host />)
  const cancel = vi.fn()
  const retry = () => showOperationProgress("retry-progress", { title: "Retrying operation", computer: "retry-vm", cancel: { onCancel: cancel } })
  act(() => showOperationFailure("retry-progress", "Operation failed", { retry, computer: "retry-vm", native: false }))
  await tick()
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  await tick()
  await act(async () => { await vi.advanceTimersByTimeAsync(500) })
  expect(screen.getByText("Retrying operation")).toBeInTheDocument()
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
  expect(cancel).toHaveBeenCalledOnce()
  act(() => dismissOperationToast("retry-progress"))
})


it("retains computer ownership while Retry waits for backend-driven progress", async () => {
  render(<Host />)
  const retry = vi.fn()
  const onDismiss = vi.fn()
  act(() => showOperationFailure("retry-waiting", "Waiting retry", { retry, onDismiss, computer: "waiting-vm", native: false }))
  await tick()
  fireEvent.click(screen.getByRole("button", { name: "Retry" }))
  await tick()
  expect(retry).toHaveBeenCalledOnce()
  expect(onDismiss).not.toHaveBeenCalled()
  act(() => dismissComputerToasts("waiting-vm"))
  await tick()
  await act(async () => { await vi.advanceTimersByTimeAsync(500) })
  expect(screen.queryByText("Waiting retry")).not.toBeInTheDocument()
  expect(onDismiss).toHaveBeenCalledOnce()
})


it.each(["Retry", "Open"])("clears the previous %s action when a toast becomes progress", async (label) => {
  render(<Host />)
  const id = `progress-clears-${label}`
  const action = vi.fn()
  act(() => {
    if (label === "Retry") showOperationFailure(id, "Failed before retry", { retry: action, native: false })
    else showOperationSuccess(id, "Previous success", { action: { label, onClick: action } })
  })
  await tick()
  expect(screen.getByRole("button", { name: label })).toBeInTheDocument()
  act(() => showOperationProgress(id, { title: "Next operation", cancel: { onCancel: vi.fn() } }))
  await tick()
  expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument()
  expect(screen.queryByRole("button", { name: label })).not.toBeInTheDocument()
  act(() => dismissOperationToast(id))
})


it("acknowledges a result exactly once when its action closes the notification", async () => {
  render(<Host />)
  const onDismiss = vi.fn(() => dismissOperationToast("acknowledge-result"))
  const open = vi.fn()
  act(() => showOperationSuccess("acknowledge-result", "Imported computer", {
    action: { label: "Open imported computer", onClick: open }, onDismiss,
  }))
  await tick()
  fireEvent.click(screen.getByRole("button", { name: "Open imported computer" }))
  await tick()
  await act(async () => { await vi.advanceTimersByTimeAsync(500) })
  expect(open).toHaveBeenCalledOnce()
  expect(onDismiss).toHaveBeenCalledOnce()
  expect(screen.queryByText("Imported computer")).not.toBeInTheDocument()
})

describe("approval notices", () => {
  it("shows a request from an agent as a warning with a Review button, and never re-delivers it", async () => {
    render(<Host />)
    const review = vi.fn()
    const notice = { category: "approvals" as const, key: "approval:ap-1", title: "main asks: Publish port 8080", body: "", computer }
    act(() => showBackendNotice(notice, review))
    await tick()
    expect(screen.getByText("main asks: Publish port 8080")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Review" }))
    expect(review).toHaveBeenCalledExactlyOnceWith(notice)
    expect(delivered).not.toHaveBeenCalled()
  })

  it("replaces the toast of the same request in place and drops it when the request is answered", async () => {
    render(<Host />)
    act(() => showBackendNotice({ category: "approvals", key: "approval:ap-2", title: "main asks: Allow SSH", body: "", computer }))
    act(() => showBackendNotice({ category: "approvals", key: "approval:ap-2", title: "main asks: Allow SSH", body: "", computer }))
    await tick()
    expect(screen.getAllByText("main asks: Allow SSH")).toHaveLength(1)
    act(() => dismissOperationToast("approval:ap-2"))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.queryByText("main asks: Allow SSH")).not.toBeInTheDocument()
  })
})
