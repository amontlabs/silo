import { ComputerUseProvider } from "./computer-use-provider"
import { Profiler } from "react"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { createFixtureComputerUseBackend, fixtureDesktopState } from "@/fixtures/computer-use"
import { createComputerUseBridge, type ComputerUseBackend } from "./computer-use-bridge"
import { ComputerUseSection } from "./computer-use-panel"

const computer = "silo-remote:11111111-1111-4111-8111-111111111111:33333333-3333-4333-8333-333333333333"
const backend = (overrides: Partial<ComputerUseBackend> = {}): ComputerUseBackend => ({
  ...createFixtureComputerUseBackend("ready"),
  ...overrides,
})
const advance = async (ms: number) => { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
function visibility(hidden: boolean) {
  vi.spyOn(document, "visibilityState", "get").mockReturnValue(hidden ? "hidden" : "visible")
  act(() => { document.dispatchEvent(new Event("visibilitychange")) })
}
function section(b: ComputerUseBackend, active = true) {
  const bridge = createComputerUseBridge(b)
  const page = (visible: boolean) => <ComputerUseProvider bridge={bridge}><ComputerUseSection computer={computer} active={visible} /></ComputerUseProvider>
  const view = render(page(active))
  return { ...view, setActive: (visible: boolean) => view.rerender(page(visible)) }
}
beforeEach(() => { vi.useFakeTimers(); vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible") })
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })


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




