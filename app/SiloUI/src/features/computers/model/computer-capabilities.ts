import type { SetupComputerConfiguration } from "@/contracts/silo"
import unavailableOnContainers from "./container-unavailable.json"

/** An action a computer's kind may not have yet. The engine refuses it by the same id (`containers::capability`). */
export type ComputerCapability = "terminal" | "editor" | "sshAccess" | "export" | "desktop" | "memorySnapshots"

const missingOnContainers: ReadonlySet<string> = new Set(unavailableOnContainers)

/** Whether the computer can do `capability`. The engine refuses the rest before doing anything, so the app does not offer them. */
export function computerCan(configuration: Pick<SetupComputerConfiguration, "runtime">, capability: ComputerCapability): boolean {
  return configuration.runtime !== "container" || !missingOnContainers.has(capability)
}
