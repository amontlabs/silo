import {
  approvalListSchema, approvalShape, auditListSchema, type Approval, type ApprovalJob, type AuditEntry, type DeviceResources, type RequestEntry, type RequestList, type RequestDetail,
  keepRequestResultSchema, requestListSchema,
} from "@/desktop/production-schemas"
import type { ReviewRequestActions } from "@/features/application/components/review-request"
import type { ApplicationActions } from "@/features/application/model/application-source"
import { fixtureReviewRequests } from "./pending-changes"
import { createReviewRequestFixtureActions, reviewRequestFixtures } from "./review-requests"

/** `none`: nothing is asked of the person. `waiting`: several kinds of approvals wait, some were answered or failed, and one merge has conflicts. */
export const approvalFixtureModes = ["none", "waiting"] as const
export type ApprovalFixtureMode = (typeof approvalFixtureModes)[number]

export function approvalFixtureModeFromSearch(search: string): ApprovalFixtureMode | undefined {
  const requested = new URLSearchParams(search).get("approvals")
  return approvalFixtureModes.find(mode => mode === requested)
}

type FixtureComputer = { id: string; name: string }
const defaultComputers: FixtureComputer[] = [{ id: "fixture-main", name: "main" }, { id: "fixture-docs", name: "docs-build" }]

const MINUTE = 60_000
const HOUR = 60 * MINUTE

export type ApprovalFixtureActions = Required<Pick<ApplicationActions, "listApprovals" | "decideApproval" | "watchApprovals" | "readApprovalJob" | "listAudit" | "readDeviceResources" | "listReviewRequests">> & {
  reviewRequestActions: ReviewRequestActions
}

export interface ApprovalFixture {
  actions: ApprovalFixtureActions
  /** The approvals as they stand now. */
  approvals: () => Approval[]
}

const resources: DeviceResources = {
  logicalCpus: 10, memoryTotalBytes: 16 * 2 ** 30, memoryAvailableBytes: 3.1 * 2 ** 30, diskFreeBytes: 214 * 2 ** 30, diskTotalBytes: 494 * 2 ** 30,
  memoryFloorBytes: 6.4 * 2 ** 30, diskFloorBytes: 24.7 * 2 ** 30,
  runningComputers: 2, runningMemoryGib: 8, state: "low", message: "Starting another computer may slow this device.",
  computers: [],
}

const mergeDetail = (id: string, forkName: string, text: string): RequestDetail => ({
  request: { id, forkId: `${id}-fork`, target: "main", createdAt: "2026-11-11T09:00:00Z", reviewDigest: "d1", summary: "", status: "open", counts: { code: 2, settings: 1 }, keepId: undefined, resolvedAt: undefined, keepingSinceMs: undefined, closedReason: undefined, approvalId: undefined },
  forkName, forkState: "running", targetName: undefined, stale: false, reviewDigest: "d1", fresh: true, diff: null, text, counts: { code: 2, settings: 1 }, credentials: { used: [], blocked: [] },
})

/**
 * Approvals the way an engine would hold them, with the review requests of its merges, the
 * audit log and the device's resources. Everything answers from memory, and answering an
 * approval changes what the next read returns and tells the watchers.
 */
