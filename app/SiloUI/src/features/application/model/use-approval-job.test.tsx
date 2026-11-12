import { renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { approvalListSchema, type ApprovalJob } from "@/desktop/production-schemas"

import { approvalOutcomeText } from "./approval-copy"
import { FINISHED_JOBS_KEPT, useApprovalJob } from "./use-approval-job"

const allowed = (id: string, jobId: string) => approvalListSchema.parse({ approvals: [{
  id, kind: "merge", computerId: "c1", computerName: "dev", method: "changes.keep", summary: "Merge",
  state: "allowed", outcome: { jobId }, createdAt: "2026-11-11T10:00:00Z",
}] }).approvals[0]!
const done = { state: "finished", status: "Done" } as unknown as ApprovalJob

describe("useApprovalJob", () => {
  it("reads a finished job once for every row that shows it", async () => {
    const read = vi.fn().mockResolvedValue(done)
    const first = renderHook(() => useApprovalJob(read, allowed("a1", "shared-job")))
    await waitFor(() => expect(first.result.current?.state).toBe("finished"))
    const second = renderHook(() => useApprovalJob(read, allowed("a1", "shared-job")))
    expect(second.result.current?.state).toBe("finished")
    expect(read).toHaveBeenCalledTimes(1)
  })

  it("forgets the oldest finished jobs", async () => {
    const read = vi.fn().mockResolvedValue(done)
    for (let index = 0; index <= FINISHED_JOBS_KEPT; index += 1) {
      const { result, unmount } = renderHook(() => useApprovalJob(read, allowed(`a${index}`, `old-job-${index}`)))
      await waitFor(() => expect(result.current?.state).toBe("finished"))
      unmount()
    }
    expect(read).toHaveBeenCalledTimes(FINISHED_JOBS_KEPT + 1)
    const newest = renderHook(() => useApprovalJob(read, allowed("x", `old-job-${FINISHED_JOBS_KEPT}`)))
    expect(newest.result.current?.state).toBe("finished")
    expect(read).toHaveBeenCalledTimes(FINISHED_JOBS_KEPT + 1)
    const oldest = renderHook(() => useApprovalJob(read, allowed("y", "old-job-0")))
    expect(oldest.result.current).toBeUndefined()
    await waitFor(() => expect(read).toHaveBeenCalledTimes(FINISHED_JOBS_KEPT + 2))
  })

  it("stops reading a job the engine no longer holds", async () => {
    const read = vi.fn().mockResolvedValue({ state: "gone" } as unknown as ApprovalJob)
    const { result } = renderHook(() => useApprovalJob(read, allowed("g1", "gone-job")))
    await waitFor(() => expect(result.current?.state).toBe("gone"))
    expect(approvalOutcomeText(allowed("g1", "gone-job"), result.current)).toEqual({ tone: "neutral", text: "Finished. Silo no longer keeps its result." })
    expect(read).toHaveBeenCalledTimes(1)
  })
})
