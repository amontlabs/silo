import { z } from "zod"

import {
  setupComputerConfigurationRequestSchema,
  setupComputerConfigurationSchema,
} from "@/contracts/silo"
import { onboardingSteps } from "@/features/onboarding/model/onboarding-state"

// Recovery stores unfinished input. Existing Save validation still decides
// whether these values can become a computer configuration.
const unfinishedComputerSchema = z.object({
  ...setupComputerConfigurationSchema.shape,
  name: z.string(),
  cpus: z.number(),
  maxCPUs: z.number(),
  workspaceStorageGiB: z.number(),
  runtimeStorageGiB: z.number(),
  memoryGiB: z.number(),
  maxMemoryGiB: z.number(),
}).strict()

export const computerEditorDraftSchema = z.object({
  draft: unfinishedComputerSchema,
  originalID: z.uuid().optional(),
  insertAt: z.number().int().nonnegative(),
  displayAfterID: z.uuid().optional(),
}).strict()

export type ComputerEditorDraft = z.infer<typeof computerEditorDraftSchema>

export const onboardingDraftSchema = z.object({
  currentStep: z.enum(onboardingSteps),
  computers: setupComputerConfigurationRequestSchema.shape.computers,
  unfinishedComputerEditor: computerEditorDraftSchema.nullable(),
  computerSelections: z.record(z.string(), z.array(z.object({
    repository: z.string(),
    allowPushes: z.boolean(),
  }).strict())),
  computerRepositoryAccess: z.record(z.string(), z.object({ authenticationMethod: z.enum(["oauth", "token"]).optional(), tokenSecretId: z.string().min(1).max(64).optional(), repositoryMode: z.enum(["selected", "all"]), allRepositoriesAllowChanges: z.boolean() }).strict()).optional(),
  computerIdentities: z.record(z.string(), z.object({
    name: z.string(), email: z.string(), apply: z.boolean(),
  }).strict()),
}).strict().superRefine((draft, context) => {
  const result = setupComputerConfigurationRequestSchema.safeParse({ schemaVersion: 1, computers: draft.computers })
  if (!result.success) {
    for (const issue of result.error.issues) context.addIssue({ ...issue })
  }
})

export type OnboardingDraft = z.infer<typeof onboardingDraftSchema>
