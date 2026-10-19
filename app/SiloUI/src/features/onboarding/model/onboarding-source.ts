import { z } from "zod"

import type { ApplicationGitHubRepositoryPolicy, ApplicationGitHubComputerPolicy, ApplicationComputerGitIdentity } from "@/features/application/model/application-source"

import {
  githubComputerPolicySchema,
  siloBootstrapConfigurationSchema,
  siloBootstrapResultSchema,
  siloBootstrapStateSchema,
  siloPreflightCheckSchema,
  siloProgressEventSchema,
  siloProtocolErrorSchema,
  setupComputerConfigurationRequestSchema,
  setupQueueItemIdSchema,
} from "@/contracts/silo"
import type { SetupComputerConfigurationRequest } from "@/contracts/silo"
import { applicationPreferenceSelectionSchema, type ApplicationPreferenceSelection } from "@/features/preferences/model/application-preferences"

// This is the frontend's narrow input seam, not a Silo wire object. Each field
// remains an unmodified current protocol or app-state shape so a future bridge
// only has to replace the provider.
export const onboardingSourceSchema = z.object({
  readyToFinish: z.boolean().optional(),
  /**
   * Why Finish is unavailable after computers were set up (one failed, is starting, or
   * its status is unconfirmed), with the action that resolves it, if any.
   */
  finishBlocker: z.object({
    message: z.string(),
    computer: z.string(),
    action: z.enum(["start", "refresh"]).nullable(),
  }).strict().nullable().optional(),
  /**
   * False while `computerConfigurations` is a fallback (saved list or defaults) because
   * this device's computers have not loaded; the draft is seeded again once they do.
   */
  configurationsAuthoritative: z.boolean().optional(),
  /** Computers that already exist on this device; absent while the device's computers have not loaded. Dropping one from the draft deletes it. */
  existingConfigurations: setupComputerConfigurationRequestSchema.shape.computers.optional(),
  /** Every computer id on this device, including any whose saved configuration could not be read. Absent while the list is not known. */
  existingConfigurationIds: z.array(z.string()).optional(),
  setupQueue: z.array(z.object({ id: setupQueueItemIdSchema, status: z.enum(["idle", "queued", "running", "succeeded", "failed"]), failure: z.string().optional() }).strict()).optional(),
  computerConfigurations: setupComputerConfigurationRequestSchema.shape.computers,
  bootstrapConfiguration: siloBootstrapConfigurationSchema,
  bootstrapState: siloBootstrapStateSchema,
  preflightChecks: z.array(siloPreflightCheckSchema),
  progressEvents: z.array(siloProgressEventSchema),
  activityEvents: z.array(siloProgressEventSchema).optional(),
  activityError: z.string().optional(),
  githubPolicies: z.array(githubComputerPolicySchema),
  currentDeviceGitIdentity: z.object({
    name: z.string(),
    email: z.string(),
  }).strict().nullable(),
  applicationPreferences: applicationPreferenceSelectionSchema,
  bootstrapResult: siloBootstrapResultSchema.nullable(),
  error: siloProtocolErrorSchema.nullable(),
}).strict().superRefine((source, context) => {
  const result = setupComputerConfigurationRequestSchema.safeParse({
    schemaVersion: 1,
    computers: source.computerConfigurations,
  })
  if (!result.success) {
    for (const issue of result.error.issues) {
      context.addIssue({ ...issue, path: ["computerConfigurations", ...issue.path.slice(1)] })
    }
  }
})

export type OnboardingSource = z.infer<typeof onboardingSourceSchema>

export type GitHubConnectionState = "disconnected" | "connecting" | "connected"

export type ComputerRepositorySelection = ApplicationGitHubRepositoryPolicy

export type ComputerGitIdentity = ApplicationComputerGitIdentity

export interface OnboardingCompletionRequest {
  computerConfiguration: SetupComputerConfigurationRequest
  applications: ApplicationPreferenceSelection
  github: {
    connectionState: GitHubConnectionState
    computers: ApplicationGitHubComputerPolicy[]
  }
}

/**
 * Existing computers (configuration ids) the user explicitly confirmed deleting in this
 * session. A submission never deletes an existing computer that is not listed here.
 */
export interface OnboardingSubmissionOptions {
  confirmedDeletions: readonly string[]
}

export interface OnboardingActions {
  submitStep?: (step: "computers" | "github", request: OnboardingCompletionRequest, options?: OnboardingSubmissionOptions) => void
  cancelGitHubConnection?: () => void
  reopenGitHubAuthorization?: () => void
  connectGitHub: () => void
  saveComputerConfiguration: (request: SetupComputerConfigurationRequest, options?: OnboardingSubmissionOptions) => void
  /** Retry the last submission, rebuilt from `request` (the current draft) when given. */
  retryComputerSetup: (request?: OnboardingCompletionRequest, options?: OnboardingSubmissionOptions) => void
  finishSetup: (request: OnboardingCompletionRequest, options?: OnboardingSubmissionOptions) => void
  /** Resolves a `finishBlocker` whose action is "start". */
  startComputer?: (computer: string) => void
  /** Resolves a `finishBlocker` whose action is "refresh". */
  refreshSetupState?: () => void
}

export function parseOnboardingSource(input: unknown): OnboardingSource {
  return onboardingSourceSchema.parse(input)
}
