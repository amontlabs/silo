import { z } from "zod"

const bytes = z.number().int().nonnegative()
export const workspaceStorageStateSchema = z.object({
  history: z.array(z.object({
    at: z.number().int().nonnegative(),
    trigger: z.string(),
    reclaimedBytes: bytes.nullable(),
    error: z.string().nullable(),
  })).max(50).default([]),
  /** Host allocation of the workspace disk and its layers; null when Silo could not find it. */
  workspaceHostBytes: bytes.nullable(),
  /** Host allocation of the computer's runtime disks; null when Silo could not find them. */
  runtimeHostBytes: bytes.nullable(),
  /** Host space the computer's checkpoints use; null when it could not be measured. */
  checkpointHostBytes: bytes.nullable().default(null),
  checkpointCount: z.number().int().nonnegative().default(0),
  workspaceUsedBytes: bytes.nullable(),
  workspaceCapacityBytes: bytes.nullable(),
  lastReclaimedBytes: bytes.nullable(),
  lastTrimAt: z.number().int().nonnegative().nullable(),
  lastError: z.string().nullable(),
})

export type WorkspaceStorageState = z.infer<typeof workspaceStorageStateSchema>
