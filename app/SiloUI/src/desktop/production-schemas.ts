import { defaultSettings, settingSchemas } from "@/features/preferences/model/settings"
import { z } from "zod"
import { quietStateSchema } from "@/features/macos-computers/model/macos-computers"

import { siloBootstrapResultSchema, siloProgressEventSchema, siloProtocolErrorSchema, setupComputerConfigurationRequestSchema } from "@/contracts/silo"
import type { SshAccessState, NetworkState, ApplicationFileEntry, ApplicationSource } from "@/features/application/model/application-source"
import type { BackupState } from "@/features/application/model/backup-source"

const sshAccessShape = z.object({ computers: z.array(z.object({
  computer: z.string(), user: z.string().optional(), enabled: z.boolean(), port: z.number().int(), bindAddress: z.string(), keys: z.array(z.string()),
  state: z.enum(["disabled", "waiting", "listening", "error"]), message: z.string().nullable(), fingerprint: z.string().nullable(), deviceName: z.string(), addresses: z.array(z.string()),
})) })

export const githubStateShape = z.object({
  // Superseded by token secrets; still sent for the secret named `github-token`.
  personalToken: z.object({ state: z.enum(["connected", "disconnected"]), saved: z.boolean(), account: z.string().optional(), message: z.string().optional() }).optional(),
  policyRevision: z.number().int().nonnegative().optional(),
  state: z.enum(["disconnected", "connecting", "connected"]),
  account: z.string().nullish().transform((value) => value ?? undefined),
  accessEnabled: z.boolean().optional(),
  deviceIdentity: z.object({ name: z.string(), email: z.string() }).nullable().optional(),
  repositoryCatalog: z.array(z.string()).optional(),
  repositoryCatalogStatus: z.discriminatedUnion("status", [
    z.object({ status: z.literal("available") }),
    z.object({ status: z.literal("unavailable"), message: z.string(), canRetry: z.boolean() }),
  ]).optional(),
  computers: z.array(z.object({
    computer: z.string(), identity: z.object({ name: z.string(), email: z.string(), apply: z.boolean() }),
    authenticationMethod: z.enum(["oauth", "token"]).nullish().transform(value => value ?? undefined),
    tokenSecretId: z.string().nullish().transform(value => value ?? undefined),
    tokenMissing: z.boolean().optional(),
    repositoryMode: z.enum(["selected", "all"]).default("selected"), allRepositoriesAllowChanges: z.boolean().default(false),
    repositories: z.array(z.object({ repository: z.string(), allowPushes: z.boolean() })),
  })).optional(),
  computerOperations: z.array(z.discriminatedUnion("status", [
    z.object({ computer: z.string(), status: z.literal("applying"), message: z.string() }),
    z.object({ computer: z.string(), status: z.literal("succeeded"), message: z.string() }),
    z.object({ computer: z.string(), status: z.literal("failed"), message: z.string(), canRetry: z.literal(true), diagnosticDetails: z.string().optional() }),
  ])).optional(),
})

export const directoryPageShape = z.object({
  snapshotId: z.string().min(1),
  entries: z.array(z.object({ name: z.string().min(1), path: z.string().refine(path => path.startsWith("/workspace/") || path.startsWith("/Users/silo/workspace/")), kind: z.enum(["folder", "file", "symlink"]) }).strict()).max(200),
  nextOffset: z.number().int().nonnegative().nullable(),
}).strict()

export const secretShape = z.object({
  id: z.string(), name: z.string(), kind: z.enum(["generic", "github-token"]).optional(),
  computers: z.array(z.string()), allowedDomains: z.array(z.string()),
  account: z.string().nullish().transform((value) => value ?? undefined),
  connection: z.object({ state: z.enum(["connected", "disconnected"]), message: z.string().nullish().transform((value) => value ?? undefined) }).nullish().transform((value) => value ?? undefined),
  state: z.enum(["active", "applying", "restart-required"]), pendingComputers: z.array(z.string()).optional(),
  error: z.string().nullish().transform((value) => value ?? undefined), removing: z.boolean().optional(),
})

