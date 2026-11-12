import type { Approval, ApprovalJob } from "@/desktop/production-schemas"

const kindLabels: Record<string, string> = {
  merge: "Merge",
  replace_main: "Replace main",
  github: "GitHub access",
  secret: "Secret",
  network: "Network access",
  port: "Published port",
  ssh: "SSH access",
  access: "Access for a fork",
  container: "Container computer",
  resources: "More resources",
}

/** The short name of what an approval is about; a kind from a newer engine reads as its own words. */
export function approvalKindLabel(kind: string): string {
  const known = kindLabels[kind]
  if (known) return known
  const words = kind.replace(/[_-]+/g, " ").trim()
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : "Request"
}

export type ApprovalOutcomeTone = "waiting" | "running" | "done" | "failed" | "neutral"

export interface ApprovalOutcomeText {
  tone: ApprovalOutcomeTone
  /** The result in a few words, or the error. */
  text: string
}

/** What came of the call an allowed approval ran, in a few words. */
function allowedOutcome(approval: Approval, job: ApprovalJob | undefined): ApprovalOutcomeText {
  const { outcome } = approval
  if (!outcome) return { tone: "running", text: "Running…" }
  if (outcome.error) return { tone: "failed", text: `Could not run: ${outcome.error}` }
  if (outcome.jobId) {
    if (job?.state === "gone") return { tone: "neutral", text: "Finished. Silo no longer keeps its result." }
    if (!job || job.state !== "finished") return { tone: "running", text: "Running…" }
    if (job.error) return { tone: "failed", text: `Failed: ${job.error}` }
    if (job.status === "refused") return { tone: "failed", text: job.message ? `Not done: ${job.message}` : "Not done: the engine refused it." }
    return { tone: "done", text: "Done." }
  }
  const result: { status?: unknown; timing?: unknown } = outcome.result && typeof outcome.result === "object" ? outcome.result : {}
  if (approval.kind === "network" && !outcome.jobId) {
    if (result.timing === "nextRestart") return { tone: "done", text: "Done. It applies when the computer restarts." }
    if (result.timing === "nextStart") return { tone: "done", text: "Done. It applies when the computer next starts." }
  }
  const { status } = result
  return { tone: "done", text: typeof status === "string" && status ? `Done (${status}).` : "Done." }
}

/** How an approval stands now: waiting, running, done, failed or closed without running. */
export function approvalOutcomeText(approval: Approval, job?: ApprovalJob): ApprovalOutcomeText {
  switch (approval.state) {
    case "pending": return { tone: "waiting", text: "Waiting for you" }
    case "allowed": return allowedOutcome(approval, job)
    case "denied": return { tone: "neutral", text: "You said no." }
    case "cancelled": return { tone: "neutral", text: approval.reason ?? "Withdrawn." }
    case "expired": return { tone: "neutral", text: "Nobody answered in time." }
  }
}
