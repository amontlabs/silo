import { ComputerUseProvider } from "./computer-use-provider"
import { Profiler } from "react"
import { act, fireEvent, render, renderHook, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { ComputerUseSettings } from "@/features/application/components/computer-use-settings"
import { applicationSourceForScenario } from "@/fixtures/application-scenarios"
import { createFixtureComputerUseBackend, fixtureDesktopState } from "@/fixtures/computer-use"
import { createComputerUseBridge, useChatGptApp, type ComputerUseBackend } from "./computer-use-bridge"
import { ComputerUseSection } from "./computer-use-panel"

const computer = "silo-remote:11111111-1111-4111-8111-111111111111:33333333-3333-4333-8333-333333333333"
const backend = (overrides: Partial<ComputerUseBackend> = {}): ComputerUseBackend => ({
  ...createFixtureComputerUseBackend("ready", "ready"),
  listenStatus: async () => () => {},
  ...overrides,
})
const advance = async (ms: number) => { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
function visibility(hidden: boolean) {
  vi.spyOn(document, "visibilityState", "get").mockReturnValue(hidden ? "hidden" : "visible")
  act(() => { document.dispatchEvent(new Event("visibilitychange")) })
}
function section(b: ComputerUseBackend, active = true) {
  const bridge = createComputerUseBridge(b, { busy: 1000, idle: 1000 })
  const page = (visible: boolean) => <ComputerUseProvider bridge={bridge}><ComputerUseSection computer={computer} active={visible} /></ComputerUseProvider>
  const view = render(page(active))
  return { ...view, setActive: (visible: boolean) => view.rerender(page(visible)) }
}
beforeEach(() => { vi.useFakeTimers(); vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible") })
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

it("reads remote download status without waiting for local event registration", async () => {
  const listen = vi.fn(() => new Promise<() => void>(() => {}))
  const read = vi.fn(async () => ({ state: "downloading", receivedBytes: 1, totalBytes: 10 }))
  const store = createComputerUseBridge(backend({ chatGptStatus: read, listenStatus: listen }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  const stop = store.subscribe(() => {})
  try {
    await advance(0)
    expect(read).toHaveBeenCalledWith("office")
    expect(store.getSnapshot().status).toMatchObject({ state: "downloading" })
    await advance(1000)
    expect(read).toHaveBeenCalledTimes(2)
    expect(listen).not.toHaveBeenCalled()
    stop()
    await advance(1000)
    expect(read).toHaveBeenCalledTimes(2)
  } finally { stop() }
})

it("does not commit the computer-use panel for equal reads but shows changed approval", async () => {
  let state = fixtureDesktopState("ready")
  const read = vi.fn(async () => structuredClone(state))
  const bridge = createComputerUseBridge(backend({ readDesktopState: read }))
  const commits = vi.fn()
  const view = render(<ComputerUseProvider bridge={bridge}><Profiler id="computer-use" onRender={commits}>
    <ComputerUseSection computer={computer} />
  </Profiler></ComputerUseProvider>)
  try {
    await advance(0)
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).not.toBeChecked()
    commits.mockClear()
    for (let tick = 0; tick < 10; tick++) await advance(5000)
    expect(read).toHaveBeenCalledTimes(11)
    expect(commits).not.toHaveBeenCalled()
    state = fixtureDesktopState("auto")
    await advance(5000)
    expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeChecked()
    expect(commits).toHaveBeenCalledOnce()
  } finally { view.unmount() }
})

it("keeps remote download consumers stable until progress or a read error changes", async () => {
  let receivedBytes = 1
  let unavailable = false
  const read = vi.fn(async () => {
    if (unavailable) throw new Error("Device disconnected")
    return { state: "downloading", receivedBytes, totalBytes: 10 }
  })
  const store = createComputerUseBridge(backend({ chatGptStatus: read }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  let renders = 0
  const view = renderHook(() => { renders++; return useChatGptApp(store) })
  try {
    await advance(0)
    const first = view.result.current
    const initialRenders = renders
    for (let tick = 0; tick < 10; tick++) await advance(1000)
    expect(read).toHaveBeenCalledTimes(11)
    expect(renders - initialRenders).toBe(0)
    expect(view.result.current).toBe(first)

    receivedBytes = 2
    await advance(1000)
    expect(view.result.current.status).toMatchObject({ receivedBytes: 2 })
    unavailable = true
    await advance(1000)
    expect(view.result.current.loadError).toBe("Device disconnected")
    const errorRenders = renders
    await advance(2000)
    expect(renders).toBe(errorRenders)
    unavailable = false
    await advance(4000)
    expect(view.result.current.loadError).toBeNull()
    expect(view.result.current.status).toMatchObject({ receivedBytes: 2 })
  } finally { view.unmount() }
})

it("backs off failed remote download reads to a cap and restores polling after recovery", async () => {
  const read = vi.fn(async (): Promise<unknown> => { throw new Error("Device disconnected") })
  const store = createComputerUseBridge(backend({ chatGptStatus: read }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  const stop = store.subscribe(() => {})
  try {
    await advance(0)
    expect(read).toHaveBeenCalledOnce()
    for (const delay of [2000, 4000, 8000, 16000, 30000, 30000]) {
      const calls = read.mock.calls.length
      await advance(delay - 1)
      expect(read).toHaveBeenCalledTimes(calls)
      await advance(1)
      expect(read).toHaveBeenCalledTimes(calls + 1)
    }
    expect(store.getSnapshot().loadError).toBe("Device disconnected")
    read.mockResolvedValue({ state: "idle" })
    await advance(30000)
    expect(store.getSnapshot().loadError).toBeNull()
    const calls = read.mock.calls.length
    await advance(999)
    expect(read).toHaveBeenCalledTimes(calls)
    await advance(1)
    expect(read).toHaveBeenCalledTimes(calls + 1)
    stop()
    await advance(60000)
    expect(read).toHaveBeenCalledTimes(calls + 1)
  } finally { stop() }
})

it("backs off malformed remote download status and restores polling after recovery", async () => {
  const read = vi.fn(async (): Promise<unknown> => null)
  const store = createComputerUseBridge(backend({ chatGptStatus: read }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  const stop = store.subscribe(() => {})
  try {
    await advance(0)
    expect(store.getSnapshot().loadError).toBe("Silo could not read the ChatGPT for Linux status.")
    for (const delay of [2000, 4000, 8000, 16000, 30000]) {
      const calls = read.mock.calls.length
      await advance(delay - 1)
      expect(read).toHaveBeenCalledTimes(calls)
      await advance(1)
      expect(read).toHaveBeenCalledTimes(calls + 1)
    }
    read.mockResolvedValue({ state: "idle" })
    await advance(30000)
    expect(store.getSnapshot().loadError).toBeNull()
    const calls = read.mock.calls.length
    await advance(999)
    expect(read).toHaveBeenCalledTimes(calls)
    await advance(1)
    expect(read).toHaveBeenCalledTimes(calls + 1)
  } finally { stop() }
})

it("backs off failed computer-use state reads and stops a pending schedule when inactive", async () => {
  const read = vi.fn(async (): Promise<unknown> => { throw new Error("Computer unavailable") })
  const view = section(backend({ readDesktopState: read }))
  await advance(0)
  expect(read).toHaveBeenCalledOnce()
  for (const delay of [10000, 20000, 30000, 30000]) {
    const calls = read.mock.calls.length
    await advance(delay - 1)
    expect(read).toHaveBeenCalledTimes(calls)
    await advance(1)
    expect(read).toHaveBeenCalledTimes(calls + 1)
  }
  read.mockResolvedValue(fixtureDesktopState("ready"))
  await advance(30000)
  const calls = read.mock.calls.length
  await advance(4999)
  expect(read).toHaveBeenCalledTimes(calls)
  await advance(1)
  expect(read).toHaveBeenCalledTimes(calls + 1)
  view.setActive(false)
  await advance(60000)
  expect(read).toHaveBeenCalledTimes(calls + 1)
})

it("pauses an inactive computer-use section and refreshes once on return", async () => {
  const read = vi.fn(async () => fixtureDesktopState("ready"))
  const view = section(backend({ readDesktopState: read }), false)
  await advance(15_000)
  expect(read).not.toHaveBeenCalled()
  view.setActive(true)
  await advance(0)
  expect(read).toHaveBeenCalledOnce()
  view.setActive(false)
  await advance(15_000)
  expect(read).toHaveBeenCalledOnce()
  view.setActive(true)
  await advance(0)
  expect(read).toHaveBeenCalledTimes(2)
})

it("pauses document-hidden computer-use reads and refreshes once when visible", async () => {
  visibility(true)
  const read = vi.fn(async () => fixtureDesktopState("ready"))
  const view = section(backend({ readDesktopState: read }))
  await advance(15_000)
  expect(read).not.toHaveBeenCalled()
  visibility(false)
  await advance(0)
  expect(read).toHaveBeenCalledOnce()
  visibility(true)
  await advance(15_000)
  expect(read).toHaveBeenCalledOnce()
  view.setActive(false)
  visibility(false)
  await advance(0)
  expect(read).toHaveBeenCalledOnce()
})

it("lets an explicit approval change finish while the section and document are hidden", async () => {
  let finish!: (value: unknown) => void
  const work = vi.fn(() => new Promise(resolve => { finish = resolve }))
  const read = vi.fn(async () => fixtureDesktopState("ready"))
  const view = section(backend({ readDesktopState: read, setApproval: work }))
  await advance(0)
  fireEvent.click(screen.getByRole("switch", { name: /Allow without asking/ }))
  expect(work).toHaveBeenCalledOnce()
  view.setActive(false)
  visibility(true)
  await act(async () => { finish(fixtureDesktopState("ready")) })
  expect(screen.getByRole("switch", { name: /Allow without asking/ })).toBeEnabled()
  await advance(15_000)
  expect(read).toHaveBeenCalledOnce()
})

it("releases remote download polling while its computer-use section is inactive", async () => {
  const read = vi.fn(async () => ({ state: "downloading", receivedBytes: 1, totalBytes: 10 }))
  const view = section(backend({ readDesktopState: async () => fixtureDesktopState("app-failed"), chatGptStatus: read }))
  await advance(0)
  expect(read).toHaveBeenCalledOnce()
  view.setActive(false)
  await advance(15_000)
  expect(read).toHaveBeenCalledOnce()
  view.setActive(true)
  await advance(0)
  expect(read).toHaveBeenCalledTimes(2)
})

it("pauses remote download status polling when hidden and still settles an explicit retry", async () => {
  const read = vi.fn(async () => ({ state: "downloading", receivedBytes: 1, totalBytes: 10 }))
  let finish!: () => void
  const retry = vi.fn(() => new Promise<void>(resolve => { finish = resolve }))
  const store = createComputerUseBridge(backend({ chatGptStatus: read, retry }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  const stop = store.subscribe(() => {})
  try {
    await advance(0)
    expect(read).toHaveBeenCalledOnce()
    visibility(true)
    await advance(15_000)
    expect(read).toHaveBeenCalledOnce()
    const pending = store.retry()
    expect(store.getSnapshot().busy).toBe(true)
    finish()
    await pending
    expect(store.getSnapshot().busy).toBe(false)
    expect(read).toHaveBeenCalledTimes(2)
    visibility(false)
    await advance(0)
    expect(read).toHaveBeenCalledTimes(3)
    stop()
    visibility(true)
    visibility(false)
    await advance(15_000)
    expect(read).toHaveBeenCalledTimes(3)
  } finally { stop() }
})

it("releases remote download polling while computer use settings are inactive", async () => {
  const read = vi.fn(async (_device?: string) => ({ state: "downloading", receivedBytes: 1, totalBytes: 10 }))
  const bridge = createComputerUseBridge(backend({ chatGptStatus: read }), { busy: 1000, idle: 1000 })
  const source = { ...applicationSourceForScenario("running"), devices: [{ id: "office", name: "Office", address: "owner@office", connected: true }] }
  const page = (active: boolean) => <ComputerUseProvider bridge={bridge}><ComputerUseSettings source={source} active={active} /></ComputerUseProvider>
  const view = render(page(true))
  await advance(0)
  const reads = () => read.mock.calls.filter(([device]) => device === "office")
  expect(reads()).toHaveLength(1)
  view.rerender(page(false))
  await advance(15_000)
  expect(reads()).toHaveLength(1)
  view.rerender(page(true))
  await advance(0)
  expect(reads()).toHaveLength(2)
})

it("keeps one remote polling schedule when visibility returns during a read", async () => {
  let finish!: (value: unknown) => void
  const read = vi.fn(async (_device?: string): Promise<unknown> => ({ state: "idle" }))
  read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  const store = createComputerUseBridge(backend({ chatGptStatus: read }), { busy: 1000, idle: 1000 }).chatGptFor("office")
  const stop = store.subscribe(() => {})
  try {
    await advance(0)
    visibility(true)
    visibility(false)
    await advance(0)
    await act(async () => { finish({ state: "idle" }) })
    expect(read).toHaveBeenCalledTimes(2)
    await advance(1000)
    expect(read).toHaveBeenCalledTimes(3)
  } finally { stop() }
})
