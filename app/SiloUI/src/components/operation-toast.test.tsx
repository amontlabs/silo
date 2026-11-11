import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { Toaster } from "@/components/ui/sonner"
import { SettingsProvider } from "@/features/preferences/settings-store"
import { formatElapsed } from "@/lib/format-elapsed"
import { dismissComputerToasts, showOperationFailure, showOperationProgress, showOperationSuccess, useOperationProgressToast, type OperationProgressState } from "@/lib/operation-toast"

function Host() { return <SettingsProvider initialSettings={{ theme: "light" }}><Toaster /></SettingsProvider> }
const tick = () => act(async () => { await Promise.resolve(); await vi.advanceTimersByTimeAsync(50) })

beforeEach(() => { vi.useFakeTimers({ now: new Date("2026-10-02T12:00:00Z") }) })
afterEach(() => { vi.useRealTimers() })

describe("showOperationProgress", () => {
  it("renders determinate progress, step, steps and elapsed time", async () => {
    render(<Host />)
    act(() => showOperationProgress("op", { title: "Importing", step: "Copying disk", progress: 0.4, startedAt: Date.now() - 72_000, steps: [{ label: "Verify", state: "done" }, { label: "Copy", state: "current" }, { label: "Boot", state: "pending" }] }))
    await tick()
    expect(screen.getByText("Importing")).toBeInTheDocument()
    const bar = screen.getByRole("progressbar", { name: "Copying disk" })
    expect(bar).toHaveAttribute("aria-valuenow", "40")
    expect(screen.getByRole("list", { name: "Steps" }).querySelectorAll("li")).toHaveLength(3)
    expect(screen.getByText("1m 12s")).toBeInTheDocument()
    await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
    expect(screen.getByText("1m 17s")).toBeInTheDocument()
  })

  it("renders an indeterminate bar and updates in place", async () => {
    render(<Host />)
    act(() => showOperationProgress("op", { title: "Working", progress: null }))
    await tick()
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow")
    act(() => showOperationProgress("op", { title: "Working", step: "Next", progress: 0.5 }))
    await tick()
    expect(screen.getAllByText("Working")).toHaveLength(1)
    expect(screen.getByRole("progressbar", { name: "Next" })).toHaveAttribute("aria-valuenow", "50")
  })

  it("cancel without confirm calls onCancel directly", async () => {
    const onCancel = vi.fn()
    render(<Host />)
    act(() => showOperationProgress("op", { title: "Working", cancel: { onCancel } }))
    await tick()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(onCancel).toHaveBeenCalledOnce()
  })

  it("cancel with confirm shows an inline confirmation that survives updates", async () => {
    const onCancel = vi.fn()
    const cancel = { confirm: { prompt: "Remove the incomplete computer?", confirmLabel: "Remove" }, onCancel }
    render(<Host />)
    act(() => showOperationProgress("op", { title: "Working", cancel }))
    await tick()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(screen.getByText("Remove the incomplete computer?")).toBeInTheDocument()
    expect(onCancel).not.toHaveBeenCalled()
    act(() => showOperationProgress("op", { title: "Working", progress: 0.6, cancel }))
    await tick()
    expect(screen.getByText("Remove the incomplete computer?")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Keep going" }))
    expect(screen.queryByText("Remove the incomplete computer?")).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    fireEvent.click(screen.getByRole("button", { name: "Remove" }))
    expect(onCancel).toHaveBeenCalledOnce()
  })

  it("success and failure replace the progress body", async () => {
    render(<Host />)
    act(() => showOperationProgress("op", { title: "Working", cancel: { onCancel: vi.fn() } }))
    await tick()
    act(() => showOperationSuccess("op", "Done"))
    await tick()
    expect(screen.getByText("Done")).toBeInTheDocument()
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument()
    expect(screen.queryByText("Working")).not.toBeInTheDocument()
    act(() => showOperationProgress("op", { title: "Again" }))
    await tick()
    act(() => showOperationFailure("op", "Failed", { retry: vi.fn() }))
    await tick()
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument()
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument()
  })
})

describe("useOperationProgressToast", () => {
  function Hooked({ state }: { state: OperationProgressState }) { useOperationProgressToast("hook", state); return null }

  it("ignores finished states at mount and reacts to transitions", async () => {
    const { rerender } = render(<><Host /><Hooked state={{ status: "success", title: "Old result" }} /></>)
    await tick()
    expect(screen.queryByText("Old result")).not.toBeInTheDocument()
    rerender(<><Host /><Hooked state={{ status: "running", title: "Pushing", progress: 0.2 }} /></>)
    await tick()
    expect(screen.getByText("Pushing")).toBeInTheDocument()
    rerender(<><Host /><Hooked state={{ status: "failure", title: "Push failed" }} /></>)
    await tick()
    expect(screen.getByText("Push failed")).toBeInTheDocument()
    expect(screen.queryByText("Pushing")).not.toBeInTheDocument()
  })
})

it("formats elapsed time", () => {
  expect(formatElapsed(5000)).toBe("5s")
  expect(formatElapsed(72_000)).toBe("1m 12s")
  expect(formatElapsed(3_720_000)).toBe("1h 2m")
})

describe("progress notification polish", () => {
  it("reveals complete checklist labels in every step state", async () => {
    const steps = (["done", "current", "pending", "failed"] as const).map(state => ({ state, label: `${state}: ${"long operation step ".repeat(20)}`.trim() }))
    render(<Host />)
    act(() => showOperationProgress("long-checklist", { title: "Working", steps }))
    await tick()
    for (const { label } of steps) expect(screen.getByText(label)).toHaveAttribute("title", label)
  })

  it("hides a step that only repeats the title", async () => {
    render(<Host />)
    act(() => showOperationProgress("dup", { title: "Creating checkpoint “X”", step: "Creating checkpoint…", progress: null }))
    await tick()
    expect(screen.queryByText("Creating checkpoint…")).not.toBeInTheDocument()
    act(() => showOperationProgress("dup", { title: "Creating checkpoint “X”", step: "Saving disk copies", progress: null }))
    await tick()
    expect(screen.getByText("Saving disk copies")).toBeInTheDocument()
  })

  it("renders the indeterminate bar as a sliding segment, not a static half bar", async () => {
    render(<Host />)
    act(() => showOperationProgress("slide", { title: "Working", progress: null }))
    await tick()
    const indicator = document.querySelector("[data-slot=progress-indicator]")!
    expect(indicator.className).toContain("silo-progress-indeterminate")
    expect(indicator.className).not.toContain("w-1/2")
    expect(document.querySelector("[data-slot=progress]")!.className).toContain("w-full")
  })

  it("keeps the body within the toast width and truncates long steps", async () => {
    render(<Host />)
    act(() => showOperationProgress("wide", { title: "Exporting", step: "x".repeat(300), progress: 0.5 }))
    await tick()
    const step = screen.getByText("x".repeat(300))
    expect(step.className).toContain("truncate")
    expect(step.closest("div.grid")!.className).toContain("min-w-0")
    expect(step.closest("div.grid")!.className).toContain("w-full")
  })

  it("puts Cancel in a small outline button", async () => {
    render(<Host />)
    act(() => showOperationProgress("cancel-style", { title: "Working", cancel: { onCancel: vi.fn() } }))
    await tick()
    const button = screen.getByRole("button", { name: "Cancel" })
    expect(button).toHaveAttribute("data-size", "xs")
    expect(button).toHaveAttribute("data-variant", "outline")
  })
})

describe("toast actions", () => {
  it("renders every action through the same Sonner action button", async () => {
    render(<Host />)
    act(() => {
      showOperationSuccess("a", "Fork created", { action: { label: "Open", onClick: vi.fn() } })
      showOperationSuccess("b", "Exported", { action: { label: "Show in Finder", onClick: vi.fn() } })
      showOperationFailure("c", "Failed", { retry: vi.fn() })
    })
    await tick()
    for (const name of ["Open", "Show in Finder", "Retry"]) {
      const button = screen.getByRole("button", { name })
      expect(button).toHaveAttribute("data-button", "true")
      expect(button.getAttribute("data-slot")).toBeNull()
    }
  })

  it("dismisses notifications tagged with a deleted computer", async () => {
    render(<Host />)
    act(() => {
      showOperationSuccess("tagged", "Imported gone", { computer: "gone", action: { label: "Open", onClick: vi.fn() } })
      showOperationSuccess("other", "Imported kept", { computer: "kept" })
    })
    await tick()
    expect(screen.getByText("Imported gone")).toBeInTheDocument()
    act(() => dismissComputerToasts("gone"))
    await tick()
    await act(async () => { await vi.advanceTimersByTimeAsync(500) })
    expect(screen.queryByText("Imported gone")).not.toBeInTheDocument()
    expect(screen.getByText("Imported kept")).toBeInTheDocument()
  })
})

describe("notification persistence", () => {
  const settle = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms) })

  it("auto-dismisses a quick success without an action after 4 s", async () => {
    render(<Host />)
    act(() => showOperationProgress("quick", { title: "Adding port", progress: null }))
    await tick()
    act(() => showOperationSuccess("quick", "Port 8000 added"))
    await tick()
    expect(screen.getByText("Port 8000 added")).toBeInTheDocument()
    await settle(3000)
    expect(screen.getByText("Port 8000 added")).toBeInTheDocument()
    await settle(2500)
    expect(screen.queryByText("Port 8000 added")).not.toBeInTheDocument()
  })

  it("keeps a success with an action for 20 s", async () => {
    render(<Host />)
    act(() => showOperationSuccess("act", "Exported", { action: { label: "Show in Finder", onClick: vi.fn() } }))
    await tick()
    await settle(15_000)
    expect(screen.getByText("Exported")).toBeInTheDocument()
    await settle(6_000)
    expect(screen.queryByText("Exported")).not.toBeInTheDocument()
  })

  it("keeps the success of a long operation for 20 s", async () => {
    render(<Host />)
    act(() => showOperationProgress("long", { title: "Creating checkpoint", progress: null }))
    await tick()
    await settle(3500)
    act(() => showOperationSuccess("long", "Checkpoint created"))
    await tick()
    await settle(15_000)
    expect(screen.getByText("Checkpoint created")).toBeInTheDocument()
    await settle(6_000)
    expect(screen.queryByText("Checkpoint created")).not.toBeInTheDocument()
  })

  it("honours an explicit persist override for 20 s", async () => {
    render(<Host />)
    act(() => showOperationSuccess("forced", "Pushed 1 commit", { persist: true }))
    await tick()
    await settle(15_000)
    expect(screen.getByText("Pushed 1 commit")).toBeInTheDocument()
    await settle(6_000)
    expect(screen.queryByText("Pushed 1 commit")).not.toBeInTheDocument()
  })

  it("keeps a failure until closed", async () => {
    render(<Host />)
    act(() => showOperationProgress("fail", { title: "Adding port", progress: null }))
    await tick()
    act(() => showOperationFailure("fail", "Could not add port 8000"))
    await tick()
    await settle(15_000)
    expect(screen.getByText("Could not add port 8000")).toBeInTheDocument()
  })
})
