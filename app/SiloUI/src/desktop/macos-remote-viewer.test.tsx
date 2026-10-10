import { act, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const rfbs = vi.hoisted(() => [] as FakeRfb[])
class FakeRfb {
  listeners = new Map<string, Set<(event: CustomEvent) => void>>()
  scaleViewport = false
  resizeSession = true
  clipViewport = true
  showDotCursor = true
  focusOnClick = false
  disconnected = 0
  sendCredentials = vi.fn()
  approveServer = vi.fn()
  focus = vi.fn()
  target: HTMLElement
  url: string
  options: { credentials: { username: string; password: string }; shared: boolean }
  constructor(target: HTMLElement, url: string, options: { credentials: { username: string; password: string }; shared: boolean }) {
    this.target = target
    this.url = url
    this.options = options
    rfbs.push(this)
  }
  addEventListener(type: string, listener: (event: CustomEvent) => void) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set())
    this.listeners.get(type)!.add(listener)
  }
  removeEventListener(type: string, listener: (event: CustomEvent) => void) { this.listeners.get(type)?.delete(listener) }
  disconnect() { this.disconnected += 1 }
  emit(type: string, detail: unknown = {}) { for (const listener of [...(this.listeners.get(type) ?? [])]) listener(new CustomEvent(type, { detail })) }
  listenerCount() { return [...this.listeners.values()].reduce((sum, set) => sum + set.size, 0) }
}
vi.mock("@novnc/novnc", () => ({ default: class { constructor(...args: ConstructorParameters<typeof FakeRfb>) { return new FakeRfb(...args) } } }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }))

import { MacosRemoteViewer } from "./macos-remote-viewer"
import type { MacosDisplaySession } from "./macos-remote-viewer-state"

const SECRET = "s3cret-pass"
const session: MacosDisplaySession = { url: "ws://127.0.0.1:9000/token", username: "silo-user", password: SECRET, width: 1280, height: 800 }

let resizeCallback: (() => void) | null = null
let observerDisconnects = 0
let size = { width: 1000, height: 700 }

beforeEach(() => {
  vi.useFakeTimers()
  rfbs.length = 0
  resizeCallback = null
  observerDisconnects = 0
  size = { width: 1000, height: 700 }
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resizeCallback = callback }
    observe() {}
    disconnect() { observerDisconnects += 1 }
  })
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => size.width)
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(() => size.height)
  vi.stubGlobal("devicePixelRatio", 2)
})
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

