import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import { ViewerClipboard } from "./viewer-clipboard"
import { clipboardFeedback, type ClipboardReport } from "./viewer-clipboard-feedback"
import { LinuxDesktopViewer } from "./linux-desktop-viewer"

const native = vi.hoisted(() => ({ invoke: vi.fn(), receive: (_event: { payload: unknown }) => {}, unlisten: vi.fn() }))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: async (_name: string, receive: typeof native.receive) => { native.receive = receive; return native.unlisten } }))
beforeEach(() => { native.invoke.mockReset(); native.unlisten.mockReset() })
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers() })

const report = (overrides: Partial<ClipboardReport>): ClipboardReport => ({ action: "copy", status: "copied", content: "text", message: null, ...overrides })

it("copies from the computer and reports it by name", async () => {
  native.invoke.mockResolvedValue(report({}))
  const user = userEvent.setup()
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await user.click(screen.getByRole("button", { name: "Copy from computer" }))
  expect(native.invoke).toHaveBeenCalledWith("desktop_viewer_clipboard", { computer: "dev", action: "copy" })
  expect(await screen.findByRole("status")).toHaveTextContent("Copied from Build box")
})

it("pastes into the computer and reports an image paste", async () => {
  native.invoke.mockResolvedValue(report({ action: "paste", status: "pasted", content: "image" }))
  const user = userEvent.setup()
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await user.click(screen.getByRole("button", { name: "Paste into computer" }))
  expect(native.invoke).toHaveBeenCalledWith("desktop_viewer_clipboard", { computer: "dev", action: "paste" })
  expect(await screen.findByRole("status")).toHaveTextContent("Pasted image into Build box")
})

it.each<[Partial<ClipboardReport>, string]>([
  [{ status: "computer-empty", content: null }, "Nothing to copy from Build box"],
  [{ action: "paste", status: "device-empty", content: null }, "This device's clipboard has no text or image"],
  [{ action: "paste", status: "too-large", content: "image" }, "That image is too large to paste"],
  [{ status: "too-large", content: "text" }, "That text is too large to copy"],
  [{ status: "too-large", content: null }, "Build box's clipboard is too large to copy"],
  [{ status: "not-connected", content: null }, "The desktop is not connected"],
  [{ status: "failed", content: null, message: "The computer did not answer." }, "The computer did not answer."],
])("shows %j as an alert", async (overrides, text) => {
  native.invoke.mockResolvedValue(report(overrides))
  const user = userEvent.setup()
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await user.click(screen.getByRole("button", { name: overrides.action === "paste" ? "Paste into computer" : "Copy from computer" }))
  expect(await screen.findByRole("alert")).toHaveTextContent(text)
})

it("shows a rejected command as an alert and re-enables the buttons", async () => {
  native.invoke.mockRejectedValue("This window cannot access that desktop.")
  const user = userEvent.setup()
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await user.click(screen.getByRole("button", { name: "Copy from computer" }))
  expect(await screen.findByRole("alert")).toHaveTextContent("This window cannot access that desktop.")
  expect(screen.getByRole("button", { name: "Copy from computer" })).toBeEnabled()
})

it("asks for a desktop update when the backend reports the clipboard unsupported", async () => {
  native.invoke.mockResolvedValue(report({ action: "paste", status: "unsupported", content: null }))
  const user = userEvent.setup()
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await user.click(screen.getByRole("button", { name: "Paste into computer" }))
  expect(native.invoke).toHaveBeenCalledWith("desktop_viewer_clipboard", { computer: "dev", action: "paste" })
  expect(await screen.findByRole("alert")).toHaveTextContent("Update the desktop to use the clipboard")
})

it("shows the same update request for a shortcut-started transfer", async () => {
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await waitFor(() => expect(native.receive).not.toBe(undefined))
  await act(async () => { native.receive({ payload: report({ status: "unsupported", content: null }) }) })
  expect(screen.getByRole("alert")).toHaveTextContent("Update the desktop to use the clipboard")
})

it("shows the outcome of a shortcut-started transfer and ignores unrelated events", async () => {
  render(<ViewerClipboard computer="dev" name="Build box" />)
  await waitFor(() => expect(native.receive).not.toBe(undefined))
  await act(async () => { native.receive({ payload: "unrelated" }) })
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
  await act(async () => { native.receive({ payload: report({}) }) })
  expect(screen.getByRole("status")).toHaveTextContent("Copied from Build box")
  expect(native.invoke).not.toHaveBeenCalled()
})

it("clears the feedback after a few seconds and stops listening on unmount", async () => {
  native.invoke.mockResolvedValue(report({}))
  const view = render(<ViewerClipboard computer="dev" name="Build box" />)
  await waitFor(() => expect(native.receive).not.toBe(undefined))
  vi.useFakeTimers()
  await act(async () => { native.receive({ payload: report({}) }) })
  expect(screen.getByRole("status")).toBeInTheDocument()
  act(() => { vi.advanceTimersByTime(4100) })
  expect(screen.queryByRole("status")).not.toBeInTheDocument()
  vi.useRealTimers()
  view.unmount()
  await waitFor(() => expect(native.unlisten).toHaveBeenCalled())
})

it("describes every status in words", () => {
  expect(clipboardFeedback(report({ status: "busy" }), "x").text).toBe("A clipboard transfer is already running")
  expect(clipboardFeedback(report({ status: "failed", message: null }), "x").text).toBe("The clipboard transfer failed")
})

it("appears in the toolbar of a running desktop only", () => {
  const props = { name: "dev", busy: false, error: null, onAction: vi.fn(), onRetry: vi.fn(), onFullscreen: vi.fn(), clipboard: <ViewerClipboard computer="dev" name="dev" /> }
  const view = render(<LinuxDesktopViewer {...props} state={{ installed: true, autoStart: true, state: "running" }} />)
  expect(screen.getByRole("button", { name: "Paste into computer" })).toBeVisible()
  view.rerender(<LinuxDesktopViewer {...props} state={{ installed: true, autoStart: true, state: "stopped" }} />)
  expect(screen.queryByRole("button", { name: "Paste into computer" })).not.toBeInTheDocument()
})
