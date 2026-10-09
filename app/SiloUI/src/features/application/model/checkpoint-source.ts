import { z } from "zod"
import type { ApplicationComputer } from "./application-source"

export interface ComputerCheckpoint {
  id: string
  name: string
  createdAt: string
  scope: "full" | "disk"
  reason: "manual" | "before-restore"
  sizeBytes?: number
}

export interface ComputerCheckpointOperation {
  kind: "capture" | "fork" | "restore" | "delete"
  status: "running" | "failed"
  stage: string
  error?: string
}

/** A Restore that did not finish (`unfinishedRestore` in the computer view). */
export interface UnfinishedRestore {
  /** The checkpoint being restored. */
  checkpointId: string
  checkpointName?: string | null
  /** "capturing" until the recovery checkpoint is saved, then "secured". */
  phase: "capturing" | "secured"
}

export interface PendingCheckpointRestore {
  checkpointId: string
  sourceComputer: string
  state: "full" | "disk"
}

/** Computer names already used on one device (`undefined` for this device), so a fork name conflict shows inline. */
export function computerNamesOnDevice(computers: readonly ApplicationComputer[], deviceId: string | undefined, localMacosNames: readonly string[] = []): string[] {
  const names = computers.filter(computer => (computer.device?.id ?? "") === (deviceId ?? "")).map(computer => computer.configuration.name)
  // macOS computers exist only on this device.
  return deviceId ? names : [...names, ...localMacosNames]
}

/** Storage and Delete availability per checkpoint (`read_checkpoint_usage`). */
export const checkpointUsageSchema = z.object({
  /** Host space this computer's checkpoints use, each saved state counted once; null when unknown. */
  totalBytes: z.number().int().nonnegative().nullable(),
  checkpoints: z.array(z.object({
    id: z.string().min(1),
    sizeBytes: z.number().int().nonnegative().optional(),
    /** Other computers that depend on the checkpoint. */
    usedBy: z.array(z.string()).optional(),
    /** Why Delete is unavailable; absent when the checkpoint can be deleted. */
    deleteBlocker: z.string().optional(),
  })),
})
export type CheckpointUsage = z.infer<typeof checkpointUsageSchema>
