import type {
  SetupQueueItemID,
  SiloPreflightCheck,
  SiloProgressEvent,
} from "@/contracts/silo"
import type { GitHubConnectionState, OnboardingSource } from "@/features/onboarding/model/onboarding-source"

export const onboardingSteps = ["dependencies", "computers", "github", "review"] as const
export type OnboardingStep = (typeof onboardingSteps)[number]

export type PresentationStatus = "waiting" | "running" | "succeeded" | "failed"

export interface DependencyItemView {
  name: string
  check?: SiloPreflightCheck
}

export interface DependencyGroupView {
  id: string
  title: string
  status: "succeeded" | "failed" | "running"
  items: DependencyItemView[]
}

export interface ComputerView {
  name: string
  status: "waiting" | "working" | "ready" | "failed"
  detail: string
}

export interface ComputerProgressView {
  status: PresentationStatus
  elapsedSeconds: number
  currentComputer?: string
  currentMessage: string
  completedOperations: number
  totalOperations: number
  fraction?: number
  computers: ComputerView[]
  visibleEvents: SiloProgressEvent[]
  activityError?: string
  readyCount: number
  workingCount: number
  waitingCount: number
  failedCount: number
  recovery?: string
  retryable: boolean
}

export interface ReviewQueueItemView {
  id: SetupQueueItemID
  label: string
  status: "idle" | "queued" | "running" | "succeeded" | "failed"
  failure?: string
}

export interface OnboardingViewModel {
  dependencies: DependencyGroupView[]
  dependencyStatus: PresentationStatus
  computerProgress: ComputerProgressView
  queueItems: ReviewQueueItemView[]
  finishEnabled: boolean
  /** Why Finish is unavailable once computer setup settled, and how to resolve it. */
  finishBlocker: NonNullable<OnboardingSource["finishBlocker"]> | null
  error: OnboardingSource["error"]
  stepStatus: Record<OnboardingStep, PresentationStatus>
}

const inventory = [
  {
    id: "system",
    title: "System",
    items: [
      ["Supported OS", "system-os"],
      ["Virtualization", "system-virtualization"],
    ],
  },
  {
    id: "bundled-tools",
    title: "Bundled tools",
    items: [
      ["Computer runtime", "runtime-microsandbox"],
      ["Git", "tool-git"],
      ["Git LFS", "tool-git-lfs"],
    ],
  },
] as const

const queueByStep: Record<"computers" | "review", SetupQueueItemID[]> = {
  computers: ["computerRun", "computerVerify"],
  review: ["completion"],
}

const computerOperationSteps = new Set([
  "computer-configuration",
  "computer-networking",
  "computer-verification",
])

function combineQueueStatus(items: ReviewQueueItemView[]): PresentationStatus {
  if (items.some(({ status }) => status === "failed")) return "failed"
  if (items.some(({ status }) => status === "running")) return "running"
  if (items.length > 0 && items.every(({ status }) => status === "succeeded")) return "succeeded"
  return "waiting"
}

function computerDetail(event: SiloProgressEvent): string {
  if (event.fraction === 1 && event.step === "computer-verification") return "Ready"
  return event.message
}

const queueLabels: Record<SetupQueueItemID, string> = {
  computerRun: "Create computers",
  computerVerify: "Verify computers",
  githubRun: "Save GitHub access",
  githubVerify: "Check GitHub access",
  identityRun: "Save Git identities",
  identityVerify: "Verify Git identities",
  completion: "Finish setup",
}

function projectQueue(source: OnboardingSource): ReviewQueueItemView[] {
  if (source.setupQueue) return source.setupQueue.map((item) => ({ ...item, label: queueLabels[item.id] }))
  const ids = Object.keys(queueLabels) as SetupQueueItemID[]
  const completedPhases = new Set(source.bootstrapState.completedPhases)
  const computerBoundaryComplete = completedPhases.has("computers") && source.bootstrapResult?.phase === "complete"
  const allSetupComplete = completedPhases.has("complete") && computerBoundaryComplete && !source.error
  const isVerifying = source.progressEvents.some(({ step }) => step === "computer-verification")
  return ids.map((id) => {
    if (allSetupComplete) return { id, label: queueLabels[id], status: "succeeded" }
    if (computerBoundaryComplete && (id === "computerRun" || id === "computerVerify")) {
      return { id, label: queueLabels[id], status: "succeeded" }
    }
    if (completedPhases.has("github") && (id === "githubRun" || id === "githubVerify")) {
      return { id, label: queueLabels[id], status: "succeeded" }
    }
    if (completedPhases.has("identity") && (id === "identityRun" || id === "identityVerify")) {
      return { id, label: queueLabels[id], status: "succeeded" }
    }
    if (source.error && id === (isVerifying ? "computerVerify" : "computerRun")) {
      return { id, label: queueLabels[id], status: "failed", failure: source.error.message }
    }
    if (source.progressEvents.length > 0) {
      if (id === "computerRun") return { id, label: queueLabels[id], status: isVerifying ? "succeeded" : "running" }
      if (id === "computerVerify" && isVerifying && !source.error) return { id, label: queueLabels[id], status: "running" }
    }
    return { id, label: queueLabels[id], status: "queued" }
  })
}

