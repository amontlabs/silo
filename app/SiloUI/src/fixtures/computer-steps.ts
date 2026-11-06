import type { ComputerStep, ComputerTreeNode } from "@/desktop/production-schemas"

/** Steps for the browser preview: two with a checkpoint, one failed, one without. */
const blank = { exitCode: undefined, checkpointId: undefined, parentStep: undefined, parentCheckpoint: undefined }

export const fixtureComputerSteps: ComputerStep[] = ([
  { ...blank, id: "st-0001aaaa", computerId: "fixture", number: 1, command: "npm install", exitCode: 0, startedAt: "2026-11-05T09:00:00Z", finishedAt: "2026-11-05T09:01:10Z", checkpointId: "step-cp-1" },
  { ...blank, id: "st-0002bbbb", computerId: "fixture", number: 2, command: "npm test -- --run src/features/application", exitCode: 1, startedAt: "2026-11-05T09:02:00Z", finishedAt: "2026-11-05T09:02:40Z", checkpointId: "step-cp-2", parentStep: 1 },
  { ...blank, id: "st-0003cccc", computerId: "fixture", number: 3, command: "git status", exitCode: 0, startedAt: "2026-11-05T09:03:00Z", finishedAt: "2026-11-05T09:03:01Z", parentStep: 2 },
] satisfies Partial<ComputerStep>[]) as ComputerStep[]

const none = { requestId: null, buckets: null }

export const fixtureComputerTree: ComputerTreeNode[] = [
  { id: "main", name: "main", parent: null, from: { kind: "root" }, state: "running", pendingChanges: none },
  { id: "fork-a", name: "try-redis", parent: "main", from: { kind: "step", number: 2, checkpointId: "step-cp-2" }, state: "running", pendingChanges: { requestId: "rq-1", buckets: null } },
  { id: "fork-b", name: "try-sqlite", parent: "main", from: { kind: "main" }, state: "stopped", pendingChanges: none },
  { id: "fork-c", name: "redis-tuning", parent: "fork-a", from: { kind: "checkpoint", checkpointId: "cp-9" }, state: "stopped", pendingChanges: none },
]
