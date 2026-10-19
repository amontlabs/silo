import type { SetupComputerConfiguration } from "@/contracts/silo"
import { divergentComputerFields, sameComputerConfiguration } from "@/features/application/model/computer-change"

/** A field changed both here and elsewhere since the editor opened. */
export interface ComputerReviewConflict {
  field: string
  label: string
  theirs: string
  mine: string
}

/** What "Review changes" did to the draft after a stale-baseline rejection. */
export interface ComputerReview {
  /** Fields changed on both sides; the draft keeps the user's value. */
  conflicts: ComputerReviewConflict[]
  /** Fields changed only elsewhere; the draft now has their value. */
  adopted: string[]
}

const fieldLabels = new Map([
  ["name", "Name"], ["cpus", "CPUs at start"], ["maxCPUs", "Maximum CPUs"],
  ["memoryGiB", "Memory at start"], ["maxMemoryGiB", "Maximum memory"],
  ["workspaceStorageGiB", "Workspace disk"], ["runtimeStorageGiB", "Runtime disk"],
  ["desktop", "Linux desktop"],
])

/** The editor's label for a configuration field. */
export function computerFieldLabel(field: string): string {
  return fieldLabels.get(field) ?? field
}

function fieldValue(field: string, value: unknown): string {
  if (field === "cpus" || field === "maxCPUs") return `${value} ${value === 1 ? "CPU" : "CPUs"}`
  if (field === "memoryGiB" || field === "maxMemoryGiB" || field === "workspaceStorageGiB" || field === "runtimeStorageGiB") return `${value} GiB`
  if (field === "desktop") {
    if (!value) return "Not installed"
    return (value as { startWithComputer?: boolean }).startWithComputer === false ? "Starts from its viewer" : "Starts with computer"
  }
  return String(value)
}

/**
 * Rebase the user's draft onto the latest saved configuration: fields the user edited keep
 * their value, fields changed only elsewhere take the latest value (so saving does not
 * silently revert someone else's change), and fields edited on both sides are listed with
 * both values for review.
 */
export function rebaseComputerDraft(opened: SetupComputerConfiguration, latest: SetupComputerConfiguration, draft: SetupComputerConfiguration): { draft: SetupComputerConfiguration; review: ComputerReview } {
  const fields = new Set([...Object.keys(opened), ...Object.keys(latest), ...Object.keys(draft)])
  const edited = new Set(divergentComputerFields(opened, draft))
  const changedElsewhere = new Set(divergentComputerFields(opened, latest))
  const disagree = new Set(divergentComputerFields(latest, draft))
  const mineValues = new Map<string, unknown>(Object.entries(draft))
  const theirValues = new Map<string, unknown>(Object.entries(latest))
  const merged = new Map<string, unknown>()
  const conflicts: ComputerReviewConflict[] = []
  const adopted: string[] = []
  for (const field of fields) {
    const value = edited.has(field) ? mineValues.get(field) : theirValues.get(field)
    if (value !== undefined) merged.set(field, structuredClone(value))
    if (edited.has(field) && changedElsewhere.has(field) && disagree.has(field)) {
      conflicts.push({ field, label: computerFieldLabel(field), theirs: fieldValue(field, theirValues.get(field)), mine: fieldValue(field, mineValues.get(field)) })
    } else if (changedElsewhere.has(field) && !edited.has(field)) adopted.push(computerFieldLabel(field))
  }
  const rebased = Object.fromEntries(merged) as SetupComputerConfiguration
  return { draft: sameComputerConfiguration(rebased, draft) ? draft : rebased, review: { conflicts, adopted } }
}
