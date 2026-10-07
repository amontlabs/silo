import { act, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { showOperationFailure, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"
import type { WorkspaceStorageState } from "../model/workspace-storage"
import { WorkspaceStoragePanel } from "./workspace-storage-panel"

vi.mock("@/lib/operation-toast", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/operation-toast")>(),
  showOperationFailure: vi.fn(),
  showOperationProgress: vi.fn(),
  showOperationSuccess: vi.fn(),
}))

beforeEach(() => { vi.clearAllMocks() })

const storage: WorkspaceStorageState = {
  history: [], workspaceHostBytes: 2 * 1024 ** 2, runtimeHostBytes: 0,
  checkpointHostBytes: 0, checkpointCount: 0, workspaceUsedBytes: 0,
  workspaceCapacityBytes: 4 * 1024 ** 2, lastReclaimedBytes: null,
  lastTrimAt: null, lastError: null,
}

it.each(["success", "lastError", "rejection"])("settles a reclaim notification after Storage unmounts (%s)", async outcome => {
  let finish!: (value: WorkspaceStorageState) => void
  let fail!: (cause: Error) => void
  const read = vi.fn().mockResolvedValue(storage)
  const reclaim = vi.fn(() => new Promise<WorkspaceStorageState>((resolve, reject) => { finish = resolve; fail = reject }))
  const user = userEvent.setup()
  const view = render(<WorkspaceStoragePanel computerId="vm-id" computerName="dev" running read={read} reclaim={reclaim} />)
  await screen.findByText("2 MiB")
  await user.click(screen.getByRole("button", { name: "Free up space" }))
  expect(reclaim).toHaveBeenCalledExactlyOnceWith("vm-id")
  expect(showOperationProgress).toHaveBeenCalledWith("storage-reclaim:vm-id", expect.objectContaining({ title: "Freeing up space" }))
  view.unmount()
  await act(async () => {
    if (outcome === "rejection") fail(new Error("Trim failed"))
    else finish({ ...storage, lastReclaimedBytes: 1024 ** 2, lastError: outcome === "lastError" ? "Trim failed" : null })
  })
  if (outcome === "success") {
    expect(showOperationSuccess).toHaveBeenCalledWith("storage-reclaim:vm-id", "Freed 1 MiB", expect.objectContaining({ noticeComputer: { id: "vm-id", name: "dev" } }))
  } else {
    expect(showOperationFailure).toHaveBeenCalledWith("storage-reclaim:vm-id", "Could not free up space", expect.objectContaining({ description: "Trim failed" }))
    const options = vi.mocked(showOperationFailure).mock.calls.find(call => call[0] === "storage-reclaim:vm-id")![2]!
    expect(options.retry).toBeUndefined()
    expect(read).toHaveBeenCalledOnce()
  }
})
