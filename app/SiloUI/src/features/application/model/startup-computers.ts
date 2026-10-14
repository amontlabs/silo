import type { ApplicationComputer } from "./application-source"

/** Only computers on this device can start when Silo opens. */
export function startupComputerCandidates(computers: readonly ApplicationComputer[]): ApplicationComputer[] {
  return computers.filter(computer => !computer.device)
}

/** The startup selection before the user chooses one: the local "dev" computer, else the first local one. */
export function defaultStartupComputerIds(computers: readonly ApplicationComputer[], macosComputers: readonly { id: string; installed?: boolean; state?: string; needsPersonalizing?: boolean }[] = []): string[] {
  const candidates = startupComputerCandidates(computers)
  const initial = candidates.find(({ configuration }) => configuration.name === "dev") ?? candidates[0]
  if (initial) return [initial.configuration.id]
  const initialMac = macosComputers.find(computer => computer.installed && (computer.state === "stopped" || computer.state === "failed") && !computer.needsPersonalizing)
  return initialMac ? [initialMac.id] : []
}
