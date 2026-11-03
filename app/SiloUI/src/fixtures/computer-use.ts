import type { ComputerUseBackend } from "@/desktop/computer-use-bridge"
import type { ApplicationSource } from "@/features/application/model/application-source"
import type { ComputerUseState, LinuxDesktopState } from "@/desktop/linux-desktop-state"
import { computerUseStates } from "@/desktop/linux-desktop-state"

// Deterministic fixtures for built-in computer use. Select with `?computer-use=<name>`
// in the browser preview; nothing here reaches Silo services.
export const computerUseFixtureNames = [...computerUseStates, "auto", "unknown-approval", "approval-pending", "approval-failed", "approval-partial", "pre-v4"] as const
export type ComputerUseFixtureName = typeof computerUseFixtureNames[number]

export function computerUseFixtureFromSearch(search: string): ComputerUseFixtureName | undefined {
  const requested = new URLSearchParams(search).get("computer-use")
  return computerUseFixtureNames.find(name => name === requested)
}
export function fixtureComputerUse(name: ComputerUseFixtureName): ComputerUseState {
  const base: ComputerUseState = {
    state: "ready", reason: null, approval: "ask", appliedApproval: "ask", approvalApply: "applied", approvalApplyReason: null,
    driverVersion: "0.38.0", agents: ["Claude Code", "Codex"],
  }
  switch (name) {
    case "unavailable": return { ...base, state: "unavailable", reason: "This computer was created before computer use was built in.", agents: null }
    case "installing": return { ...base, state: "installing", reason: "Configuring Claude Code and Codex.", driverVersion: null }
    case "failed": return { ...base, state: "failed", reason: "No supported agent was found. Install one, then set up computer use again.", agents: [] }
    case "auto": return { ...base, approval: "auto", appliedApproval: "auto" }
    case "unknown-approval": return { ...base, approval: "unknown", appliedApproval: "unknown" }
    // The user turned it off; Silo is applying it and the agents still have the old setting.
    case "approval-pending": return { ...base, approval: "ask", appliedApproval: "auto", approvalApply: "pending" }
    // Silo could not apply it: nothing is assumed rolled back.
    case "approval-failed": return { ...base, approval: "ask", appliedApproval: "auto", approvalApply: "failed", approvalApplyReason: "Applying took too long. Silo tries again when the computer starts." }
    case "approval-partial": return { ...base, approval: "ask", appliedApproval: "auto", approvalApply: "partial", approvalApplyReason: "Some agents could not be configured. Details are in /var/log/silo-computer-use.log in the computer." }
    default: return base
  }
}

export function fixtureDesktopState(name: ComputerUseFixtureName): LinuxDesktopState {
  const common = { installed: true, autoStart: true, state: "running" as const, sessionState: "running" as const, streamState: "running" as const }
  return name === "pre-v4" ? common : { ...common, computerUse: fixtureComputerUse(name) }
}

/** Marks the first computer as having a built-in desktop (or an older optional one). */
export function withComputerUseFixture(source: ApplicationSource, name: ComputerUseFixtureName): ApplicationSource {
  return {
    ...source,
    computers: source.computers.map((computer, index) => index === 0
      ? { ...computer, configuration: { ...computer.configuration, desktop: name === "pre-v4" ? { startWithComputer: true } : { startWithComputer: true, builtIn: true } } }
      : computer),
  }
}

/** A backend that behaves like the native one against in-memory state. */
export function createFixtureComputerUseBackend(name: ComputerUseFixtureName): ComputerUseBackend {
  let desktop = fixtureDesktopState(name)
  const delay = (ms: number) => new Promise(resolve => window.setTimeout(resolve, ms))
  const setUse = (patch: Partial<ComputerUseState>) => { if (desktop.computerUse) desktop = { ...desktop, computerUse: { ...desktop.computerUse, ...patch } } }
  return {
    readDesktopState: async () => structuredClone(desktop),
    // Like the native one: the choice is stored at once and applied in the background.
    setApproval: async (_computer, mode) => {
      setUse({ approval: mode, approvalApply: "pending", approvalApplyReason: null })
      window.setTimeout(() => setUse({ appliedApproval: mode, approvalApply: "applied" }), 900)
      return structuredClone(desktop)
    },
    setup: async () => { await delay(900); setUse({ state: "ready", reason: null, driverVersion: "0.38.0" }); return structuredClone(desktop) },
  }
}