export const checkpointShape = z.object({
  id: z.string().min(1), name: z.string().min(1),
  // The Rust checkpoint journal stores Unix milliseconds (the same u64
  // contract as activity timestamps); normalize at the native boundary for
  // the UI's ISO timestamp model. String values remain accepted for remotes.
  createdAt: z.union([
    z.string().datetime(),
    z.number().int().nonnegative().max(8.64e15).transform(value => new Date(value).toISOString()),
  ]),
  scope: z.enum(["full", "disk"]), reason: z.enum(["manual", "before-restore"]),
  kind: z.enum(["manual", "automatic", "before-keep", "fork-point", "warm", "step"]).optional(),
  sizeBytes: z.number().int().nonnegative().optional(),
})
const checkpointOperationShape = z.object({
  kind: z.enum(["capture", "fork", "restore", "delete"]), status: z.enum(["running", "failed"]),
  stage: z.string(), error: z.string().optional(),
})
const unfinishedRestoreShape = z.object({
  checkpointId: z.string().min(1), checkpointName: z.string().nullish(), phase: z.enum(["capturing", "secured"]),
})
const pendingCheckpointRestoreShape = z.object({
  checkpointId: z.string().min(1), sourceComputer: z.string().min(1), state: z.enum(["full", "disk"]),
})

/** An enum that maps a value from a newer Silo to a fallback instead of rejecting the whole state. */
function tolerantEnum<const T extends readonly [string, ...string[]]>(values: T, fallback: T[number]) {
  return z.string().transform((value): T[number] => (values as readonly string[]).includes(value) ? value : fallback)
}

/** A list that drops entries it cannot read instead of rejecting the state around it. */
function tolerantArray<T extends z.ZodType>(item: T) {
  return z.array(z.unknown()).transform(items => items.flatMap((entry): z.output<T>[] => {
    const parsed = item.safeParse(entry)
    return parsed.success ? [parsed.data] : []
  }))
}

const computerStates = ["running", "starting", "stopped", "failed"] as const
const repositoryShape = z.object({ path: z.string(), branch: z.string(), ahead: z.number().int().nonnegative(), behind: z.number().int().nonnegative(), dirty: z.boolean(), repository: z.string().nullable().optional(), head: z.string().nullable().optional() })
const fileEntryShape: z.ZodType<ApplicationFileEntry> = z.lazy(() => z.object({ name: z.string(), kind: z.enum(["folder", "file"]), children: z.array(fileEntryShape).optional() }))
const computerPortShape = z.object({
  port: z.number().int().min(1).max(65535), listening: z.boolean().nullable(),
  hostPort: z.number().int().min(1).max(65535).nullish(), scheme: z.enum(["http", "https"]).nullish(), configured: z.boolean().optional(),
})
const logShape = z.object({ line: z.string(), occurredAt: z.string() })
const attentionShape = z.object({ level: tolerantEnum(["warning", "error"], "warning"), message: z.string() })

// Fields from a newer Silo pass through untouched, so editing never drops them.
const configurationShape = z.object({
  id: z.string().min(1), name: z.string().min(1),
  cpus: z.number().int().positive(), maxCPUs: z.number().int().positive(), memoryGiB: z.number().int().positive(), maxMemoryGiB: z.number().int().positive(),
  workspaceStorageGiB: z.number().int().positive(), runtimeStorageGiB: z.number().int().positive(),
  desktop: z.object({ startWithComputer: z.boolean(), builtIn: z.boolean().optional() }).optional(),
  runtime: z.literal("container").optional().catch(undefined),
}).passthrough()