async function flush() { await act(async () => { await Promise.resolve(); await Promise.resolve() }) }
async function advance(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

type Size = { widthPx: number; heightPx: number }
function mount(getSession: () => Promise<MacosDisplaySession> = () => Promise.resolve(session), resize: (size: Size) => Promise<unknown> = () => Promise.resolve({})) {
  return render(<MacosRemoteViewer name="Build Mac" device="Studio" getSession={getSession} resize={resize} onFullscreen={vi.fn()} />)
}

describe("macOS remote viewer", () => {
  it("connects with the session credentials and keeps them out of the page", async () => {
    const { container } = mount()
    expect(screen.getByRole("status")).toHaveTextContent("Connecting")
    await flush()
    expect(rfbs).toHaveLength(1)
    const rfb = rfbs[0]
    expect(rfb.url).toBe(session.url)
    expect(rfb.options).toEqual({ credentials: { username: "silo-user", password: SECRET }, shared: false })
    expect(rfb).toMatchObject({ scaleViewport: true, resizeSession: false, clipViewport: false, showDotCursor: false, focusOnClick: true })
    act(() => rfb.emit("connect"))
    await flush()
    expect(screen.getByRole("status")).toHaveTextContent("Connected")
    expect(screen.getByRole("heading")).toHaveTextContent("Build Mac · Studio")
    expect(container.innerHTML).not.toContain(SECRET)
    expect(document.body.textContent).not.toContain(SECRET)
  })

  it("answers credentialsrequired and approves the server", async () => {
    mount()
    await flush()
    act(() => { rfbs[0].emit("serververification"); rfbs[0].emit("credentialsrequired") })
    expect(rfbs[0].approveServer).toHaveBeenCalled()
    expect(rfbs[0].sendCredentials).toHaveBeenCalledWith({ username: "silo-user", password: SECRET })
  })

  it("reports an authentication failure without retrying until Reconnect", async () => {
    const getSession = vi.fn(() => Promise.resolve(session))
    mount(getSession)
    await flush()
    act(() => { rfbs[0].emit("securityfailure", { status: 1, reason: "bad" }); rfbs[0].emit("disconnect", { clean: false }) })
    expect(screen.getByRole("alert")).toHaveTextContent("rejected the sign-in")
    expect(screen.getByRole("status")).toHaveTextContent("Authentication failed")
    await advance(60000)
    expect(getSession).toHaveBeenCalledTimes(1)
    await act(async () => { screen.getByRole("button", { name: "Reconnect" }).click() })
    await flush()
    expect(getSession).toHaveBeenCalledTimes(2)
    expect(rfbs).toHaveLength(2)
    expect(rfbs[0].disconnected).toBe(1)
  })

  it("shows non-transient session errors with a manual reconnect and no retry", async () => {
    const getSession = vi.fn(() => Promise.reject("The computer is not running"))
    mount(getSession)
    await flush()
    expect(screen.getByRole("alert")).toHaveTextContent("The computer is not running")
    expect(screen.getByRole("status")).toHaveTextContent("Unavailable")
    await advance(60000)
    expect(getSession).toHaveBeenCalledTimes(1)
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeInTheDocument()
  })

  it("retries a transient session error with backoff", async () => {
    const getSession = vi.fn().mockRejectedValueOnce("The owner is offline").mockResolvedValue(session)
    mount(getSession)
    await flush()
    expect(screen.getByRole("status")).toHaveTextContent("Reconnecting (attempt 1, in 1s)")
    await advance(1000)
    expect(getSession).toHaveBeenCalledTimes(2)
    expect(rfbs).toHaveLength(1)
  })

  it("reconnects with backoff after an unclean disconnect and gives up after eight attempts", async () => {
    const getSession = vi.fn(() => Promise.resolve(session))
    mount(getSession)
    await flush()
    act(() => { rfbs[0].emit("connect"); rfbs[0].emit("disconnect", { clean: false }) })
    expect(screen.getByRole("status")).toHaveTextContent("attempt 1, in 1s")
    const delays = [1000, 2000, 4000, 8000, 15000, 15000, 15000, 15000]
    for (let attempt = 0; attempt < 8; attempt++) {
      await advance(delays[attempt] - 1)
      expect(getSession).toHaveBeenCalledTimes(attempt + 1)
      await advance(1)
      expect(getSession).toHaveBeenCalledTimes(attempt + 2)
      act(() => rfbs[attempt + 1].emit("disconnect", { clean: false }))
    }
    expect(screen.getByRole("status")).toHaveTextContent("Disconnected")
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeInTheDocument()
    await advance(60000)
    expect(getSession).toHaveBeenCalledTimes(9)
  })

  it("does not retry a clean disconnect or a failure before connecting", async () => {
    const getSession = vi.fn(() => Promise.resolve(session))
    mount(getSession)
    await flush()
    act(() => rfbs[0].emit("disconnect", { clean: false }))
    expect(screen.getByRole("status")).toHaveTextContent("Disconnected")
    await advance(60000)
    expect(getSession).toHaveBeenCalledTimes(1)
  })

  it("disposes the previous connection on reconnect and unmount", async () => {
    const { unmount } = mount()
    await flush()
    const first = rfbs[0]
    act(() => { first.emit("connect"); first.emit("disconnect", { clean: true }) })
    await act(async () => { screen.getByRole("button", { name: "Reconnect" }).click() })
    await flush()
    expect(first.disconnected).toBe(1)
    expect(first.listenerCount()).toBe(0)
    const second = rfbs[1]
    unmount()
    expect(second.disconnected).toBe(1)
    expect(second.listenerCount()).toBe(0)
    expect(observerDisconnects).toBe(1)
  })

  it("sends the container size once connected, then debounced, serialised changes", async () => {
    const calls: Size[] = []
    const releases: Array<() => void> = []
    const resize = vi.fn((target: Size) => {
      calls.push(target)
      return new Promise<void>(resolve => releases.push(resolve))
    })
    mount(undefined, resize)
    await flush()
    resizeCallback!()
    await advance(400)
    expect(resize).not.toHaveBeenCalled()
    act(() => rfbs[0].emit("connect"))
    expect(calls).toEqual([{ widthPx: 2000, heightPx: 1400 }])
    size = { width: 900, height: 600 }
    resizeCallback!()
    size = { width: 901, height: 601 }
    resizeCallback!()
    await advance(299)
    expect(resize).toHaveBeenCalledTimes(1)
    await advance(1)
    expect(resize).toHaveBeenCalledTimes(1)
    await act(async () => { releases[0](); await Promise.resolve(); await Promise.resolve() })
    expect(calls[1]).toEqual({ widthPx: 1802, heightPx: 1202 })
    await act(async () => { releases[1](); await Promise.resolve(); await Promise.resolve() })
    resizeCallback!()
    await advance(400)
    expect(resize).toHaveBeenCalledTimes(2)
    size = { width: 300, height: 200 }
    resizeCallback!()
    await advance(400)
    expect(resize).toHaveBeenCalledTimes(2)
  })

  it("shows a resize failure as a notice and stays connected", async () => {
    mount(undefined, () => Promise.reject("Resize refused"))
    await flush()
    act(() => rfbs[0].emit("connect"))
    await flush()
    expect(screen.getByRole("alert")).toHaveTextContent("Resize refused")
    expect(screen.getByRole("status")).toHaveTextContent("Connected")
  })
})
