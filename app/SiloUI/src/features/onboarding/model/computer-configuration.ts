import type { SetupComputerConfiguration } from "@/contracts/silo"
import {
  setupComputerConfigurationSchema,
  setupComputerConfigurationRequestSchema,
} from "@/contracts/silo"

export const supportedCPUs = [1, 2, 4, 6, 8, 12, 16] as const
export const supportedMemoryGiB = [1, 2, 4, 8, 12, 16, 24, 32, 48, 64] as const
export const supportedStorageGiB = [10, 20, 40, 60, 80, 100, 120, 200] as const
export const maximumComputerCount = 64

/** Adding uses another slot; editing an existing computer keeps its slot. */
export function computerCapacityError(configurationCount: number, originalID?: string): string | undefined {
  if (!originalID && configurationCount >= maximumComputerCount) {
    return `Configure no more than ${maximumComputerCount} computers.`
  }
}

// Fresh onboarding offers one dev computer; creation waits for Continue.
export const productionComputerDefaults: readonly SetupComputerConfiguration[] = [
  {
    id: "00000000-0000-4000-8000-000000000001",
    name: "dev",
    cpus: 8,
    maxCPUs: 12,
    memoryGiB: 32,
    maxMemoryGiB: 48,
    workspaceStorageGiB: 120,
    runtimeStorageGiB: 100,
  },
] as const

export function createComputerID(): string {
  return crypto.randomUUID()
}

export function nextComputerName(base: string, configurations: readonly SetupComputerConfiguration[]): string {
  const names = new Set(configurations.map(({ name }) => name.toLowerCase()))
  let suffix = "-copy"
  let candidate = `${base.slice(0, 32 - suffix.length).replace(/-+$/, "")}${suffix}`
  let index = 2
  while (names.has(candidate.toLowerCase())) {
    suffix = `-copy-${index}`
    candidate = `${base.slice(0, 32 - suffix.length).replace(/-+$/, "")}${suffix}`
    index += 1
  }
  return candidate
}

export function newVirtualComputer(configurations: readonly SetupComputerConfiguration[]): SetupComputerConfiguration {
  const names = new Set(configurations.map(({ name }) => name.toLowerCase()))
  let number = configurations.length + 1
  while (names.has(`computer-${number}`)) number += 1
  const template = productionComputerDefaults[0]
  return { ...template, id: createComputerID(), name: `computer-${number}` }
}

export function duplicateComputer(
  configuration: SetupComputerConfiguration,
  configurations: readonly SetupComputerConfiguration[],
): SetupComputerConfiguration {
  return {
    ...configuration,
    id: createComputerID(),
    name: nextComputerName(configuration.name, configurations),
  }
}

export type ComputerValidationErrors = Partial<Record<"form" | "name" | "cpus" | "maxCPUs" | "memoryGiB" | "maxMemoryGiB" | "workspaceStorageGiB" | "runtimeStorageGiB", string>>

function isComputerValidationField(field: unknown): field is Exclude<keyof ComputerValidationErrors, "form"> {
  return field === "name" || field === "cpus" || field === "maxCPUs"
    || field === "memoryGiB" || field === "maxMemoryGiB"
    || field === "workspaceStorageGiB" || field === "runtimeStorageGiB"
}

export function validateComputerName(name: string): string | undefined {
  if (!/^[a-z][a-z0-9-]{0,31}$/.test(name)) {
    return "Use 1–32 lowercase letters, numbers, or hyphens, starting with a letter."
  }
}

export function validateComputer(
  configuration: SetupComputerConfiguration,
  configurations: readonly SetupComputerConfiguration[],
  originalID?: string,
): ComputerValidationErrors {
  const result = setupComputerConfigurationSchema.safeParse(configuration)
  const errors: ComputerValidationErrors = {}
  if (!result.success) {
    for (const issue of result.error.issues) {
      const field = isComputerValidationField(issue.path[0]) ? issue.path[0] : "form"
      errors[field] ??= issue.message
    }
  }
  if (configurations.some(({ id, name }) => id !== originalID && name.toLowerCase() === configuration.name.toLowerCase())) {
    errors.name = "Computer names must be unique."
  }
  const nameError = validateComputerName(configuration.name)
  if (nameError) errors.name = nameError
  if (configuration.cpus > configuration.maxCPUs) errors.cpus = "CPUs at start cannot exceed the maximum."
  if (configuration.memoryGiB > configuration.maxMemoryGiB) errors.memoryGiB = "Memory at start cannot exceed the maximum."
  return errors
}

export function configurationRequest(configurations: readonly SetupComputerConfiguration[]) {
  return setupComputerConfigurationRequestSchema.parse({ schemaVersion: 1, computers: configurations })
}