const computerShape = z.object({
  configuration: configurationShape,
  purpose: z.string(),
  // A state from a newer Silo is shown as the last reported detail, marked stale below.
  state: z.string(),
  stateDetail: z.string(),
  canDismissError: z.boolean().optional(),
  lifecycleFailure: z.string().optional(),
  lifecycleFailureDiagnostic: z.string().optional(),
  attention: attentionShape.nullish().transform(value => value ?? undefined),
  freshness: tolerantEnum(["fresh", "stale"], "stale"),
  repositories: tolerantArray(repositoryShape), files: tolerantArray(fileEntryShape), ports: tolerantArray(computerPortShape), logs: tolerantArray(logShape),
  githubRepositories: z.array(z.string()), secretNames: z.array(z.string()),
  pendingSecretRevocations: z.array(z.string()).optional(),
  checkpoints: tolerantArray(checkpointShape).optional(),
  checkpointOperation: checkpointOperationShape.nullable().optional().catch(null),
  pendingCheckpointRestore: pendingCheckpointRestoreShape.nullable().optional().catch(null),
  unfinishedRestore: unfinishedRestoreShape.nullable().optional().catch(null),
  quiet: quietStateSchema.nullable().optional().catch(null),
  settling: z.boolean().optional(),
}).passthrough().transform(computer => (computerStates as readonly string[]).includes(computer.state)
  ? { ...computer, state: computer.state as (typeof computerStates)[number] }
  : { ...computer, state: "stopped" as const, freshness: "stale" as const, attention: computer.attention ?? { level: "warning" as const, message: "This version of Silo cannot show this computer's current state. Update Silo to see it." } })

const computersShape = z.array(computerShape)

const activityShape = z.object({
  id: z.string().min(1),
  category: tolerantEnum(["computer", "git", "backup", "secrets", "github", "system"], "system"),
  title: z.string(), detail: z.string(), occurredAt: z.string(), time: z.string(),
  diagnostic: z.string().optional(), partial: z.boolean().optional(),
  tone: tolerantEnum(["success", "neutral", "warning", "danger"], "neutral"),
  status: tolerantEnum(["running", "completed"], "completed"),
  computer: z.string().nullish().transform(value => value ?? undefined),
  progress: z.number().optional(), progressLabel: z.string().optional(),
  cancelled: z.boolean().optional(),
})

const pushTargetShape = z.object({ repository: z.string().min(1), branch: z.string().min(1), commit: z.string().min(1) })
const pushFields = { operationId: z.string().optional(), computer: z.string().min(1), repositoryPath: z.string().min(1), commitCount: z.number().int().nonnegative(), target: pushTargetShape.optional() }
const pushOperationShape = z.discriminatedUnion("status", [
  z.object({ ...pushFields, status: z.literal("pushing"), message: z.string().optional() }),
  z.object({ ...pushFields, status: z.literal("unknown"), message: z.string() }),
  z.object({ ...pushFields, status: z.literal("succeeded") }),
  z.object({ ...pushFields, status: z.literal("failed"), message: z.string(), diagnosticDetails: z.string().optional() }),
])

const runtimeRepairShape = z.object({
  status: tolerantEnum(["needed", "unavailable"], "unavailable"), reason: z.string(), recovery: z.string().optional(), checking: z.boolean().optional(),
})

const computerConfigurationOperationFields = {
  id: z.string().min(1),
  candidate: setupComputerConfigurationRequestSchema,
  progressEvents: z.array(siloProgressEventSchema),
}
const computerConfigurationOperationShape = z.discriminatedUnion("status", [
  z.object({ ...computerConfigurationOperationFields, status: z.literal("applying"), result: z.null(), error: z.null() }),
  z.object({ ...computerConfigurationOperationFields, status: z.literal("awaiting-approval"), result: siloBootstrapResultSchema, error: z.null() }),
  z.object({ ...computerConfigurationOperationFields, status: z.literal("failed"), result: z.null(), error: siloProtocolErrorSchema }),
])

const preferencesShape = z.object({
  terminal: z.string(), editor: z.string(), browser: z.string(), launchAtLogin: z.boolean(),
  startComputersAtLaunch: z.boolean(), reduceMotion: z.boolean(),
  startupComputerIds: settingSchemas.startupComputerIds.optional().catch(undefined),
  terminalPath: settingSchemas.terminalPath.optional().catch(undefined),
  editorPath: settingSchemas.editorPath.optional().catch(undefined),
  browserPath: settingSchemas.browserPath.optional().catch(undefined),
  terminalUseSystemDefault: settingSchemas.terminalUseSystemDefault.optional().catch(undefined),
  editorUseSystemDefault: settingSchemas.editorUseSystemDefault.optional().catch(undefined),
  browserUseSystemDefault: settingSchemas.browserUseSystemDefault.optional().catch(undefined),
}).passthrough()
const backupSummaryShape = z.object({ lastArchive: z.string(), completedLabel: z.string(), compressedSize: z.string(), destination: z.string() })

