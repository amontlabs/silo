import type { ComputerOs } from "@/features/application/model/application-source"
import type { SetupComputerConfiguration } from "@/contracts/silo"
import unavailableOnContainers from "./container-unavailable.json"

/**
 * An action a computer may not have. `terminal` to `memorySnapshots` are the ones a container
 * computer lacks (the engine refuses them by the same id, `containers::capability`); the rest
 * are what the platform decides: macOS computers have no restart, SSH access for other devices,
 * workspace storage report or fork of the current state, and a macOS computer on another device
 * offers only what its owner serves.
 */
export type ComputerCapability =
  | "terminal" | "editor" | "sshAccess" | "desktop" | "memorySnapshots"
  | "restart" | "storage" | "files" | "ports" | "secrets" | "github" | "checkpoints" | "fork"

/** What decides a computer's capabilities: its runtime, its system, and whether another device hosts it. */
export interface ComputerKind extends Partial<Pick<SetupComputerConfiguration, "runtime">> {
  os?: ComputerOs
  remote?: boolean
}

const missingOnContainers: ReadonlySet<string> = new Set(unavailableOnContainers)

/** Not offered on macOS: the engine serves SSH access, storage reports and Restart for Linux computers only; a macOS fork is made from a checkpoint. */
const missingOnMacos: ReadonlySet<ComputerCapability> = new Set(["sshAccess", "restart", "storage", "fork"])

/** Served for computers of this device only. */
const localOnMacos: ReadonlySet<ComputerCapability> = new Set(["terminal", "editor", "files", "ports", "secrets", "github", "checkpoints", "memorySnapshots"])

/** Whether the computer can do `capability`. The engine refuses the rest before doing anything, so the app does not offer them. */
export function computerCan(kind: ComputerKind, capability: ComputerCapability): boolean {
  if (kind.os === "macos") return !missingOnMacos.has(capability) && (!kind.remote || !localOnMacos.has(capability))
  return kind.runtime !== "container" || !missingOnContainers.has(capability)
}

/** The kind of an application computer, for `computerCan`. */
export function kindOf(computer: { configuration: Pick<SetupComputerConfiguration, "runtime">; os?: ComputerOs; device?: unknown }): ComputerKind {
  return { runtime: computer.configuration.runtime, os: computer.os, remote: Boolean(computer.device) }
}

/** Where the computer's files live: the working account's workspace folder. */
export function workspaceRoot(computer: { os?: ComputerOs }): string {
  return computer.os === "macos" ? "/Users/silo/workspace" : "/workspace"
}
