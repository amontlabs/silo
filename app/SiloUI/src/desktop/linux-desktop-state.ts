import { z } from "zod"

const desktopSessionStateSchema = z.enum(["stopped", "starting", "running", "failed"])

// Built-in computer use (computers whose desktop is built in). Every field tolerates a newer or malformed value,
// so a diagnostic detail can never make the desktop state unreadable.
export const computerUseStates = ["unavailable", "installing", "ready", "failed"] as const
export const computerUseSchema = z.object({
  state: z.enum(computerUseStates).catch("unavailable"),
  reason: z.string().nullish().catch(null),
  // What the user chose, shown by the switch. A missing or unreadable policy is "unknown", never "ask".
  approval: z.enum(["ask", "auto", "unknown"]).catch("unknown"),
  // The last mode Silo applied completely ("unknown" before any was, or when an older Silo does not
  // report it). It differs from `approval` while a change is pending, failed or only partly worked.
  appliedApproval: z.enum(["ask", "auto", "unknown"]).catch("unknown"),
  // How applying `approval` stands: Silo drives the computer toward the chosen mode itself and keeps
  // the last result. Missing or malformed status is unknown, never a confirmed success.
  approvalApply: z.enum(["applied", "pending", "failed", "partial", "unknown"]).catch("unknown"),
  // Why it failed or only partly worked, in words for the user.
  approvalApplyReason: z.string().nullish().catch(null),
  // The Cua Driver version in the computer.
  driverVersion: z.string().nullish().catch(null),
  agents: z.array(z.string()).nullish().catch(null),
})
export type ComputerUseState = z.infer<typeof computerUseSchema>
/** The modes a user can set; `ComputerUseState.approval` adds "unknown" for an unreadable policy. */
export type ComputerUseApproval = Exclude<ComputerUseState["approval"], "unknown">

export const linuxDesktopStateSchema = z.object({
  installed: z.boolean(),
  version: z.string().nullish().catch(null),
  streamerVersion: z.string().nullish().catch(null),
  state: z.enum(["running", "starting", "stopped", "failed", "uninstalled", "computer-stopped"]),
  autoStart: z.boolean(),
  backend: z.enum(["kasm", "selkies"]).nullish(),
  sessionState: desktopSessionStateSchema.nullish(),
  streamState: desktopSessionStateSchema.nullish(),
  // Required: the installed desktop cannot start. Available: it starts, and an update adds newer features.
  updateRequired: z.boolean().nullish(),
  updateAvailable: z.boolean().nullish(),
  // Diagnostic fields must never make the whole desktop state unreadable.
  // Present only on computers whose desktop is built in.
  computerUse: computerUseSchema.nullish().catch(null),
  port: z.number().nullish().catch(null),
  user: z.string().nullish().catch(null),
  display: z.string().nullish().catch(null),
})
export type LinuxDesktopState = z.infer<typeof linuxDesktopStateSchema>
export type DesktopAction = "start" | "stop" | "restart" | "setup-computer-use" | "restart-streamer" | "update-streamer"

export function parseLinuxDesktopState(value: unknown): LinuxDesktopState {
  const status = linuxDesktopStateSchema.parse(value)
  // New guests report the X session separately from its streamer. Preserve
  // legacy special states while deriving desktop health from the X session.
  return {
    ...status,
    state: status.state === "uninstalled" || status.state === "computer-stopped"
      ? status.state
      : status.sessionState ?? status.state,
  }
}

export function desktopViewerRoute() {
  const query = new URLSearchParams(window.location.search)
  const computer = query.get("desktop")
  return computer ? { computer, name: query.get("name") ?? computer, id: query.get("id") ?? computer } : null
}
