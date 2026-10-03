import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, afterEach, expect, it, vi } from "vitest"
import { NativeLinuxDesktopViewer } from "./linux-desktop-viewer"

const invoke = vi.hoisted(() => vi.fn())
const nativeMenu = vi.hoisted(() => ({ create: vi.fn(), popup: vi.fn(), close: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke }))
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: nativeMenu.create } }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isFullscreen: async () => false, setFullscreen: vi.fn(), innerSize: async () => ({ height: (window.innerHeight + 32) * 2 }), scaleFactor: async () => 2 }) }))
let resize: (() => void) | undefined
beforeEach(() => {
  invoke.mockReset()
  nativeMenu.create.mockReset().mockResolvedValue({ popup: nativeMenu.popup, close: nativeMenu.close })
  nativeMenu.popup.mockReset().mockResolvedValue(undefined)
  nativeMenu.close.mockReset().mockResolvedValue(undefined)
  vi.stubGlobal("ResizeObserver", class { constructor(callback: () => void) { resize = callback } observe() {} disconnect() {} })
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 44, left: 0, bottom: 644, width: 1000, height: 600 } as DOMRect)
})
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals() })

it("opens a stopped computer viewer without starting the computer and only starts after the user's action", async () => {
  invoke.mockImplementation(async command => command === "read_desktop_state" ? { installed: true, autoStart: false, state: "computer-stopped" }
    : command === "desktop_action" ? { installed: true, autoStart: false, state: "running" } : undefined)
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="owner/vm-id" name="dev · Remote" />)
  await screen.findByRole("button", { name: "Start computer and desktop" })
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action" || command === "desktop_viewer_attach")).toBe(false)
  await user.click(screen.getByRole("button", { name: "Start computer and desktop" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", { computer: "owner/vm-id", x: 0, y: 44, width: 1000, height: 600, viewportHeight: window.innerHeight }))
  expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "owner/vm-id", action: "start" })
})

it("resizes the native view and detaches on close without stopping its session", async () => {
  invoke.mockImplementation(async command => command === "read_desktop_state" ? { installed: true, autoStart: true, state: "running" } : undefined)
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ computer: "dev" })))
  invoke.mockClear()
  await act(async () => { resize?.() })
  expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ width: 1000, height: 600 }))
  view.unmount()
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_detach"))
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
})

it("keeps a transport failure visible when subsequent guest health checks succeed", async () => {
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") return { installed: true, autoStart: true, state: "running" }
    if (command === "desktop_viewer_attach") throw new Error("Desktop connection unavailable")
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByRole("alert")).toHaveTextContent("Desktop connection unavailable")
  await user.click(screen.getByRole("button", { name: "Reconnect" }))
  await waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === "read_desktop_state").length).toBeGreaterThan(1))
  expect(screen.getByRole("alert")).toHaveTextContent("Desktop connection unavailable")
})

it("hides an obsolete attachment error when the computer stops", async () => {
  vi.useFakeTimers()
  let stopped = false
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: stopped ? "computer-stopped" : "running",
    }
    if (command === "desktop_viewer_attach") throw new Error("Desktop connection unavailable")
  })
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(screen.getByRole("alert")).toHaveTextContent("Desktop connection unavailable")
    stopped = true
    await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
    expect(screen.getByRole("button", { name: "Start computer" })).toBeVisible()
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument()
    expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
  } finally { view.unmount(); vi.useRealTimers() }
})

it("backs off failed desktop health reads, resets after recovery, and stops on close", async () => {
  vi.useFakeTimers()
  let reachable = false
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") {
      if (!reachable) throw new Error("Device disconnected")
      return { installed: true, autoStart: true, state: "computer-stopped" }
    }
  })
  const reads = () => invoke.mock.calls.filter(([command]) => command === "read_desktop_state").length
  const advance = async (ms: number) => { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }
  const view = render(<NativeLinuxDesktopViewer computer="owner/vm-id" name="dev · Remote" />)
  try {
    await advance(0)
    expect(reads()).toBe(1)
    for (const delay of [10000, 20000, 30000, 30000]) {
      const calls = reads()
      await advance(delay - 1)
      expect(reads()).toBe(calls)
      await advance(1)
      expect(reads()).toBe(calls + 1)
    }
    reachable = true
    await advance(30000)
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    const calls = reads()
    await advance(4999)
    expect(reads()).toBe(calls)
    await advance(1)
    expect(reads()).toBe(calls + 1)
    view.unmount()
    await advance(60000)
    expect(reads()).toBe(calls + 1)
  } finally { view.unmount(); vi.useRealTimers() }
})

