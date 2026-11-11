import { z } from "zod"

const preflightStatusSchema = z.enum(["pending", "pass", "failed", "needsAction", "unavailable", "timeout"])

export const siloPreflightCheckSchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  status: preflightStatusSchema,
  detail: z.string(),
  remediation: z.string().nullable(),
}).strict()

const siloBootstrapComputerSchema = z.object({
  name: z.string().regex(/^[a-z][a-z0-9-]{0,31}$/),
  cpu: z.number().int().min(1).max(255),
  cpuCeiling: z.number().int().min(1).max(255),
  memoryGiB: z.number().int().min(1).max(4_294_967_295),
  memoryCeilingGiB: z.number().int().min(1).max(4_294_967_295),
  workspaceStorageGiB: z.number().int().min(1).max(4_194_303),
  runtimeStorageGiB: z.number().int().min(1).max(4_194_303),
}).strict().refine((computer) => computer.workspaceStorageGiB + computer.runtimeStorageGiB <= 4_194_303, { message: "The two disks together exceed 4,194,303 GiB. Reduce a disk size.", path: ["workspaceStorageGiB"] }).refine((computer) => computer.cpu <= computer.cpuCeiling, {
  message: "cpu must not exceed cpuCeiling",
}).refine((computer) => computer.memoryGiB <= computer.memoryCeilingGiB, {
  message: "memoryGiB must not exceed memoryCeilingGiB",
})

export const siloBootstrapConfigurationSchema = z.object({
  schemaVersion: z.literal(1),
  computers: z.array(siloBootstrapComputerSchema).min(1).max(64),
}).strict().refine((configuration) => {
  const names = configuration.computers.map(({ name }) => name)
  return new Set(names).size === names.length
}, { message: "Computer names must be unique." })

const desktopConfigurationSchema = z.object({
  startWithComputer: z.boolean(),
  // Reported for computers whose desktop is built into the image (v4). Read-only; whether it starts with the computer is `startWithComputer`.
  builtIn: z.boolean().optional(),
}).strict()

export const setupComputerConfigurationSchema = z.object({
  id: z.uuid(),
  name: z.string().regex(/^[a-z][a-z0-9-]{0,31}$/),
  cpus: z.number().int().min(1).max(255),
  maxCPUs: z.number().int().min(1).max(255),
  memoryGiB: z.number().int().min(1).max(4_294_967_295),
  maxMemoryGiB: z.number().int().min(1).max(4_294_967_295),
  workspaceStorageGiB: z.number().int().min(1).max(4_194_303),
  runtimeStorageGiB: z.number().int().min(1).max(4_194_303),
  desktop: desktopConfigurationSchema.optional(),
  runtime: z.literal("container").optional(),
}).strict().refine((computer) => computer.workspaceStorageGiB + computer.runtimeStorageGiB <= 4_194_303, { message: "The two disks together exceed 4,194,303 GiB. Reduce a disk size.", path: ["workspaceStorageGiB"] }).refine((computer) => computer.cpus <= computer.maxCPUs, {
  message: "cpus must not exceed maxCPUs",
}).refine((computer) => computer.memoryGiB <= computer.maxMemoryGiB, {
  message: "memoryGiB must not exceed maxMemoryGiB",
})

export const setupComputerConfigurationRequestSchema = z.object({
  schemaVersion: z.literal(1),
  computers: z.array(setupComputerConfigurationSchema).max(64),
}).strict().refine((configuration) => {
  const names = configuration.computers.map(({ name }) => name.toLowerCase())
  return new Set(names).size === names.length
}, { message: "Computer names must be unique." }).refine((configuration) => {
  const ids = configuration.computers.map(({ id }) => id)
  return new Set(ids).size === ids.length
}, { message: "Computer IDs must be unique." })

const siloBootstrapPhaseSchema = z.enum([
  "welcome",
  "preflight",
  "toolchain",
  "deviceIntegration",
  "computers",
  "github",
  "identity",
  "complete",
])

