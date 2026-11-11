import { useEffect, useState } from "react"

import type { Approval, ApprovalJob } from "@/desktop/production-schemas"
import type { ApplicationActions } from "./application-source"

/** How often a started job is read while it runs. */
export const APPROVAL_JOB_INTERVAL_MS = 2_000

/** How many finished jobs are remembered; the oldest are forgotten first. */
export const FINISHED_JOBS_KEPT = 50

/** Jobs seen finished, so every row showing the same approval does not read them again. */
const finished = new Map<string, ApprovalJob>()

function remember(id: string, job: ApprovalJob) {
  finished.delete(id)
  finished.set(id, job)
  for (const oldest of finished.keys()) {
    if (finished.size <= FINISHED_JOBS_KEPT) break
    finished.delete(oldest)
  }
}

/** Follows the job an allowed approval started until it ends. `undefined` while unknown or when the approval started no job. */
export function useApprovalJob(read: ApplicationActions["readApprovalJob"], approval: Approval): ApprovalJob | undefined {
  const jobId = approval.state === "allowed" ? approval.outcome?.jobId : undefined
  const [job, setJob] = useState<{ id: string; job: ApprovalJob }>()
  useEffect(() => {
    if (!jobId || !read || finished.has(jobId)) return
    let disposed = false
    let timer: number | undefined
    const poll = async () => {
      try {
        const next = await read(jobId)
        if (disposed) return
        const ended = next.state === "finished" || next.state === "gone"
        if (ended) remember(jobId, next)
        setJob({ id: jobId, job: next })
        if (ended) return
      } catch {
        if (disposed) return
      }
      timer = window.setTimeout(() => void poll(), APPROVAL_JOB_INTERVAL_MS)
    }
    void poll()
    return () => { disposed = true; window.clearTimeout(timer) }
  }, [jobId, read])
  if (!jobId) return undefined
  return finished.get(jobId) ?? (job?.id === jobId ? job.job : undefined)
}