// Every field the UI dereferences is validated here. Lists that only enrich the
// view (activities, pushes, repositories, logs) drop an entry they cannot read,
// and enums tolerate values from a newer Silo, so one unknown value never
// rejects a whole device's state or throws while it is being shown.
const applicationSourceShape = z.object({
  runtimeRepair: runtimeRepairShape.nullable(),
  deviceCapacity: z.object({ logicalCpus: z.number().int().positive(), physicalMemoryBytes: z.number().int().positive(), maxMemoryGib: z.number().int().positive() }).optional().catch(undefined),
  computers: computersShape,
  activities: tolerantArray(activityShape),
  // The runtime does not report an operation in progress; the source tracks its own.
  computerConfigurationOperation: computerConfigurationOperationShape.nullable().catch(null),
  repositoryPushOperations: tolerantArray(pushOperationShape),
  github: githubStateShape,
  secrets: z.array(secretShape),
  // Native runtime state no longer duplicates preference or backup stores (D-20).
  // Older owners and fixtures may still supply these legacy presentation fields.
  backup: backupSummaryShape.default({ lastArchive: "", completedLabel: "", compressedSize: "", destination: "" }),
  preferences: preferencesShape.default(() => ({ ...defaultSettings, startupComputerIds: undefined })),
}).passthrough()

// Another device's GitHub, secrets, export and preference state is never shown
// here, so a newer shape of those must not make its computers unavailable.
const remoteApplicationSourceShape = applicationSourceShape.extend({
  github: githubStateShape.catch({ state: "disconnected" as const, account: undefined }),
  secrets: tolerantArray(secretShape),
  backup: backupSummaryShape.catch({ lastArchive: "", completedLabel: "", compressedSize: "", destination: "" }),
  preferences: preferencesShape.catch({ terminal: "", editor: "", browser: "", launchAtLogin: false, startComputersAtLaunch: false, reduceMotion: false }),
})

const backupArchiveShape = z.object({
  name: z.string().min(1), archivePath: z.string().min(1), completedLabel: z.string(), size: z.string(), destination: z.string(), computers: z.array(z.string()),
  checkpointName: z.string().optional(), leftOut: z.array(z.string()).optional(),
}).strict()
const backupPhaseShape = z.object({ title: z.string(), detail: z.string(), tone: z.enum(["waiting", "running", "succeeded", "failed"]) }).strict()
const backupOperationShape = z.discriminatedUnion("kind", [
  z.object({ operation: z.enum(["backup", "restore"]), archive: backupArchiveShape, runningNames: z.array(z.string()), targetName: z.string().optional(), kind: z.literal("running"), progress: z.number().min(0).max(100), indeterminate: z.boolean().optional(), canCancel: z.boolean().optional(), phases: z.array(backupPhaseShape) }).strict(),
  z.object({ operation: z.enum(["backup", "restore"]), archive: backupArchiveShape, runningNames: z.array(z.string()), targetName: z.string().optional(), kind: z.literal("result"), outcome: z.enum(["success", "failed", "cancelled"]), title: z.string(), message: z.string(), detail: z.string().optional() }).strict(),
])
const backupStateShape = z.object({
  snapshotId: z.string(), operationId: z.string().optional(), availability: z.enum(["available", "unavailable"]), availabilityMessage: z.string().optional(),
  requiredSpaceGB: z.number().nonnegative().optional(), availableSpaceGB: z.number().nonnegative().optional(),
  unsupportedStorage: z.object({ computer: z.string(), label: z.string() }).strict().optional(),
  destination: z.string().optional(),
  archives: z.array(backupArchiveShape), operation: backupOperationShape.nullable(),
  resultUnseen: z.boolean().optional(),
}).strict()
export const archiveInspectionShape = z.object({ archive: backupArchiveShape, valid: z.boolean(), reason: z.string().optional(), warnings: z.array(z.string()).optional() }).strict()

export const exportLoginsShape = z.object({ logins: z.array(z.string()), checked: z.array(z.string()).optional(), unchecked: z.string().optional(), leavesOut: z.boolean().optional() }).strict()

