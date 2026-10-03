import { describe, expect, it, vi } from "vitest"
import { createProductionSource, type ProductionBridge } from "./production-source"

function setup(replies: Record<string, unknown>) {
  const invoke = vi.fn(async (command: string) => replies[command])
  let deliver: ((event?: { payload: unknown }) => void) | undefined
  const listen = vi.fn(async (_event: string, handler: (event?: { payload: unknown }) => void) => { deliver = handler; return () => {} })
  const production = createProductionSource({ invoke, listen } as unknown as ProductionBridge)
  const transfers = production.applicationActions.fileTransfers!
  return { invoke, listen, transfers, production, deliver: (payload: unknown) => deliver?.({ payload }) }
}

describe("production file transfers", () => {
  it("passes ids, folders and conflict policies to the native commands and validates replies", async () => {
    const { invoke, transfers, production } = setup({
      choose_upload_files: ["/Users/ada/a.txt"],
      upload_files: { status: "conflict", names: ["a.txt"] },
      download_file: { status: "done", path: "/Users/ada/Downloads/a.txt" },
      cancel_transfer: null,
    })
    try {
      expect(await transfers.chooseUploadFiles()).toEqual(["/Users/ada/a.txt"])
      expect(await transfers.upload({ id: "t1", computer: "dev", directory: "/workspace", paths: ["/Users/ada/a.txt"], conflict: "ask" })).toEqual({ status: "conflict", names: ["a.txt"] })
      expect(await transfers.download({ id: "t2", computer: "dev", path: "/workspace/a.txt" })).toEqual({ status: "done", path: "/Users/ada/Downloads/a.txt" })
      await transfers.cancel("t2")
      expect(invoke.mock.calls).toEqual([
        ["choose_upload_files"],
        ["upload_files", { transferId: "t1", computer: "dev", directory: "/workspace", paths: ["/Users/ada/a.txt"], conflict: "ask" }],
        ["download_file", { transferId: "t2", computer: "dev", path: "/workspace/a.txt" }],
        ["cancel_transfer", { transferId: "t2" }],
      ])
    } finally { production.dispose() }
  })

  it("rejects replies that are not a known outcome", async () => {
    const { transfers, production } = setup({ upload_files: { status: "unknown" }, download_file: { status: "done" } })
    try {
      await expect(transfers.upload({ id: "t", computer: "dev", directory: "/workspace", paths: ["/a"], conflict: "ask" })).rejects.toThrow()
      await expect(transfers.download({ id: "t", computer: "dev", path: "/workspace/a" })).rejects.toThrow()
    } finally { production.dispose() }
  })

  it("delivers only well-formed progress events", async () => {
    const { transfers, listen, deliver, production } = setup({})
    try {
      const seen = vi.fn()
      await transfers.onProgress(seen)
      expect(listen).toHaveBeenCalledWith("silo://transfer-progress", expect.any(Function))
      deliver({ id: "t", computer: "dev", direction: "upload", state: "transferring", name: "a", fileIndex: 0, fileCount: 1, bytesDone: 1, bytesTotal: 2 })
      deliver({ id: "t", state: "exploded" })
      expect(seen).toHaveBeenCalledTimes(1)
      expect(seen.mock.calls[0][0]).toMatchObject({ id: "t", bytesDone: 1 })
    } finally { production.dispose() }
  })
})
