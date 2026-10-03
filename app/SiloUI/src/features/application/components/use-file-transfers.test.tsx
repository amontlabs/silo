import { act, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { Toaster } from "@/components/ui/sonner"
import type { FileTransferActions, TransferProgress, UploadOutcome } from "@/features/application/model/file-transfer"
import { FileTransfersProvider, useFileTransferControls, useFileTransfers } from "./use-file-transfers"

function api(overrides: Partial<FileTransferActions> = {}) {
  let progress: ((value: TransferProgress) => void) | undefined
  const actions: FileTransferActions = {
    chooseUploadFiles: vi.fn(async () => ({ token: "pick-1", names: ["report.pdf"] })),
    upload: vi.fn(async (): Promise<UploadOutcome> => ({ status: "done", names: ["report.pdf"] })),
    download: vi.fn(async () => ({ status: "done" as const, path: "/Users/ada/Downloads/a.txt" })),
    cancel: vi.fn(async () => {}),
    onProgress: vi.fn(async handler => { progress = handler; return () => { progress = undefined } }),
    ...overrides,
  }
  return { actions, emit: (value: TransferProgress) => act(() => progress?.(value)) }
}

function Harness({ actions }: { actions?: FileTransferActions }) {
  const { controls, dialog } = useFileTransfers(actions)
  return <>
    <Toaster />
    {dialog}
    <button disabled={!controls || controls.busy} onClick={() => controls?.upload("dev", "/workspace", "dev")}>Upload</button>
    <button disabled={!controls || controls.busy} onClick={() => controls?.download("dev", "/workspace/a.txt", "dev")}>Download</button>
    <output>{controls ? (controls.busy ? "busy" : "idle") : "unavailable"}</output>
  </>
}

describe("file transfers on the Files page", () => {
  it("offers nothing without a native transfer boundary", () => {
    render(<Harness />)
    expect(screen.getByRole("status")).toHaveTextContent("unavailable")
  })

  it("uploads the chosen files and reports where they went", async () => {
    const user = userEvent.setup()
    const { actions } = api()
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    expect(await screen.findByText("Uploaded “report.pdf” to dev")).toBeInTheDocument()
    expect(actions.upload).toHaveBeenCalledWith(expect.objectContaining({ computer: "dev", directory: "/workspace", selection: "pick-1", conflict: "ask" }))
    expect(screen.getByRole("status")).toHaveTextContent("idle")
  })

  it("does nothing when the picker is dismissed", async () => {
    const user = userEvent.setup()
    const { actions } = api({ chooseUploadFiles: vi.fn(async () => null) })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("idle"))
    expect(actions.upload).not.toHaveBeenCalled()
  })

  it("asks before replacing and sends the chosen policy", async () => {
    const user = userEvent.setup()
    const upload = vi.fn()
      .mockResolvedValueOnce({ status: "conflict", names: ["report.pdf"] })
      .mockResolvedValueOnce({ status: "done", names: ["report (1).pdf"] })
    const { actions } = api({ upload })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    const dialog = await screen.findByRole("alertdialog")
    expect(dialog).toHaveTextContent("“report.pdf” already exists in this folder")
    expect(screen.getByRole("button", { name: "Keep both" })).toHaveFocus()
    await user.click(screen.getByRole("button", { name: "Keep both" }))
    expect(await screen.findByText("Uploaded “report (1).pdf” to dev")).toBeInTheDocument()
    expect(upload.mock.calls.map(([request]) => request.conflict)).toEqual(["ask", "keepBoth"])
  })

  it("replaces on request and sends nothing more when the dialog is cancelled", async () => {
    const user = userEvent.setup()
    const upload = vi.fn().mockResolvedValue({ status: "conflict", names: ["a", "b"] })
    const { actions } = api({ upload })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    const dialog = await screen.findByRole("alertdialog")
    expect(dialog).toHaveTextContent("2 files already exist in this folder")
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }))
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull())
    expect(upload).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("idle"))

    upload.mockResolvedValueOnce({ status: "conflict", names: ["a"] }).mockResolvedValueOnce({ status: "done", names: ["a"] })
    await user.click(screen.getByRole("button", { name: "Upload" }))
    await user.click(await screen.findByRole("button", { name: "Replace" }))
    await waitFor(() => expect(upload).toHaveBeenLastCalledWith(expect.objectContaining({ conflict: "replace" })))
  })

  it("shows progress with a cancel button that stops the active transfer", async () => {
    const user = userEvent.setup()
    let finish!: (outcome: UploadOutcome) => void
    const { actions, emit } = api({ upload: vi.fn(() => new Promise<UploadOutcome>(resolve => { finish = resolve })) })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    await screen.findByText("Uploading “report.pdf” to dev")
    const { id } = vi.mocked(actions.upload).mock.calls[0][0]
    emit({ id, computer: "dev", direction: "upload", state: "transferring", name: "report.pdf", fileIndex: 0, fileCount: 1, bytesDone: 500, bytesTotal: 1000 })
    expect(await screen.findByText(/500 bytes of 1\.0 KB/)).toBeInTheDocument()
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "50")
    expect(screen.getByRole("status")).toHaveTextContent("busy")
    expect(screen.getByRole("button", { name: "Upload" })).toBeDisabled()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(actions.cancel).toHaveBeenCalledWith(id)
    await act(async () => finish({ status: "cancelled" }))
    await waitFor(() => expect(screen.queryByText("Uploading “report.pdf” to dev")).toBeNull())
    expect(screen.getByRole("status")).toHaveTextContent("idle")
  })

  it("ignores progress from another transfer", async () => {
    const user = userEvent.setup()
    let finish!: (outcome: UploadOutcome) => void
    const { actions, emit } = api({ upload: vi.fn(() => new Promise<UploadOutcome>(resolve => { finish = resolve })) })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    await screen.findByText("Uploading “report.pdf” to dev")
    emit({ id: "someone-else", computer: "dev", direction: "upload", state: "transferring", name: "x", fileIndex: 0, fileCount: 1, bytesDone: 1, bytesTotal: 2 })
    expect(screen.queryByText(/of 2 bytes/)).toBeNull()
    await act(async () => finish({ status: "done", names: ["report.pdf"] }))
  })

  it("reports a failed upload with the cause", async () => {
    const user = userEvent.setup()
    const { actions } = api({ upload: vi.fn(async () => { throw "Start this computer to transfer files." }) })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Upload" }))
    expect(await screen.findByText("Upload failed")).toBeInTheDocument()
    expect(screen.getByText("Start this computer to transfer files.")).toBeInTheDocument()
    expect(screen.getByRole("status")).toHaveTextContent("idle")
  })

  it("downloads a file and shows where it was saved", async () => {
    const user = userEvent.setup()
    const { actions } = api()
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Download" }))
    expect(await screen.findByText("Downloaded “a.txt”")).toBeInTheDocument()
    expect(screen.getByText("/Users/ada/Downloads/a.txt")).toBeInTheDocument()
    expect(actions.download).toHaveBeenCalledWith(expect.objectContaining({ computer: "dev", path: "/workspace/a.txt" }))
  })

  it("closes quietly when the save dialog is dismissed and reports a failed download", async () => {
    const user = userEvent.setup()
    const download = vi.fn().mockResolvedValueOnce({ status: "cancelled" }).mockRejectedValueOnce("Symbolic links cannot be downloaded.")
    const { actions } = api({ download })
    render(<Harness actions={actions} />)
    await user.click(screen.getByRole("button", { name: "Download" }))
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("idle"))
    expect(screen.queryByText(/Downloaded/)).toBeNull()
    await user.click(screen.getByRole("button", { name: "Download" }))
    expect(await screen.findByText("Download failed")).toBeInTheDocument()
    expect(screen.getByText("Symbolic links cannot be downloaded.")).toBeInTheDocument()
  })

  it("keeps transfers, progress and the conflict question when the page that started them goes away", async () => {
    const user = userEvent.setup()
    let finish!: (outcome: UploadOutcome) => void
    const upload = vi.fn()
      .mockResolvedValueOnce({ status: "conflict", names: ["report.pdf"] })
      .mockImplementationOnce(() => new Promise<UploadOutcome>(resolve => { finish = resolve }))
    const { actions, emit } = api({ upload })
    function Page() {
      const controls = useFileTransferControls()
      return <button onClick={() => controls?.upload("dev", "/workspace", "dev")}>Upload from page</button>
    }
    function App({ page }: { page: boolean }) {
      return <FileTransfersProvider api={actions}><Toaster />{page ? <Page /> : <p>Another page</p>}</FileTransfersProvider>
    }
    const view = render(<App page />)
    await user.click(screen.getByRole("button", { name: "Upload from page" }))
    const dialog = await screen.findByRole("alertdialog")
    view.rerender(<App page={false} />)
    expect(screen.getByText("Another page")).toBeInTheDocument()
    expect(screen.getByRole("alertdialog")).toBe(dialog)
    await user.click(screen.getByRole("button", { name: "Keep both" }))
    await screen.findByText("Uploading “report.pdf” to dev")
    const { id } = upload.mock.calls[1][0]
    emit({ id, computer: "dev", direction: "upload", state: "transferring", name: "report.pdf", fileIndex: 0, fileCount: 1, bytesDone: 500, bytesTotal: 1000 })
    expect(await screen.findByText(/500 bytes of 1\.0 KB/)).toBeInTheDocument()
    await act(async () => finish({ status: "done", names: ["report (1).pdf"] }))
    expect(await screen.findByText("Uploaded “report (1).pdf” to dev")).toBeInTheDocument()
  })
})