const networkStateShape = z.object({ computers: z.array(z.object({
  computer: z.string(), error: z.string().nullable(), host: z.string().regex(/^[a-z0-9-]{1,63}\.localhost$/).nullable().optional(), ports: z.array(z.object({
    configuredHostPort: z.number().int().min(1).max(65535).nullable().optional(),
    port: z.number().int().min(1).max(65535), hostPort: z.number().int().min(1).max(65535).nullable(),
    scheme: z.enum(["http", "https"]).nullable(), state: z.enum(["reachable", "waiting", "unpublished", "unknown"]),
    configured: z.boolean(), message: z.string().nullable().optional(),
  })),
})) })

export function parseSshAccessState(input: unknown): SshAccessState {
  return sshAccessShape.parse(input)
}

export function parseNetworkState(input: unknown): NetworkState {
  return networkStateShape.parse(input)
}

export function parseApplicationSource(input: unknown): ApplicationSource {
  return applicationSourceShape.parse(input)
}

/** Another device's snapshot, which may come from an older or newer Silo. */
export function parseRemoteApplicationSource(input: unknown): ApplicationSource {
  return remoteApplicationSourceShape.parse(input)
}

export function parseBackupState(input: unknown): BackupState {
  return backupStateShape.parse(input) as BackupState
}

// Review requests (`changes.requests.*`), steps, the computer tree and credentials used.
// Tolerant like checkpoints: unknown statuses and origins fall back, bad rows are dropped.
const requestStatuses = ["open", "keeping", "kept", "thrown-away"] as const
const requestClosedReasons = ["fork-deleted", "target-deleted"] as const
const countsShape = z.record(z.string(), z.number().int().nonnegative()).catch({})

export const foldRequestShape = z.object({
  id: z.string().min(1), forkId: z.string(), target: z.string(), createdAt: z.string(),
  reviewDigest: z.string().catch(""), summary: z.string().catch(""),
  status: tolerantEnum(requestStatuses, "open"), counts: countsShape,
  keepId: z.string().nullish().transform(value => value ?? undefined),
  resolvedAt: z.string().nullish().transform(value => value ?? undefined),
  keepingSinceMs: z.number().nonnegative().nullish().transform(value => value ?? undefined),
  closedReason: z.string().nullish().transform(value => (requestClosedReasons as readonly string[]).includes(value ?? "") ? (value as (typeof requestClosedReasons)[number]) : undefined),
  /** The approval that asks the owner to merge this request, when a computer's agent asked. */
  approvalId: z.string().nullish().transform(value => value ?? undefined),
})
export type FoldRequest = z.infer<typeof foldRequestShape>

const optionalText = z.string().nullish().transform(value => value ?? undefined)
export const requestEntryShape = z.object({
  request: foldRequestShape, forkName: optionalText, forkState: optionalText, targetName: optionalText,
  deviceId: optionalText, deviceName: optionalText,
})
export type RequestEntry = z.infer<typeof requestEntryShape>

export const requestListSchema = z.object({
  requests: tolerantArray(requestEntryShape),
  unreachable: tolerantArray(z.object({ deviceId: z.string(), deviceName: z.string(), reason: z.string() })).catch([]),
})
export type RequestList = z.infer<typeof requestListSchema>

const credentialUseShape = z.object({ secret: z.string(), host: z.string(), method: z.string(), count: z.number().int().nonnegative() })
const credentialBlockShape = z.object({ host: z.string(), method: z.string(), count: z.number().int().nonnegative(), reason: z.string() })
export const credentialUsageSchema = z.object({
  used: tolerantArray(credentialUseShape).catch([]), blocked: tolerantArray(credentialBlockShape).catch([]),
})
export type CredentialUsage = z.infer<typeof credentialUsageSchema>

export const requestDetailSchema = z.object({
  request: foldRequestShape, forkName: z.string(), forkState: z.string(),
  stale: z.boolean().nullish().transform(value => value ?? undefined),
  reviewDigest: z.string(), fresh: z.boolean(),
  /** The `silo diff --json` document; null for a folder fork. */
  diff: z.unknown(), text: z.string().catch(""), counts: countsShape,
  credentials: credentialUsageSchema.catch({ used: [], blocked: [] }),
})
export type RequestDetail = z.infer<typeof requestDetailSchema>