export function createApprovalFixture({ mode = "waiting", now = Date.now(), computers = defaultComputers }: { mode?: ApprovalFixtureMode; now?: number; computers?: FixtureComputer[] } = {}): ApprovalFixture {
  const [main = defaultComputers[0]!, second = main] = computers
  const ago = (ms: number) => new Date(now - ms).toISOString()
  const ahead = (ms: number) => new Date(now + ms).toISOString()
  const asked = (id: string, kind: string, computer: FixtureComputer, summary: string, age: number, extra: Record<string, unknown> = {}) => ({
    id, kind, computerId: computer.id, computerName: computer.name, method: "fixture.method", params: {}, summary, state: "pending",
    createdAt: ago(age), expiresAt: ahead(7 * 24 * HOUR - age), ...extra,
  })

  Object.assign(reviewRequestFixtures, {
    "req-conflicts": mergeDetail("req-conflicts", "auth-redirect", "Code\n  ~/app: 46cc034..e710c1a\nSettings & files\n  /etc/app.conf: changed on both sides"),
    "req-fast-forward": mergeDetail("req-fast-forward", "api-docs", "Code\n  ~/docs: 1a2b3c4..5d6e7f8"),
  })
  const requestOf = (id: string, forkName: string, approvalId: string, age: number): RequestEntry => ({
    request: { ...reviewRequestFixtures[id]!.request, createdAt: ago(age), approvalId }, forkName, forkState: "running", targetName: "main",
    deviceId: undefined, deviceName: undefined,
  })

  let list: Approval[] = mode === "none" ? [] : approvalListSchema.parse({ approvals: [
    asked("ap-conflicts", "merge", main, "Merge auth-redirect into main", 4 * MINUTE, { note: "The login page redirected in a loop; this fixes it.", reviewRequestId: "req-conflicts", reviewDigest: "d1" }),
    asked("ap-fast-forward", "merge", main, "Merge api-docs into main", 20 * MINUTE, { reviewRequestId: "req-fast-forward", reviewDigest: "d1" }),
    asked("ap-port", "port", main, "Publish port 8080 on this device", 35 * MINUTE),
    asked("ap-replace", "replace_main", second, "Replace main with try-redis. This restarts main and ends whatever runs there.", 2 * HOUR, { note: "main is out of date; every change in try-redis is wanted." }),
    asked("ap-resources", "resources", main, "Create a computer with 16 CPUs and 32 GiB of memory", 3 * HOUR),
    asked("ap-secret", "secret", main, "Give main the secret STRIPE_KEY for api.stripe.com", 5 * HOUR),
    asked("ap-github", "github", second, "Give docs-build write access to acme/app", 26 * HOUR),
    asked("ap-ssh", "ssh", main, "Allow SSH into main from outside this device on port 2222", 27 * HOUR),
    asked("ap-future", "wallpaper", main, "Change the wallpaper", 28 * HOUR),
    asked("ap-done", "network", main, "Allow main to reach registry.npmjs.org", 30 * HOUR, { state: "allowed", decidedAt: ago(29 * HOUR), decidedBy: "silo-ui", outcome: { result: { status: "applied" } } }),
    asked("ap-running", "container", main, "Create a container computer called ci-runner", 31 * HOUR, { state: "allowed", decidedAt: ago(1 * MINUTE), decidedBy: "silo-ui", outcome: { jobId: "job:ci-runner" } }),
    asked("ap-failed", "resources", main, "Create a computer with 64 GiB of memory", 32 * HOUR, { state: "allowed", decidedAt: ago(31 * HOUR), decidedBy: "silo-ui", outcome: { error: "This device has 3.1 GiB of memory available; Silo keeps 6.4 GiB free." } }),
    asked("ap-denied", "access", second, "Give the fork try-sqlite access to the secret NPM_TOKEN", 33 * HOUR, { state: "denied", decidedAt: ago(32 * HOUR), decidedBy: "silo-ui", decisionNote: "Use a read-only token instead." }),
    asked("ap-expired", "port", main, "Publish port 3000 on this device", 9 * 24 * HOUR, { state: "expired" }),
  ] }).approvals

  const requests: RequestEntry[] = mode === "none" ? [] : [
    requestOf("req-conflicts", "auth-redirect", "ap-conflicts", 4 * MINUTE),
    requestOf("req-fast-forward", "api-docs", "ap-fast-forward", 20 * MINUTE),
  ]
  const watchers = new Set<() => void>()
  const changed = () => watchers.forEach(watch => watch())
  const replace = (id: string, change: Record<string, unknown>) => {
    list = list.map(approval => approval.id === id ? approvalShape.parse({ ...approval, ...change }) : approval)
    changed()
  }
  const closeRequest = (requestId: string, status: "kept" | "thrown-away") => {
    const index = requests.findIndex(({ request }) => request.id === requestId)
    if (index >= 0) requests.splice(index, 1)
    const detail = reviewRequestFixtures[requestId]
    if (detail) reviewRequestFixtures[requestId] = { ...detail, request: { ...detail.request, status } }
  }
  const answerMerge = (requestId: string, decision: "allow" | "deny") => {
    const approval = list.find(item => item.reviewRequestId === requestId && item.state === "pending")
    if (!approval) return
    replace(approval.id, decision === "allow"
      ? { state: "allowed", decidedAt: new Date(now).toISOString(), decidedBy: "silo-ui", outcome: { jobId: `job:keep-${requestId}` } }
      : { state: "denied", decidedAt: new Date(now).toISOString(), decidedBy: "silo-ui" })
  }

  const base = createReviewRequestFixtureActions()
  const reviewRequestActions: ReviewRequestActions = {
    ...base,
    keep: async (target) => {
      if (target.requestId === "req-conflicts" && !target.choices) {
        return keepRequestResultSchema.parse({
          status: "refused",
          report: { fork: "auth-redirect", target: "main", summary: [], steps: [], notes: [], fastForward: false },
          refusal: {
            reason: "choices-needed", message: "Both sides changed 2 items. Choose which side to keep for each one.", hint: "Choose an answer for each item and merge again.",
            conflicts: [
              { kind: "file", item: "/etc/app.conf", detail: "Both changed this file.", id: "file:/etc/app.conf", allowed: ["fork", "main"], forkChange: "changed the port", mainChange: "changed the log level" },
              { kind: "git", item: "/root/app", detail: "The commits cannot be merged cleanly.", id: "repo:/root/app", allowed: ["branch", "main"], forkChange: "3 new commits", mainChange: "2 new commits" },
              { kind: "credentials", item: "/root/.npmrc", detail: "A credential. Silo never brings these over.", id: "", allowed: [], forkChange: "added a token", mainChange: "" },
            ],
          },
          request: reviewRequestFixtures["req-conflicts"]!.request,
        })
      }
      const result = await base.keep(target)
      if (target.requestId === "req-fast-forward") result.report.fastForward = true
      closeRequest(target.requestId, "kept")
      answerMerge(target.requestId, "allow")
      return result
    },
    throwAway: async (target) => {
      const result = await base.throwAway(target)
      closeRequest(target.requestId, "thrown-away")
      answerMerge(target.requestId, "deny")
      return result
    },
  }

  let jobReads = 0
  const callers = [{ kind: "computer", id: main.id }, { kind: "owner" }, { kind: "device", id: "Mac mini" }, { kind: "computer", id: second.id }] as const
  const methods = ["computers.start", "network.publish", "changes.keep", "guest.exec", "checkpoints.create", "secrets.save", "computers.fork", "files.upload"]
  const audit: AuditEntry[] = auditListSchema.parse({ entries: Array.from({ length: 120 }, (_, index) => {
    const caller = callers[index % callers.length]!
    const outcome = (["ok", "ok", "asked", "refused", "error"] as const)[index % 5]!
    return {
      id: 120 - index, at: ago((index + 1) * 7 * MINUTE), callerKind: caller.kind, ...("id" in caller ? { callerId: caller.id } : {}),
      method: methods[index % methods.length]!, computerId: index % 7 === 0 ? undefined : (index % 2 ? main.id : second.id), outcome,
      ...(outcome === "asked" ? { approvalId: "ap-port" } : {}), ...(outcome === "refused" ? { errorCode: "permission_denied", count: 4, lastAt: ago(index * 7 * MINUTE) } : {}),
    }
  }) }).entries

  const actions: ApprovalFixtureActions = {
    listApprovals: async (options) => ({ approvals: list.filter(approval => !options?.state || approval.state === options.state) }),
    decideApproval: async ({ id, decision, note }) => {
      const found = list.find(approval => approval.id === id)
      if (!found || found.state !== "pending") throw new Error("This request is no longer waiting for you.")
      replace(id, decision === "deny"
        ? { state: "denied", decidedAt: new Date(now).toISOString(), decidedBy: "silo-ui", decisionNote: note }
        : { state: "allowed", decidedAt: new Date(now).toISOString(), decidedBy: "silo-ui", decisionNote: note,
          outcome: id === "ap-resources" ? { error: "This device has 3.1 GiB of memory available; Silo keeps 6.4 GiB free." } : id === "ap-secret" ? { jobId: "job:secret" } : { result: { status: "applied" } } })
      return list.find(approval => approval.id === id)!
    },
    watchApprovals: (onChange) => { watchers.add(onChange); return () => { watchers.delete(onChange) } },
    readApprovalJob: async (): Promise<ApprovalJob> => ++jobReads % 3 === 0 ? { state: "finished", error: undefined, status: undefined, message: undefined } : { state: "running", error: undefined, status: undefined, message: undefined },
    listAudit: async ({ computerId, callerKind, beforeId, limit = 100 } = {}) => {
      const rows = audit.filter(row => (!computerId || row.computerId === computerId) && (!callerKind || row.callerKind === callerKind) && (beforeId === undefined || row.id < beforeId))
      const page = rows.slice(0, limit)
      return { entries: page, nextBeforeId: rows.length > limit ? page.at(-1)?.id : undefined }
    },
    readDeviceResources: async () => resources,
    listReviewRequests: async (): Promise<RequestList> => requestListSchema.parse({
      requests: [...requests, ...fixtureReviewRequests.requests],
      unreachable: fixtureReviewRequests.unreachable,
    }),
    reviewRequestActions,
  }
  return { actions, approvals: () => list }
}