function projectComputerProgress(source: OnboardingSource, queueItems: ReviewQueueItemView[]): ComputerProgressView {
  const activeRevision = source.progressEvents.findLast(({ revision }) => revision !== undefined)?.revision
  const activeEvents = source.progressEvents.filter((event) => (
    !activeRevision || !event.revision || event.revision === activeRevision
  ))
  const visibleEvents = activeEvents.filter(({ safeForDisplay }) => safeForDisplay)
  const completionKeys = new Set(
    activeEvents
      .filter((event) => event.computer && event.step && computerOperationSteps.has(event.step) && event.fraction === 1)
      .map((event) => `${event.computer}:${event.step}`),
  )
  const currentEvent = visibleEvents.at(-1)
  const failedComputer = source.error?.computer ?? undefined
  const latestByComputer = new Map<string, SiloProgressEvent>()
  for (const event of activeEvents) {
    if (event.computer) latestByComputer.set(event.computer, event)
  }

  const computerVerified = source.setupQueue ? source.setupQueue.some(({ id, status }) => id === "computerVerify" && status === "succeeded") : source.bootstrapState.completedPhases.includes("computers") && source.bootstrapResult?.phase === "complete"
  const computers = source.bootstrapConfiguration.computers.map(({ name }): ComputerView => {
    const latest = latestByComputer.get(name)
    if (failedComputer === name) {
      return { name, status: "failed", detail: source.error?.message ?? "Setup failed" }
    }
    if (computerVerified || (!source.setupQueue && latest?.step === "computer-verification" && latest.fraction === 1)) {
      return { name, status: "ready", detail: "Ready" }
    }
    if (latest && latest === currentEvent && latest.fraction !== 1) {
      return { name, status: "working", detail: computerDetail(latest) }
    }
    if (latest?.step === "computer-networking" && latest.fraction === 1) {
      return { name, status: "waiting", detail: "Waiting for verification" }
    }
    if (latest?.step === "computer-configuration" && latest.fraction === 1) {
      return { name, status: "waiting", detail: "Waiting for networking" }
    }
    if (latest) return { name, status: "waiting", detail: "Waiting" }
    return { name, status: "waiting", detail: "Waiting" }
  })

  const queueStatus = combineQueueStatus(queueItems.filter(({ id }) => queueByStep.computers.includes(id)))
  const recordedProgress = !source.setupQueue && activeEvents.some(({ step }) => step && computerOperationSteps.has(step))
  const idleComputerQueue = source.setupQueue?.filter(({ id }) => queueByStep.computers.includes(id)).every(({ status }) => status === "idle") === true
  const pendingMessage = source.setupQueue?.some(({ id, status }) => queueByStep.computers.includes(id) && status === "queued") ? "Computer setup is queued" : source.setupQueue ? "Continue to create computers" : "Waiting to create computers"
  const totalOperations = computers.length * (recordedProgress ? 3 : 2)
  const completedOperations = recordedProgress ? completionKeys.size : queueItems.filter(({ id, status }) => queueByStep.computers.includes(id) && status === "succeeded").length * computers.length
  return {
    status: queueStatus,
    elapsedSeconds: source.bootstrapState.startedAt !== undefined
      ? Math.max(0, source.bootstrapState.updatedAt - source.bootstrapState.startedAt)
      : 0,
    currentComputer: currentEvent?.computer,
    currentMessage: source.error?.message ?? (idleComputerQueue ? "Continue to create computers" : currentEvent?.message ?? source.bootstrapResult?.message ?? pendingMessage),
    completedOperations,
    totalOperations,
    fraction: totalOperations > 0 ? completedOperations / totalOperations : undefined,
    computers,
    visibleEvents: source.activityEvents?.filter(({ safeForDisplay }) => safeForDisplay) ?? visibleEvents,
    activityError: source.activityError,
    readyCount: computers.filter(({ status }) => status === "ready").length,
    workingCount: computers.filter(({ status }) => status === "working").length,
    waitingCount: computers.filter(({ status }) => status === "waiting").length,
    failedCount: computers.filter(({ status }) => status === "failed").length,
    recovery: source.error?.recovery ?? undefined,
    retryable: source.error?.retryable ?? false,
  }
}

export function projectOnboarding(source: OnboardingSource, githubConnectionState: GitHubConnectionState): OnboardingViewModel {
  const checksById = new Map(source.preflightChecks.map((check) => [check.id, check]))
  const dependencies = inventory.map((group): DependencyGroupView => {
    const items = group.items.map(([name, checkId]) => {
      const check = checksById.get(checkId) ?? {
        id: checkId,
        title: name,
        status: "unavailable" as const,
        detail: "No check result was reported.",
        remediation: null,
      }
      return { name, check }
    })
    const status = items.some(({ check }) => check?.status === "pending")
      ? "running" as const
      : items.some(({ check }) => check && check.status !== "pass") ? "failed" as const : "succeeded" as const
    return {
      id: group.id,
      title: group.title,
      status,
      items,
    }
  })
  const dependencyStatus = dependencies.some(({ status }) => status === "running")
    ? "running"
    : dependencies.some(({ status }) => status === "failed") ? "failed" : "succeeded"
  const queueItems = projectQueue(source)
  const computerProgress = projectComputerProgress(source, queueItems)
  const stepStatus = {
    dependencies: dependencyStatus,
    computers: computerProgress.status,
    github: source.setupQueue ? combineQueueStatus(queueItems.filter(({ id }) => ["githubRun", "githubVerify", "identityRun", "identityVerify"].includes(id))) : githubConnectionState === "connected"
      ? "succeeded"
      : githubConnectionState === "connecting" ? "running" : "waiting",
    review: combineQueueStatus(queueItems.filter(({ id }) => queueByStep.review.includes(id))),
  } satisfies Record<OnboardingStep, PresentationStatus>

  return {
    dependencies,
    dependencyStatus,
    computerProgress,
    queueItems,
    finishEnabled: dependencyStatus === "succeeded" && source.error === null && (source.readyToFinish ?? queueItems.every(({ status }) => status === "succeeded")),
    finishBlocker: source.error === null ? source.finishBlocker ?? null : null,
    error: source.error,
    stepStatus,
  }
}
