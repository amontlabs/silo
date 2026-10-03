import { act, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { ViewerTransferStatus, useViewerFileDrop } from "./viewer-file-drop"

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  dragDrop: undefined as undefined | ((event: { payload: unknown }) => void),
  unlistenDrag: vi.fn(),
  unlistenProgress: vi.fn(),
  progress: undefined as undefined | ((event: { payload: unknown }) => void),
}))
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }))
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }))
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onDragDropEvent: async (handler: (event: { payload: unknown }) => void) => { native.dragDrop = handler; return native.unlistenDrag } }),
}))

function Viewer({ computer = "dev" }: { computer?: string }) {
  const drop = useViewerFileDrop(computer)
  return <header><ViewerTransferStatus drop={drop} /></header>
}

const drop = (paths: string[]) => act(async () => { native.dragDrop?.({ payload: { type: "drop", paths, position: { x: 1, y: 1 } } }) })

beforeEach(() => {
  native.invoke.mockReset()
  native.listen.mockReset().mockImplementation(async (_event: string, handler: (event: { payload: unknown }) => void) => { native.progress = handler; return native.unlistenProgress })
  native.dragDrop = undefined
  native.unlistenDrag.mockReset()
  native.unlistenProgress.mockReset()
})
afterEach(() => vi.useRealTimers())

describe("dropping files on the desktop viewer", () => {
  it("uploads dropped files to the Downloads folder, keeping both copies of a name", async () => {
    native.invoke.mockResolvedValue({ status: "done", names: ["report (1).pdf"] })
    render(<Viewer computer="owner/vm" />)
    await waitFor(() => expect(native.dragDrop).toBeDefined())
    await drop(["/Users/ada/report.pdf"])
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("upload_files", expect.objectContaining({
      computer: "owner/vm", directory: "/home/silo/Downloads", paths: ["/Users/ada/report.pdf"], conflict: "keepBoth",
    })))
    expect(await screen.findByRole("status")).toHaveTextContent("Uploaded “report (1).pdf” to Downloads")
    expect(native.unlistenProgress).toHaveBeenCalled()
  })

  it("shows progress for its own transfer only and cancels it from the toolbar", async () => {
    const user = userEvent.setup()
    let finish!: (value: unknown) => void
    native.invoke.mockImplementation((command: string) => command === "upload_files" ? new Promise(resolve => { finish = resolve }) : Promise.resolve(null))
    render(<Viewer />)
    await waitFor(() => expect(native.dragDrop).toBeDefined())
    await drop(["/Users/ada/a.bin", "/Users/ada/b.bin"])
    expect(await screen.findByRole("status")).toHaveTextContent("Uploading 2 files")
    const { transferId } = native.invoke.mock.calls[0][1]
    act(() => native.progress?.({ payload: { id: "other", computer: "dev", direction: "upload", state: "transferring", name: "x", fileIndex: 0, fileCount: 1, bytesDone: 9, bytesTotal: 10 } }))
    expect(screen.getByRole("progressbar")).toHaveAttribute("data-state", "indeterminate")
    act(() => native.progress?.({ payload: { id: transferId, computer: "dev", direction: "upload", state: "transferring", name: "a.bin", fileIndex: 0, fileCount: 2, bytesDone: 50, bytesTotal: 200 } }))
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
    await waitFor(() => expect(native.dragDrop).toBeDefined())
    await drop(["/Users/ada/a.bin"])
    expect(await screen.findByRole("alert")).toHaveTextContent("Start this computer to transfer files.")
    await user.click(screen.getByRole("button", { name: "Dismiss" }))
    expect(screen.queryByRole("alert")).toBeNull()
  })

  it("hints while files are dragged over and refuses a second drop during a transfer", async () => {
    native.invoke.mockImplementation(() => new Promise(() => {}))
    render(<Viewer />)
    await waitFor(() => expect(native.dragDrop).toBeDefined())
    act(() => native.dragDrop?.({ payload: { type: "enter", paths: ["/a"], position: { x: 1, y: 1 } } }))
    expect(screen.getByRole("status")).toHaveTextContent("Drop to upload to Downloads")
    act(() => native.dragDrop?.({ payload: { type: "leave" } }))
    expect(screen.queryByRole("status")).toBeNull()
    await drop(["/a"])
    await drop(["/b"])
    expect(await screen.findByRole("alert")).toHaveTextContent("Another file transfer is still running.")
    expect(native.invoke.mock.calls.filter(([command]) => command === "upload_files")).toHaveLength(1)
  })

  it("ignores a drop of nothing and stops listening when the viewer closes", async () => {
    const view = render(<Viewer />)
    await waitFor(() => expect(native.dragDrop).toBeDefined())
    await drop([])
    expect(native.invoke).not.toHaveBeenCalled()
    view.unmount()
    await waitFor(() => expect(native.unlistenDrag).toHaveBeenCalled())
  })
})