/** One item both sides changed. `allowed` is empty when no answer settles it (credentials). */
export const keepConflictShape = z.object({
  kind: z.string().catch(""), item: z.string().catch(""), detail: z.string().catch(""),
  id: z.string().catch(""), allowed: z.array(z.string()).catch([]),
  forkChange: z.string().catch(""), mainChange: z.string().catch(""),
})
export type KeepConflict = z.infer<typeof keepConflictShape>

export const keepRefusalShape = z.object({
  reason: z.string(), message: z.string(), hint: z.string().catch(""), conflicts: tolerantArray(keepConflictShape).catch([]),
})
export const keepRequestResultSchema = z.object({
  status: z.string(),
  report: z.object({
    fork: z.string(), target: z.string(), summary: z.array(z.string()).catch([]), steps: z.array(z.string()).catch([]),
    notes: z.array(z.string()).catch([]), checkpoint: optionalText, reviewDigest: optionalText,
    /** True when main had not changed since the fork, so nothing could conflict. */
    fastForward: z.boolean().nullish().transform(value => value ?? undefined),
  }),
  refusal: keepRefusalShape.nullish().transform(value => value ?? undefined),
  request: foldRequestShape,
})
export type KeepRequestResult = z.infer<typeof keepRequestResultSchema>

export const throwAwayResultSchema = z.object({ request: foldRequestShape, dropped: z.string().catch("") })
export type ThrowAwayResult = z.infer<typeof throwAwayResultSchema>

export const openRequestResultSchema = z.object({
  request: foldRequestShape, computerId: z.string(), name: z.string(), state: z.string(), started: z.boolean(),
  sshAlias: optionalText, urls: z.array(z.string()).catch([]),
})
export type OpenRequestResult = z.infer<typeof openRequestResultSchema>

export const stepShape = z.object({
  id: z.string(), computerId: z.string(), number: z.number().int().nonnegative(), command: z.string(),
  exitCode: z.number().int().nullish().transform(value => value ?? undefined),
  startedAt: z.string(), finishedAt: z.string(),
  checkpointId: optionalText,
  parentStep: z.number().int().nullish().transform(value => value ?? undefined),
  parentCheckpoint: optionalText,
})
export type ComputerStep = z.infer<typeof stepShape>
export const stepListSchema = z.object({ steps: tolerantArray(stepShape) })

/** What a fork was made from; an origin from a newer Silo reads as `root`. */
export const treeFromShape = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("root") }),
  z.object({ kind: z.literal("main") }),
  z.object({ kind: z.literal("checkpoint"), checkpointId: z.string() }),
  z.object({ kind: z.literal("step"), number: z.number().int().nonnegative(), checkpointId: z.string() }),
  z.object({ kind: z.literal("recipe"), hash: z.string() }),
  z.object({ kind: z.literal("folder"), path: z.string() }),
]).catch({ kind: "root" })
export type TreeFrom = z.infer<typeof treeFromShape>

export const treeNodeShape = z.object({
  id: z.string().min(1), name: z.string(), parent: z.string().nullish().transform(value => value ?? null),
  from: treeFromShape, state: z.string(),
  pendingChanges: z.object({
    requestId: z.string().nullish().transform(value => value ?? null),
    buckets: countsShape.nullish().transform(value => value ?? null),
  }).catch({ requestId: null, buckets: null }),
})
export type ComputerTreeNode = z.infer<typeof treeNodeShape>
export const computerTreeSchema = z.object({ nodes: tolerantArray(treeNodeShape) })

// Approvals (`approvals.*`), the audit log (`audit.list`) and `device.resources`.
// Unknown kinds, states and callers from a newer engine read as the safest known value.
const approvalStates = ["pending", "allowed", "denied", "cancelled", "expired"] as const

export const approvalOutcomeShape = z.object({
  result: z.unknown().optional(), jobId: optionalText, error: optionalText,
})
export type ApprovalOutcome = z.infer<typeof approvalOutcomeShape>

