import { act, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { createPreparationStore, describePreparation, PreparationProvider, preparationStatusSchema, usePreparationStatus, type PreparationBackend, type PreparationStatus } from "./preparation"

const task = (state: PreparationStatus["image"]["state"], extra: Partial<PreparationStatus["image"]> = {}) => ({ state, fraction: null, message: null, retryable: false, ...extra })
const status = (image: PreparationStatus["image"]): PreparationStatus => ({ image })

describe("describePreparation", () => {
  it("lists nothing when the image is ready or only waiting", () => {
    expect(describePreparation(status(task("ready")))).toEqual([])
    expect(describePreparation(status(task("pending")))).toEqual([])
    expect(describePreparation(null)).toEqual([])
  })

  it("names the image download with its progress", () => {
    const items = describePreparation(status(task("running", { fraction: 25 })))
    expect(items.map(item => item.text)).toEqual(["Downloading the computer image · 25%"])
    expect(items.map(item => item.progress)).toEqual([0.25])
  })

  it("says the image is being prepared once its download is over", () => {
    const items = describePreparation(status(task("running")))
    expect(items.map(item => item.text)).toEqual(["Preparing the computer image (first time only)"])
    expect(items[0].progress).toBeNull()
  })

  it("reports a failure with its message and whether it can be retried", () => {
    const items = describePreparation(status(task("failed", { message: "Check your network.", retryable: true })))
    expect(items).toMatchObject([{ id: "image", state: "failed", text: "Check your network.", retryable: true }])
  })
})

describe("preparation store", () => {
  function backend(initial: unknown) {
    let handler: (value: unknown) => void = () => {}
    const native: PreparationBackend = {
      read: vi.fn(async () => initial),
      retry: vi.fn(async () => status(task("running"))),
      listen: vi.fn(async next => { handler = next; return () => {} }),
    }
    return { native, emit: (value: unknown) => handler(value) }
  }

  it("reads the status once events are heard and follows later events", async () => {
    const { native, emit } = backend(status(task("running")))
    const store = createPreparationStore(native)
    const unsubscribe = store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot()?.image.state).toBe("running"))
    act(() => emit(status(task("ready"))))
    expect(store.getSnapshot()?.image.state).toBe("ready")
    unsubscribe()
  })

  it("ignores a payload it cannot read", async () => {
    const { native, emit } = backend(status(task("ready")))
    const store = createPreparationStore(native)
    store.subscribe(() => {})
    await vi.waitFor(() => expect(store.getSnapshot()).not.toBeNull())
    emit({ image: "broken" })
    expect(store.getSnapshot()?.image.state).toBe("ready")
  })

  it("accepts the native payload shape", () => {
    expect(preparationStatusSchema.parse({ image: { state: "failed", fraction: null, message: "x", retryable: true } }).image.retryable).toBe(true)
  })

  it("exposes readiness to any component", async () => {
    const { native } = backend(status(task("ready")))
    const store = createPreparationStore(native)
    function Probe() {
      const state = usePreparationStatus()
      return <p>{state.ready ? "ready" : "preparing"}</p>
    }
    render(<PreparationProvider store={store}><Probe /></PreparationProvider>)
    expect(screen.getByText("preparing")).toBeInTheDocument()
    expect(await screen.findByText("ready")).toBeInTheDocument()
  })
})