it("reattaches a retired transport when the device recovers with unchanged guest state", async () => {
  vi.useFakeTimers()
  let reachable = true
  let connected = false
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") {
      if (!reachable) throw new Error("Device disconnected")
      return { installed: true, autoStart: true, state: "running", sessionState: "running", streamState: "running" }
    }
    if (command === "desktop_viewer_attach") connected = true
    if (command === "desktop_viewer_detach") connected = false
  })
  const view = render(<NativeLinuxDesktopViewer computer="owner/vm-id" name="dev · Remote" />)
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(connected).toBe(true)
    // The owner poll closes the backend transport without changing guest state.
    reachable = false
    connected = false
    await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
    expect(screen.getByRole("alert")).toHaveTextContent("Device disconnected")
    expect(connected).toBe(false)
    reachable = true
    await act(async () => { await vi.advanceTimersByTimeAsync(10000) })
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(connected).toBe(true)
    expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
  } finally { view.unmount(); vi.useRealTimers() }
})


it("finishes an in-flight attachment before detaching on close", async () => {
  let completeAttachment: (() => void) | undefined
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") return { installed: true, autoStart: true, state: "running" }
    if (command === "desktop_viewer_attach") await new Promise<void>(resolve => { completeAttachment = resolve })
  })
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await waitFor(() => expect(completeAttachment).toBeDefined())
  invoke.mockClear()
  // Resizes arriving during the pending attachment must not survive the close.
  act(() => { resize?.() })
  view.unmount()
  expect(invoke).not.toHaveBeenCalled()
  await act(async () => { completeAttachment?.() })
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_detach"))
  expect(invoke.mock.calls.some(([command]) => command === "desktop_viewer_attach")).toBe(false)
})

it("reconnects by detaching the previous child before attaching a fresh one", async () => {
  let fail = true
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") return { installed: true, autoStart: true, state: "running" }
    if (command === "desktop_viewer_attach" && fail) throw new Error("Connection lost")
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await screen.findByRole("alert")
  invoke.mockClear()
  fail = false
  await user.click(screen.getByRole("button", { name: "Reconnect" }))
  await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument())
  expect(invoke.mock.calls.map(([command]) => command).filter(command => command.startsWith("desktop_viewer_"))).toEqual(["desktop_viewer_detach", "desktop_viewer_attach"])
})


it("updates the native inset when the viewport changes even if display bounds are unchanged", async () => {
  let viewportHeight = 788
  vi.spyOn(window, "innerHeight", "get").mockImplementation(() => viewportHeight)
  invoke.mockImplementation(async command => command === "read_desktop_state" ? { installed: true, autoStart: true, state: "running" } : undefined)
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ viewportHeight: 788 })))
  invoke.mockClear()
  viewportHeight = 820
  await act(async () => { window.dispatchEvent(new Event("resize")) })
  expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", { computer: "dev", x: 0, y: 44, width: 1000, height: 600, viewportHeight: 820 })
  view.unmount()
  await act(async () => { window.dispatchEvent(new Event("resize")) })
  expect(invoke.mock.calls.filter(([command]) => command === "desktop_viewer_attach")).toHaveLength(1)
})

it("uses a native dropdown without reconnecting or resizing the guest", async () => {
  invoke.mockImplementation(async command => command === "read_desktop_state" ? { installed: true, autoStart: true, state: "running" } : undefined)
  const user = userEvent.setup()
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await screen.findByRole("button", { name: "Desktop actions" })
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.anything()))
  invoke.mockClear()
  await user.click(screen.getByRole("button", { name: "Desktop actions" }))
  expect(nativeMenu.popup).toHaveBeenCalledOnce()
  expect(nativeMenu.popup).toHaveBeenCalledWith(expect.objectContaining({ x: 0, y: 676 }))
  expect(screen.queryByRole("menu")).not.toBeInTheDocument()
  expect(invoke).not.toHaveBeenCalled()
  // GTK returns before dismissal, so the menu must survive popup resolution.
  expect(nativeMenu.close).not.toHaveBeenCalled()
  const items = nativeMenu.create.mock.calls[0][0].items
  expect(items.map((item: { text: string }) => item.text)).toEqual(["Restart desktop…", "Stop desktop…"])
  act(() => items[1].action())
  expect(invoke).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Stop desktop" }))
  expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "stop" })
  view.unmount()
  expect(nativeMenu.close).toHaveBeenCalledOnce()
})