export const siloBootstrapStateSchema = z.object({
  phase: siloBootstrapPhaseSchema,
  startedAt: z.number().optional(),
  updatedAt: z.number(),
  lastError: z.string().optional(),
  completedPhases: z.array(siloBootstrapPhaseSchema),
  computerConfigurations: z.array(setupComputerConfigurationSchema).optional(),
  phaseDurations: z.record(z.string(), z.number().nonnegative()),
}).strict()

export const siloProgressEventSchema = z.object({
  schemaVersion: z.literal(1),
  type: z.literal("progress"),
  requestId: z.string().trim().min(1),
  phase: z.string().trim().min(1),
  step: z.string().optional(),
  computer: z.string().optional(),
  revision: z.string().regex(/^[0-9a-f]{64}$/).optional(),
  fraction: z.number().min(0).max(1).optional(),
  message: z.string(),
  safeForDisplay: z.boolean(),
  timestamp: z.number().int().nonnegative().optional(),
  level: z.enum(["info", "warning", "error"]).optional(),
  elapsedSeconds: z.number().nonnegative().optional(),
  downloadedBytes: z.number().int().nonnegative().optional(),
  totalBytes: z.number().int().nonnegative().optional(),
  failureCode: z.enum(["auth", "access", "disk", "permission", "network", "timeout", "integrity", "resources", "configuration", "unavailable", "runtime"]).optional(),
  exitCode: z.number().int().optional(),
  /** `setup-failed` only: the runtime's own explanation for a Details disclosure; never part of `message`. */
  diagnostic: z.string().optional(),
  /** Some setup changes completed before this failure. */
  partial: z.boolean().optional(),
}).strict()

export const siloBootstrapResultSchema = z.object({
  resumed: z.boolean(),
  phase: z.string(),
  requiresApproval: z.boolean(),
  vmsStarted: z.boolean(),
  message: z.string(),
}).strict()

export const siloProtocolErrorSchema = z.object({
  code: z.string().min(1),
  message: z.string().min(1),
  recovery: z.string().nullable(),
  computer: z.string().nullable(),
  retryable: z.boolean(),
}).strict()

const githubRepositoryPolicySchema = z.object({
  computer: z.string().min(1),
  repositoryID: z.number().int(),
  fullName: z.string().min(1),
  ownerID: z.number().int(),
  ownerLogin: z.string().min(1),
  ownerType: z.string().nullable(),
  mode: z.enum(["read-only", "read-write"]),
}).strict()

export const githubComputerPolicySchema = z.object({
  computer: z.string().min(1),
  repositories: z.array(githubRepositoryPolicySchema),
}).strict().refine((policy) => (
  policy.repositories.every((repository) => repository.computer === policy.computer)
), { message: "Repository computers must match the computer access policy." })

export const setupQueueItemIdSchema = z.enum([
  "computerRun",
  "computerVerify",
  "githubRun",
  "githubVerify",
  "identityRun",
  "identityVerify",
  /** Installing the chosen agent in the first computer. Its failure never fails setup. */
  "agentInstall",
  "completion",
])

export type SiloPreflightCheck = z.infer<typeof siloPreflightCheckSchema>
export type SiloBootstrapConfiguration = z.infer<typeof siloBootstrapConfigurationSchema>
export type SetupComputerConfiguration = z.infer<typeof setupComputerConfigurationSchema>
export type SetupComputerConfigurationRequest = z.infer<typeof setupComputerConfigurationRequestSchema>
export type SiloProgressEvent = z.infer<typeof siloProgressEventSchema>
export type SiloBootstrapResult = z.infer<typeof siloBootstrapResultSchema>
export type SiloProtocolError = z.infer<typeof siloProtocolErrorSchema>
export type SetupQueueItemID = z.infer<typeof setupQueueItemIdSchema>
