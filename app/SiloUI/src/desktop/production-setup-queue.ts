import type { SetupQueueItemID, SiloProgressEvent } from "@/contracts/silo"
import type { OnboardingSource } from "@/features/onboarding/model/onboarding-source"

import type { ProductionContext } from "./production-context"
import { errorMessage } from "./production-helpers"

export type SetupItem = NonNullable<OnboardingSource["setupQueue"]>[number]
export type SetupJob = { items: SetupItem[]; activityId: string }

/** The ordered queue of setup work (computers, Git identities, GitHub access, completion) and its progress as shown by the setup surface. */
export function createSetupQueue({ snapshot, publish, disposed }: ProductionContext) {
  let jobs: SetupJob[] = []
  let activeComputerJob: SetupJob | undefined
  let tail: Promise<unknown> = Promise.resolve()
  let accepting = true
  // Setup waits (GitHub access polling) end early when Quit drains setup.
  const waits = new Set<() => void>()

  function project() {
    const base = snapshot()
    publish({ ...base, setupQueue: base.setupQueue.map((item) => {
      const states = jobs.flatMap((job) => job.items.filter(({ id }) => id === item.id))
      return states.find(({ status }) => status === "running") ?? states.find(({ status }) => status === "queued") ?? states.at(-1) ?? item
    }) })
  }

  function setStatus(ids: SetupQueueItemID[], status: SetupItem["status"], failure?: string) {
    jobs = jobs.filter((job) => !job.items.some(({ id }) => ids.includes(id)) || job.items.some(({ status }) => status === "running" || status === "queued"))
    const base = snapshot()
    publish({ ...base, setupQueue: base.setupQueue.map((item) => ids.includes(item.id) ? { id: item.id, status, ...(failure && { failure }) } : item) })
    project()
  }

  function setJobStatus(job: SetupJob, ids: SetupQueueItemID[], status: SetupItem["status"], failure?: string) {
    job.items = job.items.map((item) => ids.includes(item.id) ? { id: item.id, status, ...(failure && { failure }) } : item)
    project()
  }

  function recordGitHubActivity(requestId: string, phase: "github" | "identity", message: string, failed = false) {
    const event: SiloProgressEvent = { schemaVersion: 1, type: "progress", requestId, phase, step: `${phase}-setup`, timestamp: Date.now(), level: failed ? "error" : "info", message, safeForDisplay: true }
    const base = snapshot()
    publish({ ...base, setupActivity: [...(base.setupActivity ?? []), event].slice(-500) })
  }

  function enqueue<T>(ids: SetupQueueItemID[], work: (job: SetupJob) => Promise<T>, activityId = crypto.randomUUID()): Promise<T> {
    setStatus(ids, "queued")
    const activityPhase = ids.includes("githubRun") ? "github" : ids.includes("identityRun") ? "identity" : null
    const activityLabel = activityPhase === "github" ? "GitHub access" : "Git identity"
    const job: SetupJob = { activityId, items: ids.map((id) => ({ id, status: "queued" })) }
    jobs.push(job)
    project()
    const promise = tail.then(async () => {
      if (disposed()) throw new Error("Silo was closed before the setup task started.")
      setJobStatus(job, [ids[0]], "running")
      if (activityPhase) recordGitHubActivity(activityId, activityPhase, `${activityLabel}: applying settings.`)
      try {
        const result = await work(job)
        setJobStatus(job, ids, "succeeded")
        if (activityPhase) recordGitHubActivity(activityId, activityPhase, `${activityLabel}: setup complete.`)
        return result
      } catch (cause) {
        setJobStatus(job, ids, "failed", errorMessage(cause))
        if (activityPhase) recordGitHubActivity(activityId, activityPhase, `${activityLabel}: setup failed. Review the reported error before retrying.`, true)
        throw cause
      }
    })
    tail = promise.catch(() => {})
    return promise
  }

  function delay(ms: number) {
    return new Promise<void>((resolve) => {
      const done = () => { window.clearTimeout(timer); waits.delete(done); resolve() }
      const timer = window.setTimeout(done, ms)
      waits.add(done)
    })
  }

  function isBusy() {
    return jobs.some((job) => job.items.some(({ status }) => status === "running" || status === "queued"))
  }

  /** What Quit is waiting for while setup drains, for the shutdown overlay. */
  function pendingWork(): string | undefined {
    const pending = new Set(jobs.flatMap((job) => job.items.filter(({ status }) => status === "running" || status === "queued").map(({ id }) => id)))
    const steps = [
      (pending.has("computerRun") || pending.has("computerVerify")) && "creating computers",
      (pending.has("identityRun") || pending.has("identityVerify")) && "applying Git identities",
      (pending.has("githubRun") || pending.has("githubVerify")) && "verifying GitHub access",
      pending.has("completion") && "saving setup",
    ].filter((step): step is string => Boolean(step))
    return steps.length ? `Finishing setup (${steps.join(", ")})…` : undefined
  }

  /** Stops accepting setup, wakes any wait, and resolves once the accepted work has finished. */
  async function drain() {
    accepting = false
    // Accepted setup finishes, but nothing waits minutes for GitHub to confirm access.
    ;[...waits].forEach((wake) => wake())
    const pending = pendingWork()
    if (pending) publish({ ...snapshot(), setupDrain: pending })
    try { await tail }
    finally { if (snapshot().setupDrain) publish({ ...snapshot(), setupDrain: undefined }) }
  }

  return {
    enqueue,
    setStatus,
    setJobStatus,
    recordGitHubActivity,
    delay,
    isBusy,
    drain,
    settled: () => tail,
    wake: () => waits.forEach(wake => wake()),
    isAccepting: () => accepting,
    resume: () => { accepting = true },
    get activeComputerJob() { return activeComputerJob },
    set activeComputerJob(job: SetupJob | undefined) { activeComputerJob = job },
  }
}