it("does not turn a guest status error into a tool failure", async () => {
  invoke.mockImplementation(async command => {
    if (command === "read_desktop_state") throw new Error("Device disconnected")
  })
  render(<NativeLinuxDesktopViewer computer="owner/vm-id" name="dev · Remote" />)
  expect(await screen.findByRole("alert")).toHaveTextContent("Device disconnected")
  expect(screen.queryByRole("button", { name: /agent tools/ })).not.toBeInTheDocument()
})

it("recovers a failed stream without restarting the live desktop session", async () => {
  invoke.mockImplementation(async (command, args) => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: "failed", backend: "selkies",
      sessionState: "running", streamState: "failed",
    }
    if (command === "desktop_action" && args?.action === "restart-streamer") return {
      installed: true, autoStart: true, state: "running", backend: "selkies",
      sessionState: "running", streamState: "starting",
    }
    return undefined
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByLabelText("Linux desktop display")).toBeVisible()
  expect(screen.getByRole("alert")).toHaveTextContent("Display disconnected")
  await user.click(screen.getByRole("button", { name: "Reconnect display" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "restart-streamer" }))
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Connecting display"))
  // The backend refuses to connect until the stream runs; don't attach early.
  expect(invoke.mock.calls.some(([command]) => command === "desktop_viewer_attach")).toBe(false)
  expect(invoke.mock.calls.some(([command, args]) => command === "desktop_action" && args?.action === "restart")).toBe(false)
})

it("attaches once the stream becomes ready", async () => {
  let streamState = "starting"
  invoke.mockImplementation(async command => command === "read_desktop_state" ? {
    installed: true, autoStart: true, state: "running", backend: "selkies", sessionState: "running", streamState,
  } : undefined)
  vi.useFakeTimers()
  try {
    render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
    await act(async () => { await vi.advanceTimersByTimeAsync(0) })
    expect(screen.getByLabelText("Linux desktop display")).toBeVisible()
    expect(invoke.mock.calls.some(([command]) => command === "desktop_viewer_attach")).toBe(false)
    streamState = "running"
    await act(async () => { await vi.advanceTimersByTimeAsync(4_999) })
    expect(invoke.mock.calls.some(([command]) => command === "desktop_viewer_attach")).toBe(false)
    await act(async () => { await vi.advanceTimersByTimeAsync(1) })
    expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ computer: "dev" }))
  } finally { vi.useRealTimers() }
})

it("keeps the desktop usable when the guest reports a failed or unknown diagnostic", async () => {
  invoke.mockImplementation(async command => command === "read_desktop_state" ? {
    installed: true, autoStart: true, state: "running", backend: "selkies", sessionState: "running", streamState: "running",
    lcuState: "failed", lcuReadiness: "failed", lcuReason: 42,
  } : undefined)
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByLabelText("Linux desktop display")).toBeVisible()
  expect(screen.queryByText(/Desktop unavailable/)).not.toBeInTheDocument()
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ computer: "dev" })))
})

it("offers an explicit stopped-only desktop update without starting or attaching it", async () => {
  invoke.mockImplementation(async (command, args) => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: "stopped", backend: "kasm",
      sessionState: "stopped", streamState: "stopped", updateRequired: true,
    }
    if (command === "desktop_action" && args?.action === "update-streamer") return {
      installed: true, autoStart: true, state: "stopped", backend: "selkies",
      sessionState: "stopped", streamState: "stopped", updateRequired: false,
    }
    return undefined
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByRole("button", { name: "Update desktop" })).toBeVisible()
  expect(screen.queryByRole("button", { name: "Start desktop" })).not.toBeInTheDocument()
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
  await user.click(screen.getByRole("button", { name: "Update desktop" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "update-streamer" }))
  expect(await screen.findByRole("button", { name: "Start desktop" })).toBeVisible()
  expect(invoke.mock.calls.some(([command]) => command === "desktop_viewer_attach")).toBe(false)
  expect(invoke.mock.calls.some(([command, args]) => command === "desktop_action" && args?.action === "start")).toBe(false)
})

it("keeps a compatible older desktop startable and offers the update separately", async () => {
  invoke.mockImplementation(async (command, args) => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: "stopped", backend: "selkies",
      sessionState: "stopped", streamState: "stopped", updateRequired: false, updateAvailable: true,
    }
    if (command === "desktop_action" && args?.action === "start") return {
      installed: true, autoStart: true, state: "stopped", backend: "selkies",
      sessionState: "stopped", streamState: "stopped", updateRequired: false, updateAvailable: true,
    }
    return undefined
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByRole("button", { name: "Start desktop" })).toBeVisible()
  expect(screen.getByRole("button", { name: "Update desktop" })).toBeVisible()
  expect(screen.getByText(/clipboard sharing, sound control and a screen that follows the window/)).toBeVisible()
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
  await user.click(screen.getByRole("button", { name: "Start desktop" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "start" }))
  expect(invoke.mock.calls.some(([command, args]) => command === "desktop_action" && args?.action === "update-streamer")).toBe(false)
})

