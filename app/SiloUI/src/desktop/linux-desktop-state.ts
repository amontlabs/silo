import { z } from "zod"

const desktopSessionStateSchema = z.enum(["stopped", "starting", "running", "failed"])

// Built-in computer use (v4 computers). Every field tolerates a newer or malformed value,
// so a diagnostic detail can never make the desktop state unreadable.
export const computerUseStates = ["unavailable", "preparing", "installing", "ready", "failed"] as const
export const computerUseSchema = z.object({
  state: z.enum(computerUseStates).catch("unavailable"),
  reason: z.string().nullish().catch(null),
  // `app-download`: a failure of the host's ChatGPT download, which setting up the computer cannot fix.
  cause: z.enum(["app-download"]).nullish().catch(null),
  compatibility: z.enum(["tested", "untested", "unknown"]).catch("unknown"),
  warning: z.string().nullish().catch(null),
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
  appVersion: z.string().nullish().catch(null),
  runtimeVersion: z.string().nullish().catch(null),
  lcuVersion: z.string().nullish().catch(null),
  agents: z.array(z.string()).nullish().catch(null),
})
export type ComputerUseState = z.infer<typeof computerUseSchema>
/** The modes a user can set; `ComputerUseState.approval` adds "unknown" for an unreadable policy. */
export type ComputerUseApproval = Exclude<ComputerUseState["approval"], "unknown">

// The official ChatGPT Linux app, downloaded automatically by every device that runs Silo.
// "unknown" is a device whose status cannot be read (an older Silo, or a newer state this
// one does not know): it is never an error.
export const chatGptAppStatusSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("unknown") }),
  z.object({ state: z.literal("idle") }),
  z.object({ state: z.literal("downloading"), receivedBytes: z.number().nonnegative().catch(0), totalBytes: z.number().nonnegative().nullish().catch(null) }),
  z.object({ state: z.literal("verifying") }),
  z.object({ state: z.literal("extracting") }),
  z.object({ state: z.literal("ready"), path: z.string().nullish().catch(null), version: z.string().nullish().catch(null) }),
  z.object({ state: z.literal("failed"), reason: z.string().catch("The download did not finish."), retryable: z.boolean().catch(false) }),
])
export type ChatGptAppStatus = z.infer<typeof chatGptAppStatusSchema>

/** A status the schema knows, `unknown` for any other tagged state (an older Silo's
 * `notConsented`, a newer one's addition), and null for a value that is not a status. */
export function parseChatGptAppStatus(value: unknown): ChatGptAppStatus | null {
  const parsed = chatGptAppStatusSchema.safeParse(value)
  if (parsed.success) return parsed.data
  const state = typeof value === "object" && value !== null ? (value as { state?: unknown }).state : undefined
  return typeof state === "string" ? { state: "unknown" } : null
}

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
  lcuState: z.enum(["needs-runtime", "not-installed", "installing", "repair-required", "failed", "ready"]).nullish(),
  lcuReason: z.string().nullish().catch(null),
  lcuVersion: z.string().nullish().catch(null),
  lcuAppVersion: z.string().nullish().catch(null),
  lcuRuntimeVersion: z.string().nullish().catch(null),
  lcuAgents: z.array(z.string()).nullish().catch(null),
  // Diagnostic fields must never make the whole desktop state unreadable.
  // Present only on v4 computers, where the desktop is built in. Older computers report the lcu* fields.
  computerUse: computerUseSchema.nullish().catch(null),
  lcuReadiness: z.enum(["ready", "unverified", "failed"]).nullish().catch(null),
  port: z.number().nullish().catch(null),
  user: z.string().nullish().catch(null),
  display: z.string().nullish().catch(null),
})
export type LinuxDesktopState = z.infer<typeof linuxDesktopStateSchema>
export type DesktopAction = "start" | "stop" | "restart" | "setup-lcu" | "setup-computer-use" | "restart-streamer" | "update-streamer"

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
