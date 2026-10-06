import { act, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"

import { Toaster } from "@/components/ui/sonner"
import { createPreparationStore, PreparationProvider, type PreparationBackend, type PreparationStatus } from "@/desktop/preparation"

import { PREPARATION_HIDE_DELAY_MS, PREPARATION_SHOW_DELAY_MS, PreparationToast } from "./preparation-toast"

const task = (state: PreparationStatus["image"]["state"], extra: Partial<PreparationStatus["image"]> = {}) => ({ state, fraction: null, message: null, retryable: false, ...extra })

function mount(initial: PreparationStatus) {
  let handler: (value: unknown) => void = () => {}
  const backend: PreparationBackend = {
    read: async () => initial,
    retry: vi.fn(async () => initial),
    listen: async next => { handler = next; return () => {} },
  }
  render(<PreparationProvider store={createPreparationStore(backend)}><Toaster /><PreparationToast /></PreparationProvider>)
  return { backend, emit: (value: PreparationStatus) => act(() => handler(value)) }
}

beforeEach(() => { vi.useFakeTimers({ shouldAdvanceTime: true }) })
afterEach(() => { vi.restoreAllMocks(); toast.dismiss(); vi.useRealTimers() })

describe("PreparationToast", () => {
  it("shows nothing when everything is already prepared", async () => {
    mount({ image: task("ready"), lcu: task("ready") })
    await act(async () => { await vi.advanceTimersByTimeAsync(PREPARATION_SHOW_DELAY_MS + PREPARATION_HIDE_DELAY_MS) })
    expect(screen.queryByText("Preparing Silo")).not.toBeInTheDocument()
  })

  it("shows the current item after a short delay and disappears when it is ready", async () => {
    const dismiss = vi.spyOn(toast, "dismiss")
    const { emit } = mount({ image: task("running"), lcu: task("pending") })
    await act(async () => { await vi.advanceTimersByTimeAsync(PREPARATION_SHOW_DELAY_MS + 50) })
    expect(screen.getByText("Preparing Silo")).toBeInTheDocument()
    expect(screen.getAllByText("Preparing the computer image (first time only)").length).toBeGreaterThan(0)
    expect(dismiss).not.toHaveBeenCalledWith("preparation")
    emit({ image: task("ready"), lcu: task("ready") })
    await act(async () => { await vi.advanceTimersByTimeAsync(PREPARATION_HIDE_DELAY_MS + 50) })
    expect(dismiss).toHaveBeenCalledWith("preparation")
  })

  it("never flashes for work that finishes within the delay", async () => {
    const { emit } = mount({ image: task("pending"), lcu: task("pending") })
    emit({ image: task("running"), lcu: task("pending") })
    await act(async () => { await vi.advanceTimersByTimeAsync(100) })
    emit({ image: task("ready"), lcu: task("ready") })
    await act(async () => { await vi.advanceTimersByTimeAsync(PREPARATION_SHOW_DELAY_MS + PREPARATION_HIDE_DELAY_MS) })
    expect(screen.queryByText("Preparing Silo")).not.toBeInTheDocument()
  })

  it("shows a short message with Retry when something fails", async () => {
    const { backend } = mount({ image: task("ready"), lcu: task("failed", { message: "Silo could not download the computer use tools. Check your connection, then retry.", retryable: true }) })
    expect(await screen.findByText("Silo could not finish preparing")).toBeInTheDocument()
    expect(screen.getByText(/Silo could not download the computer use tools/)).toBeInTheDocument()
    await userEvent.setup({ advanceTimers: vi.advanceTimersByTime }).click(screen.getByRole("button", { name: "Retry" }))
    expect(backend.retry).toHaveBeenCalledOnce()
  })
})