it("keeps a healthy stopped legacy desktop startable and makes migration optional", async () => {
  invoke.mockImplementation(async (command, args) => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: "stopped", backend: "kasm",
      sessionState: "stopped", streamState: "stopped", updateRequired: false,
    }
    if (command === "desktop_action" && args?.action === "start") return {
      installed: true, autoStart: true, state: "stopped", backend: "kasm",
      sessionState: "stopped", streamState: "stopped", updateRequired: false,
    }
    return undefined
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByRole("button", { name: "Start desktop" })).toBeVisible()
  expect(screen.getByRole("button", { name: "Update desktop" })).toBeVisible()
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
  await user.click(screen.getByRole("button", { name: "Start desktop" }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "start" }))
  expect(invoke.mock.calls.some(([command, args]) => command === "desktop_action" && args?.action === "update-streamer")).toBe(false)
})

it("keeps a running desktop healthy when the LCU runtime prerequisite is missing", async () => {
  invoke.mockImplementation(async command => command === "read_desktop_state" ? {
    installed: true, autoStart: true, state: "running", backend: "selkies",
    sessionState: "running", streamState: "running", lcuState: "needs-runtime",
    lcuReason: "chatgpt-app-required",
  } : undefined)
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByLabelText("Linux desktop display")).toBeVisible()
  expect(await screen.findByText(/LCU requires the official ChatGPT app/)).toBeVisible()
  expect(screen.getByRole("button", { name: "Set up LCU" })).toBeEnabled()
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_attach", expect.objectContaining({ computer: "dev" })))
  expect(invoke.mock.calls.some(([command]) => command === "desktop_action")).toBe(false)
})

it("runs explicit LCU setup in a live session even when its display stream failed", async () => {
  invoke.mockImplementation(async (command, args) => {
    if (command === "read_desktop_state") return {
      installed: true, autoStart: true, state: "failed", backend: "selkies",
      sessionState: "running", streamState: "failed", lcuState: "not-installed",
    }
    if (command === "desktop_action" && args?.action === "setup-lcu") return {
      installed: true, autoStart: true, state: "failed", backend: "selkies",
      sessionState: "running", streamState: "failed", lcuState: "needs-runtime",
      lcuReason: "chatgpt-app-required",
    }
    return undefined
  })
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByLabelText("Linux desktop display")).toBeVisible()
  expect(await screen.findByRole("button", { name: "Set up LCU" })).toBeEnabled()
  // A failed stream cannot be attached; the session stays usable for setup.
  const attachmentCount = 0
  await user.click(screen.getByRole("button", { name: "Set up LCU" }))
  expect(await screen.findByText(/LCU requires the official ChatGPT app/)).toBeVisible()
  expect(invoke).toHaveBeenCalledWith("desktop_action", { computer: "dev", action: "setup-lcu" })
  expect(screen.getByLabelText("Linux desktop display")).toBeVisible()
  expect(invoke.mock.calls.filter(([command]) => command === "desktop_viewer_attach")).toHaveLength(attachmentCount)
  expect(invoke.mock.calls.some(([command, action]) => command === "desktop_action" &&
    ["restart", "restart-streamer", "start"].includes(action?.action))).toBe(false)
})

it("pauses viewer health polling when hidden and refreshes once on return", async () => {
  vi.useFakeTimers()
  const visible = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
  invoke.mockImplementation(async command => command === "read_desktop_state" ? { installed: true, autoStart: true, state: "computer-stopped" } : undefined)
  const view = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  const reads = () => invoke.mock.calls.filter(([command]) => command === "read_desktop_state")
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
    expect(reads()).toHaveLength(0)
    visible.mockReturnValue("visible")
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); await vi.advanceTimersByTimeAsync(0) })
    expect(reads()).toHaveLength(1)
    visible.mockReturnValue("hidden")
    act(() => { document.dispatchEvent(new Event("visibilitychange")) })
    await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
    expect(reads()).toHaveLength(1)
    view.unmount()
    visible.mockReturnValue("visible")
    act(() => { document.dispatchEvent(new Event("visibilitychange")) })
    expect(reads()).toHaveLength(1)
  } finally { view.unmount(); vi.useRealTimers() }
})