export const approvalShape = z.object({
  id: z.string().min(1), kind: z.string().catch("other"), computerId: z.string().catch(""), computerName: z.string().catch(""),
  method: z.string().catch(""), params: z.unknown().optional(), summary: z.string().catch(""),
  /** The agent's own words; never the engine's. */
  note: optionalText,
  state: tolerantEnum(approvalStates, "cancelled"),
  createdAt: z.string(), expiresAt: optionalText, decidedAt: optionalText, decidedBy: optionalText,
  decisionNote: optionalText, reason: optionalText,
  outcome: approvalOutcomeShape.nullish().transform(value => value ?? undefined),
  reviewRequestId: optionalText, reviewDigest: optionalText,
})
export type Approval = z.infer<typeof approvalShape>
export type ApprovalState = Approval["state"]

export const approvalListSchema = z.object({ approvals: tolerantArray(approvalShape) })
export type ApprovalList = z.infer<typeof approvalListSchema>

const callerKinds = ["owner", "device", "computer"] as const
const auditOutcomes = ["ok", "error", "refused", "asked"] as const
export type AuditCallerKind = (typeof callerKinds)[number]

export const auditEntryShape = z.object({
  id: z.number().int().nonnegative(), at: z.string(),
  callerKind: tolerantEnum(callerKinds, "computer"), callerId: optionalText, client: optionalText,
  method: z.string(), computerId: optionalText,
  outcome: tolerantEnum(auditOutcomes, "error"), errorCode: optionalText, jobId: optionalText, approvalId: optionalText,
  count: z.number().int().positive().catch(1), lastAt: optionalText,
})
export type AuditEntry = z.infer<typeof auditEntryShape>

export const auditListSchema = z.object({
  entries: tolerantArray(auditEntryShape),
  nextBeforeId: z.number().int().nonnegative().nullish().transform(value => value ?? undefined),
})
export type AuditPage = z.infer<typeof auditListSchema>

export const deviceResourcesSchema = z.object({
  logicalCpus: z.number().catch(0), memoryTotalBytes: z.number().catch(0),
  memoryAvailableBytes: z.number().nullish().transform(value => value ?? undefined),
  diskFreeBytes: z.number().catch(0), diskTotalBytes: z.number().catch(0),
  runningComputers: z.number().catch(0), runningMemoryGib: z.number().catch(0),
  state: tolerantEnum(["ok", "low", "critical"], "ok"), message: z.string().catch(""),
  computers: tolerantArray(z.object({ id: z.string(), name: z.string(), running: z.boolean(), cpus: z.number(), memoryGib: z.number() })).catch([]),
})
export type DeviceResources = z.infer<typeof deviceResourcesSchema>

/** How a job an allowed approval started is going. */
export const approvalJobSchema = z.object({
  state: tolerantEnum(["waiting", "running", "finished"], "running"),
  error: optionalText, status: optionalText, message: optionalText,
})
export type ApprovalJob = z.infer<typeof approvalJobSchema>

// Agents (`agents.list`, `agents.install`) and the SSH name of a computer (`guest.sshAlias.ensure`).
export const agentIds = ["claude-code", "codex"] as const
export type AgentId = (typeof agentIds)[number]
const agentStates = ["ready", "missing", "outdated", "installing", "unknown"] as const
export type AgentState = (typeof agentStates)[number]

/** An agent this Silo does not know is dropped, so a newer engine never shows a row nothing can act on. */
const knownAgent = z.object({
  id: z.enum(agentIds), name: z.string().catch(""),
  state: tolerantEnum(agentStates, "unknown"), version: optionalText, expectedVersion: z.string().catch(""),
})
export type AgentInfo = z.infer<typeof knownAgent>

export const agentsListSchema = z.object({ agents: tolerantArray(knownAgent) })

export const agentInstallStepSchema = tolerantEnum(["waiting", "downloading", "installing", "registering", "done"], "installing")
export type AgentInstallStep = z.infer<typeof agentInstallStepSchema>
export const agentInstallProgressSchema = z.object({
  computerId: z.string(), agent: z.enum(agentIds), step: agentInstallStepSchema, detail: optionalText,
})
export type AgentInstallProgress = z.infer<typeof agentInstallProgressSchema>

export const sshNameSchema = z.object({
  alias: z.string().min(1), includeInstalled: z.boolean().catch(true), includeLine: z.string().catch(""),
})
export type SshName = z.infer<typeof sshNameSchema>
