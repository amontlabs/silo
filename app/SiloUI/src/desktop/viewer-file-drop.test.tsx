import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { ViewerTransferStatus, useViewerFileDrop } from "./viewer-file-drop"

type Handler = (event: { payload: unknown }) => void
const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  handlers: new Map<string, Handler>(),
  windows: new Map<string, Map<string, Handler>>(),
  label: "desktop-shell-a",
  unlisten: vi.fn(),
}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => {
    const label = native.label
    return {
      listen: async (event: string, handler: Handler) => {
        const handlers = native.windows.get(label) ?? new Map<string, Handler>()
        native.windows.set(label, handlers)
        handlers.set(event, handler)
        return native.unlisten
      },
    }
  },
}))
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => { throw new Error("the viewer does not read native drops itself") },
}))

function Viewer({ computer = "dev" }: { computer?: string }) {
  const drop = useViewerFileDrop(computer)
  return <header><ViewerTransferStatus drop={drop} /></header>
}

// The native side addresses a drop to one viewer window; only that window's listeners hear it.
const emitTo = (label: string, event: string, payload: unknown) => act(async () => { native.windows.get(label)?.get(event)?.({ payload }) })
const emit = (event: string, payload: unknown) => emitTo("desktop-shell-a", event, payload)
const dropped = (names: string[], token = "tok") => emit("silo://viewer-drop", { token, names })
const ready = (label = "desktop-shell-a") => waitFor(() => expect(native.windows.get(label)?.has("silo://viewer-drop") && native.windows.get(label)?.has("silo://viewer-drag")).toBe(true))

beforeEach(() => {
  native.invoke.mockReset()
  native.handlers.clear()
  native.windows.clear()
  native.label = "desktop-shell-a"
  native.unlisten.mockReset()
  native.listen.mockReset().mockImplementation(async (event: string, handler: Handler) => { native.handlers.set(event, handler); return native.unlisten })
})
afterEach(() => vi.useRealTimers())

describe("dropping files on the desktop viewer", () => {
  it("uploads dropped files to the Downloads folder by their token, keeping both copies of a name", async () => {
    native.invoke.mockResolvedValue({ status: "done", names: ["report (1).pdf"] })
    render(<Viewer computer="owner/vm" />)
    await ready()
    await dropped(["report.pdf"], "token-1")
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("upload_files", expect.objectContaining({
      computer: "owner/vm", directory: "/home/silo/Downloads", selection: "token-1", conflict: "keepBoth",
    })))
    expect(native.invoke.mock.calls[0][1]).not.toHaveProperty("paths")
    expect(await screen.findByRole("status")).toHaveTextContent("Uploaded “report (1).pdf” to Downloads")
  })

  it("shows progress for its own transfer only and cancels it from the toolbar", async () => {
    const user = userEvent.setup()
    let finish!: (value: unknown) => void
    native.invoke.mockImplementation((command: string) => command === "upload_files" ? new Promise(resolve => { finish = resolve }) : Promise.resolve(null))
    render(<Viewer />)
    await ready()
    await dropped(["a.bin", "b.bin"])
    expect(await screen.findByRole("status")).toHaveTextContent("Uploading 2 files")
    const { transferId } = native.invoke.mock.calls[0][1]
    act(() => native.handlers.get("silo://transfer-progress")?.({ payload: { id: "other", computer: "dev", direction: "upload", state: "transferring", name: "x", fileIndex: 0, fileCount: 1, bytesDone: 9, bytesTotal: 10 } }))
    expect(screen.getByRole("progressbar")).toHaveAttribute("data-state", "indeterminate")
    act(() => native.handlers.get("silo://transfer-progress")?.({ payload: { id: transferId, computer: "dev", direction: "upload", state: "transferring", name: "a.bin", fileIndex: 0, fileCount: 2, bytesDone: 50, bytesTotal: 200 } }))
    expect(screen.getByRole("status")).toHaveTextContent("Uploading a.bin · 1 of 2")
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25")
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(native.invoke).toHaveBeenCalledWith("cancel_transfer", { transferId })
    await act(async () => finish({ status: "cancelled" }))
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull())
  })

  it("explains a failed upload and lets it be dismissed", async () => {
    const user = userEvent.setup()
    native.invoke.mockRejectedValue("Start this computer to transfer files.")
    render(<Viewer />)
    await ready()
    await dropped(["a.bin"])
    expect(await screen.findByRole("alert")).toHaveTextContent("Start this computer to transfer files.")
    await user.click(screen.getByRole("button", { name: "Dismiss" }))
    expect(screen.queryByRole("alert")).toBeNull()
  })

  it("hints while files are dragged over", async () => {
    render(<Viewer />)
    await ready()
    await emit("silo://viewer-drag", true)
    expect(screen.getByRole("status")).toHaveTextContent("Drop to upload to Downloads")
    await emit("silo://viewer-drag", false)
    expect(screen.queryByRole("status")).toBeNull()
  })

  it("keeps the running upload and its Cancel button when a second drop is refused", async () => {
    const user = userEvent.setup()
    native.invoke.mockImplementation(() => new Promise(() => {}))
    render(<Viewer />)
    await ready()
    await dropped(["a"], "first")
    await dropped(["b"], "second")
    const alert = await screen.findByRole("alert")
    expect(alert).toHaveTextContent("Another file transfer is still running.")
    expect(screen.getByRole("status")).toHaveTextContent("Uploading a")
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument()
    expect(native.invoke.mock.calls.filter(([command]) => command === "upload_files")).toHaveLength(1)
    await user.click(screen.getByRole("button", { name: "Dismiss" }))
    expect(screen.queryByRole("alert")).toBeNull()
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument()
  })

  it("listens on its own window only, so a drop aimed at one viewer is not uploaded by another", async () => {
    native.invoke.mockResolvedValue({ status: "done", names: ["a.txt"] })
    render(<Viewer computer="first" />)
    native.label = "desktop-shell-b"
    render(<Viewer computer="second" />)
    await ready("desktop-shell-a")
    await ready("desktop-shell-b")
    expect(native.listen.mock.calls.map(([event]) => event)).not.toContain("silo://viewer-drop")
    await emitTo("desktop-shell-b", "silo://viewer-drop", { token: "for-b", names: ["a.txt"] })
    await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(1))
    expect(native.invoke).toHaveBeenCalledWith("upload_files", expect.objectContaining({ computer: "second", selection: "for-b" }))
  })

  it("ignores malformed or empty drops and stops listening when the viewer closes", async () => {
    const view = render(<Viewer />)
    await ready()
    await dropped([])
    await emit("silo://viewer-drop", { paths: ["/Users/ada/secret"] })
    await emit("silo://viewer-drop", "/Users/ada/secret")
    expect(native.invoke).not.toHaveBeenCalled()
    view.unmount()
    await waitFor(() => expect(native.unlisten).toHaveBeenCalledTimes(2))
  })
})
