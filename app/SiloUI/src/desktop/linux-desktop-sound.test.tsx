import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { LinuxDesktopViewer, NativeLinuxDesktopViewer } from "./linux-desktop-viewer"
import { readMuted } from "./linux-desktop-sound-state"

const invoke = vi.hoisted(() => vi.fn())
vi.mock("@tauri-apps/api/core", () => ({ invoke }))
const nativeMenu = vi.hoisted(() => ({ create: vi.fn() }))
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: nativeMenu.create } }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ isFullscreen: async () => false, setFullscreen: vi.fn(), innerSize: async () => ({ height: 900 }), scaleFactor: async () => 2 }) }))

let soundSupported = true
let hidden = false
function stubVisibility() {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (hidden ? "hidden" : "visible") })
}
beforeEach(() => {
  soundSupported = true
  hidden = false
  localStorage.clear()
  nativeMenu.create.mockReset()
  stubVisibility()
  invoke.mockReset().mockImplementation(async command => {
    if (command === "read_desktop_state") return { installed: true, autoStart: true, state: "running" }
    if (command === "desktop_viewer_sound_support") return { sound: soundSupported }
    return undefined
  })
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} })
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 44, left: 0, bottom: 644, width: 1000, height: 600 } as DOMRect)
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  Reflect.deleteProperty(document, "visibilityState")
})

const audioCalls = () => invoke.mock.calls.filter(([command]) => command === "desktop_viewer_set_audio").map(([, args]) => args)

it("shows no sound control when the engine cannot play the computer's sound", async () => {
  soundSupported = false
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("desktop_viewer_sound_support", { computer: "dev" }))
  await screen.findByLabelText("Linux desktop display")
  expect(screen.queryByRole("button", { name: /sound/i })).not.toBeInTheDocument()
  expect(audioCalls()).toEqual([])
})

it("shows a sound control that starts unmuted and applies the choice to the computer", async () => {
  const user = userEvent.setup()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  const mute = await screen.findByRole("button", { name: "Mute sound" })
  await waitFor(() => expect(audioCalls()).toEqual([{ computer: "dev", muted: false, active: true }]))
  await user.click(mute)
  expect(await screen.findByRole("button", { name: "Unmute sound" })).toHaveAttribute("aria-pressed", "true")
  await waitFor(() => expect(audioCalls().at(-1)).toEqual({ computer: "dev", muted: true, active: true }))
})

it("remembers the mute choice per computer on this device", async () => {
  const user = userEvent.setup()
  const first = render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await user.click(await screen.findByRole("button", { name: "Mute sound" }))
  expect(readMuted("dev")).toBe(true)
  expect(readMuted("other")).toBe(false)
  first.unmount()
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  expect(await screen.findByRole("button", { name: "Unmute sound" })).toBeVisible()
  await waitFor(() => expect(audioCalls().at(-1)).toEqual({ computer: "dev", muted: true, active: true }))
  await user.click(screen.getByRole("button", { name: "Unmute sound" }))
  expect(readMuted("dev")).toBe(false)
})

it("stops the audio stream while the viewer is hidden and resumes when it returns", async () => {
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await screen.findByRole("button", { name: "Mute sound" })
  await waitFor(() => expect(audioCalls().length).toBe(1))
  hidden = true
  act(() => { document.dispatchEvent(new Event("visibilitychange")) })
  await waitFor(() => expect(audioCalls().at(-1)).toEqual({ computer: "dev", muted: false, active: false }))
  hidden = false
  act(() => { document.dispatchEvent(new Event("visibilitychange")) })
  await waitFor(() => expect(audioCalls().at(-1)).toEqual({ computer: "dev", muted: false, active: true }))
})

it("resets the screen size from the native desktop actions menu", async () => {
  const user = userEvent.setup()
  nativeMenu.create.mockResolvedValue({ popup: vi.fn().mockResolvedValue(undefined), close: vi.fn().mockResolvedValue(undefined) })
  render(<NativeLinuxDesktopViewer computer="dev" name="dev" />)
  await user.click(await screen.findByRole("button", { name: "Desktop actions" }))
  await waitFor(() => expect(nativeMenu.create).toHaveBeenCalled())
  const items = nativeMenu.create.mock.calls[0][0].items as Array<{ text: string; action: () => void }>
  expect(items.map(item => item.text)).toEqual(["Reset to 1440×900", "Restart desktop…", "Stop desktop…"])
  items[0].action()
  expect(invoke).toHaveBeenCalledWith("desktop_viewer_reset_screen", { computer: "dev" })
})

it("offers the reset item only when the viewer can reset", async () => {
  const user = userEvent.setup()
  const onResetScreen = vi.fn()
  const props = { name: "dev", state: { installed: true, autoStart: true, state: "running" } as const, busy: false, error: null, onAction: vi.fn(), onRetry: vi.fn(), onFullscreen: vi.fn() }
  const view = render(<LinuxDesktopViewer {...props} />)
  await user.click(screen.getByRole("button", { name: "Desktop actions" }))
  expect(within(screen.getByRole("menu")).queryByRole("menuitem", { name: "Reset to 1440×900" })).not.toBeInTheDocument()
  view.unmount()
  render(<LinuxDesktopViewer {...props} onResetScreen={onResetScreen} />)
  await user.click(screen.getByRole("button", { name: "Desktop actions" }))
  await user.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: "Reset to 1440×900" }))
  expect(onResetScreen).toHaveBeenCalledOnce()
})

it("shows the sound control from the supplied state", () => {
  const props = { name: "dev", state: { installed: true, autoStart: true, state: "running" } as const, busy: false, error: null, onAction: vi.fn(), onRetry: vi.fn(), onFullscreen: vi.fn() }
  const view = render(<LinuxDesktopViewer {...props} sound={{ available: false, muted: false, onToggle: vi.fn() }} />)
  expect(screen.queryByRole("button", { name: /sound/i })).not.toBeInTheDocument()
  view.unmount()
  render(<LinuxDesktopViewer {...props} sound={{ available: true, muted: true, onToggle: vi.fn() }} />)
  expect(screen.getByRole("button", { name: "Unmute sound" })).toBeVisible()
})
