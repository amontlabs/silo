import { progressStatuses, type ProgressStatus, type StatusTone } from "@/components/status-tone"
import type { ComputerView, ReviewQueueItemView } from "@/features/onboarding/model/onboarding-state"

export interface StatusPresentation { tone: StatusTone; label: string }

export const reviewQueueStatuses: Record<ReviewQueueItemView["status"], StatusPresentation> = {
  idle: { tone: "neutral", label: "Not started" },
  queued: { tone: "neutral", label: "Waiting" },
  running: progressStatuses.running,
  succeeded: progressStatuses.succeeded,
  failed: progressStatuses.failed,
}

const computerProgress: Record<ComputerView["status"], ProgressStatus> = {
  waiting: "waiting",
  working: "running",
  ready: "succeeded",
  failed: "failed",
}

export const computerStatuses: Record<ComputerView["status"], StatusPresentation> = {
  waiting: progressStatuses[computerProgress.waiting],
  working: progressStatuses[computerProgress.working],
  ready: progressStatuses[computerProgress.ready],
  failed: progressStatuses[computerProgress.failed],
}
