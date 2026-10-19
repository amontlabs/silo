import type { SetupComputerConfiguration } from "@/contracts/silo"
import { supportedCPUs, supportedMemoryGiB, type ComputerValidationErrors } from "@/features/onboarding/model/computer-configuration"

/**
 * What the runtime accepts for a computer's resources: CPU counts are stored as a `u8`,
 * memory as a `u32` of GiB, and the two disks share 4,194,303 GiB (`contracts/silo.ts`).
 */
export const runtimeLimits = {
  cpus: 255,
  memoryGiB: 4_294_967_295,
  storageGiB: 4_194_303,
} as const

/**
 * The device a computer runs on, as the runtime checks it before creating or changing a
 * computer: its CPU and memory ceilings may not exceed the logical CPUs and physical memory
 * (`validate_host_ceiling`). Undefined when the device has not reported it (D-46).
 */
export interface DeviceCapacity {
  logicalCPUs: number
  memoryGiB: number
}

/**
 * The editor's capacity for this device from the application state's `deviceCapacity`
 * (`logicalCpus`, `maxMemoryGib`: the exact ceilings the runtime accepts). The native state
 * passes unknown fields through unvalidated, so anything but positive whole numbers is
 * treated as unknown.
 */
export function deviceCapacityFrom(reported: unknown): DeviceCapacity | undefined {
  if (!reported || typeof reported !== "object") return undefined
  const { logicalCpus, maxMemoryGib } = reported as Record<string, unknown>
  const valid = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 1
  return valid(logicalCpus) && valid(maxMemoryGib) ? { logicalCPUs: logicalCpus, memoryGiB: maxMemoryGib } : undefined
}

type ResourceField = "cpus" | "maxCPUs" | "memoryGiB" | "maxMemoryGiB" | "workspaceStorageGiB" | "runtimeStorageGiB"
export const resourceFields: readonly ResourceField[] = ["cpus", "maxCPUs", "memoryGiB", "maxMemoryGiB", "workspaceStorageGiB", "runtimeStorageGiB"]

/** Reads a custom resource value typed by the user: only plain digits count ("1.5", "1e3" and "" do not). */
export function parseWholeNumber(text: string): number {
  const trimmed = text.trim()
  return /^\d+$/.test(trimmed) ? Number(trimmed) : 0
}

function wholeNumberIn(value: number, maximum: number) {
  return Number.isSafeInteger(value) && value >= 1 && value <= maximum
}

const number = (value: number) => value.toLocaleString("en-US")

/** The largest CPU and memory values a custom field accepts on this device. */
export function resourceMaximums(capacity?: DeviceCapacity) {
  return {
    cpus: Math.min(runtimeLimits.cpus, capacity?.logicalCPUs ?? runtimeLimits.cpus),
    memoryGiB: Math.min(runtimeLimits.memoryGiB, capacity?.memoryGiB ?? runtimeLimits.memoryGiB),
  }
}

/** Presets the device can run, plus its own maximum when the presets stop short of it. */
export function presetsWithin(presets: readonly number[], maximum: number | undefined): readonly number[] {
  if (maximum === undefined || !Number.isSafeInteger(maximum) || maximum < 1) return presets
  const within = presets.filter(value => value <= maximum)
  return within.includes(maximum) ? within : [...within, maximum]
}

function largestPresetAtMost(value: number, presets: readonly number[]) {
  return [...presets].reverse().find(preset => preset <= value) ?? 1
}

/**
 * New-computer defaults fitted to the device: ceilings no higher than the device, and
 * limits no more than half of it (snapped down to a preset) so the host keeps headroom.
 * Defaults a device can already run are unchanged.
 */
export function fitComputerToCapacity(configuration: SetupComputerConfiguration, capacity: DeviceCapacity | undefined): SetupComputerConfiguration {
  if (!capacity) return configuration
  const maximums = resourceMaximums(capacity)
  const maxCPUs = Math.min(configuration.maxCPUs, maximums.cpus)
  const maxMemoryGiB = Math.min(configuration.maxMemoryGiB, maximums.memoryGiB)
  const halfCPUs = largestPresetAtMost(Math.max(1, Math.floor(maximums.cpus / 2)), supportedCPUs)
  const halfMemoryGiB = largestPresetAtMost(Math.max(1, Math.floor(maximums.memoryGiB / 2)), supportedMemoryGiB)
  return {
    ...configuration,
    maxCPUs,
    cpus: Math.min(configuration.cpus, halfCPUs, maxCPUs),
    maxMemoryGiB,
    memoryGiB: Math.min(configuration.memoryGiB, halfMemoryGiB, maxMemoryGiB),
  }
}

/**
 * Readable range checks for a computer's resource fields. They replace the contract schema's
 * messages ("Too small: expected number to be >=1") for these fields and, when the
 * device's capacity is known, reject ceilings the runtime would refuse.
 */
export function validateComputerResources(configuration: SetupComputerConfiguration, capacity?: DeviceCapacity, deviceName = "This device"): ComputerValidationErrors {
  const errors: ComputerValidationErrors = {}
  const range = (field: ResourceField, maximum: number, unit: string) => {
    if (!wholeNumberIn(configuration[field], maximum)) errors[field] = `Enter a whole number of ${unit} from 1 to ${number(maximum)}.`
  }
  range("cpus", runtimeLimits.cpus, "CPUs")
  range("maxCPUs", runtimeLimits.cpus, "CPUs")
  range("memoryGiB", runtimeLimits.memoryGiB, "GiB")
  range("maxMemoryGiB", runtimeLimits.memoryGiB, "GiB")
  range("workspaceStorageGiB", runtimeLimits.storageGiB, "GiB")
  range("runtimeStorageGiB", runtimeLimits.storageGiB, "GiB")
  if (capacity) {
    const { cpus, memoryGiB } = resourceMaximums(capacity)
    if (!errors.maxCPUs && configuration.maxCPUs > cpus) errors.maxCPUs = `${deviceName} has ${number(cpus)} CPUs. Choose ${number(cpus)} or fewer.`
    if (!errors.maxMemoryGiB && configuration.maxMemoryGiB > memoryGiB) errors.maxMemoryGiB = `${deviceName} has ${number(memoryGiB)} GiB of memory. Choose ${number(memoryGiB)} GiB or fewer.`
  }
  if (!errors.cpus && !errors.maxCPUs && configuration.cpus > configuration.maxCPUs) errors.cpus = "CPUs at start cannot exceed the maximum."
  if (!errors.memoryGiB && !errors.maxMemoryGiB && configuration.memoryGiB > configuration.maxMemoryGiB) errors.memoryGiB = "Memory at start cannot exceed the maximum."
  if (!errors.workspaceStorageGiB && !errors.runtimeStorageGiB && configuration.workspaceStorageGiB + configuration.runtimeStorageGiB > runtimeLimits.storageGiB) {
    errors.workspaceStorageGiB = `Workspace and runtime storage together can't exceed ${number(runtimeLimits.storageGiB)} GiB.`
  }
  return errors
}
