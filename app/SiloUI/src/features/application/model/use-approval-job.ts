import { useEffect, useState } from "react"

import type { Approval, ApprovalJob } from "@/desktop/production-schemas"
import type { ApplicationActions } from "./application-source"

/** How often a started job is read while it runs. */
export const APPROVAL_JOB_INTERVAL_MS = 2_000

/** Jobs seen finished, so every row showing the same approval does not read them again. */
const finished = new Map<string, ApprovalJob>()

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
        if (next.state === "finished") finished.set(jobId, next)
        setJob({ id: jobId, job: next })
        if (next.state === "finished") return
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
