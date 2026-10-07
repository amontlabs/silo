import { defaultSettings, settingSchemas } from "@/features/preferences/model/settings"
import { z } from "zod"

import { siloBootstrapResultSchema, siloProgressEventSchema, siloProtocolErrorSchema, setupComputerConfigurationRequestSchema } from "@/contracts/silo"
import type { SshAccessState, NetworkState, ApplicationFileEntry, ApplicationSource } from "@/features/application/model/application-source"
import type { BackupState } from "@/features/application/model/backup-source"

const sshAccessShape = z.object({ computers: z.array(z.object({
  computer: z.string(), user: z.string().optional(), enabled: z.boolean(), port: z.number().int(), bindAddress: z.string(), keys: z.array(z.string()),
  state: z.enum(["disabled", "waiting", "listening", "error"]), message: z.string().nullable(), fingerprint: z.string().nullable(), deviceName: z.string(), addresses: z.array(z.string()),
})) })

export const githubStateShape = z.object({
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
  entries: z.array(z.object({ name: z.string().min(1), path: z.string().startsWith("/workspace/"), kind: z.enum(["folder", "file", "symlink"]) }).strict()).max(200),
  nextOffset: z.number().int().nonnegative().nullable(),
}).strict()

export const secretShape = z.object({
  id: z.string(), name: z.string(), computers: z.array(z.string()), allowedDomains: z.array(z.string()),
  state: z.enum(["active", "applying", "restart-required"]), pendingComputers: z.array(z.string()).optional(),
  error: z.string().nullish().transform((value) => value ?? undefined), removing: z.boolean().optional(),
})

const checkpointShape = z.object({
  id: z.string().min(1), name: z.string().min(1),
  // The Rust checkpoint journal stores Unix milliseconds (the same u64
  // contract as activity timestamps); normalize at the native boundary for
  // the UI's ISO timestamp model. String values remain accepted for remotes.
  createdAt: z.union([
    z.string().datetime(),
    z.number().int().nonnegative().max(8.64e15).transform(value => new Date(value).toISOString()),
  ]),
  scope: z.enum(["full", "disk"]), reason: z.enum(["manual", "before-restore"]),
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
  checkpointName: z.string().optional(),
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
export const archiveInspectionShape = z.object({ archive: backupArchiveShape, valid: z.boolean(), reason: z.string().optional() }).strict()

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
