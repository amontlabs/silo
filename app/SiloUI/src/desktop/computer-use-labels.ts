import type { ComputerUseState } from "./linux-desktop-state"

const stateLabels: Record<ComputerUseState["state"], string> = {
  unavailable: "Unavailable",
  installing: "Installing…",
  ready: "Ready",
  failed: "Setup failed",
}
export const computerUseLabel = (state: ComputerUseState["state"]) => stateLabels[state]
