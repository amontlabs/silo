import { configurationFailureDiagnostic } from "../model/configuration-failure"
import { setupComputerConfigurationSchema, type SetupComputerConfiguration, type SiloProgressEvent } from "@/contracts/silo"
import type { ApplicationComputer, ApplicationSource, ComputerConfigurationOperation } from "@/features/application/model/application-source"
import type { ComputerIconState } from "@/features/computers/components/computer-list"

export interface ConfigurationRowView {
  status: "running" | "failed"
  message: string
  diagnostic?: string
  completedSteps?: number
  recovery?: string
  retryable: boolean
}

const configurationSteps = new Set([
  "computer-configuration",
  "computer-networking",
  "computer-verification",
])

function emptyComputer(configuration: SetupComputerConfiguration): ApplicationComputer {
  return {
    configuration,
    purpose: "New computer",
    state: "stopped",
    stateDetail: "Not configured",
    freshness: "fresh",
    repositories: [],
    files: [],
    ports: [],
    logs: [],
    githubRepositories: [],
    secretNames: [],
  }
}

export function displayComputers(source: ApplicationSource): ApplicationComputer[] {
  const operation = source.computerConfigurationOperation
  if (!operation) return source.computers
  const committedIDs = new Set(source.computers.map(({ configuration }) => configuration.id))
  const candidatesByID = new Map(operation.candidate.computers.map((configuration) => [configuration.id, configuration]))
  return [
    ...source.computers.map((computer) => ({
      ...computer,
      configuration: candidatesByID.get(computer.configuration.id) ?? computer.configuration,
    })),
    ...operation.candidate.computers
      .filter(({ id }) => !committedIDs.has(id))
      .map(emptyComputer),
  ]
}

function latestSafeEvent(operation: ComputerConfigurationOperation, computer: string): SiloProgressEvent | undefined {
  const activeRevision = operation.progressEvents.findLast(({ revision }) => revision)?.revision
  return operation.progressEvents.findLast((event) => (
    event.safeForDisplay
    && event.computer === computer
    && (!activeRevision || !event.revision || event.revision === activeRevision)
  ))
}

export function configurationRowView(
  computer: ApplicationComputer,
  committedComputer: ApplicationComputer | undefined,
  operation: ComputerConfigurationOperation,
): ConfigurationRowView | undefined {
  const candidate = operation.candidate.computers.find(({ id }) => id === computer.configuration.id)
  const candidateName = candidate?.name ?? computer.configuration.name
  const removed = Boolean(committedComputer && !candidate)
  const addedOrChanged = !committedComputer || JSON.stringify(setupComputerConfigurationSchema.parse(committedComputer.configuration)) !== JSON.stringify(candidate && setupComputerConfigurationSchema.parse(candidate))
  const errorTargetsComputer = operation.status === "failed"
    && (operation.error.computer === candidateName || (!operation.error.computer && (removed || addedOrChanged)))

  if (errorTargetsComputer) {
    return {
      status: "failed",
      message: operation.error.message,
      diagnostic: configurationFailureDiagnostic(operation, candidateName),
      recovery: operation.error.recovery ?? undefined,
      retryable: operation.error.retryable,
    }
  }
  if (operation.status === "failed") return undefined
  if (removed) {
    return {
      status: "running",
      message: "Deleting the computer’s files and checkpoints.",
      retryable: false,
    }
  }

  const latest = latestSafeEvent(operation, candidateName)
  if (!latest && !addedOrChanged) return undefined
  if (!latest) {
    return {
      status: "running",
      message: "Preparing computer configuration.",
      completedSteps: 0,
      retryable: false,
    }
  }

  const completedSteps = new Set(operation.progressEvents
    .filter((event) => event.computer === candidateName && event.step && event.fraction === 1 && configurationSteps.has(event.step))
    .map(({ step }) => step)).size
  return {
    status: "running",
    message: latest.message,
    completedSteps,
    retryable: false,
  }
}

export const attentionPriority: Record<ComputerIconState, number> = {
  error: 0,
  warning: 1,
  normal: 2,
}
