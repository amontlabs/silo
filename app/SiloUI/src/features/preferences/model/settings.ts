import { z } from "zod"
import { applicationPathSchema } from "./application-preferences"

export const settingSchemas = {
  theme: z.enum(["system", "dark", "light"]),
  launchAtLogin: z.boolean(),
  startComputersAtLaunch: z.boolean(),
  startupComputerIds: z.array(z.string().min(1).max(256)).max(256),
  /** This device's order for the computer list, local and remote, by `computerOrderKey`. */
  computerOrder: z.array(z.string().min(1).max(512)).max(1024),
  terminal: z.string().min(1).max(256),
  editor: z.string().min(1).max(256),
  browser: z.string().min(1).max(256),
  terminalPath: applicationPathSchema,
  editorPath: applicationPathSchema,
  browserPath: applicationPathSchema,
  terminalUseSystemDefault: z.boolean(),
  editorUseSystemDefault: z.boolean(),
  browserUseSystemDefault: z.boolean(),
  reduceMotion: z.boolean(),
  /** Whether computers created or imported on this device start with agents allowed to use the desktop without asking. */
  computerUseAutoApproval: z.boolean(),
  notificationsEnabled: z.boolean(),
  notifyFailures: z.boolean(),
  notifyChanges: z.boolean(),
  notifyCompletions: z.boolean(),
  /** Whether a computer's agent asking for your answer sends a system notification. */
  notifyApprovals: z.boolean(),
  /** The agent the person uses, which later computers suggest; null until chosen, "none" for one they set up themselves. */
  agent: z.enum(["claude-code", "codex", "none"]).nullable(),
  onboardingComplete: z.boolean(),
  alphaNoticeDismissed: z.boolean(),
  /** The SSH `Include` line whose notice was dismissed; a different needed line shows it again. */
  editorIncludeNoticeDismissed: z.string().min(1).max(8192).nullable(),
} as const

export const settingsSchema = z.object(settingSchemas).strict()
export const settingsPatchSchema = settingsSchema.partial()
export type Settings = z.infer<typeof settingsSchema>
export type SettingsPatch = Partial<Settings>

export const defaultSettings: Settings = {
  theme: "system",
  launchAtLogin: true,
  startComputersAtLaunch: false,
  startupComputerIds: [],
  computerOrder: [],
  terminal: "Terminal",
  editor: "Visual Studio Code",
  browser: "Safari",
  terminalPath: null,
  editorPath: null,
  browserPath: null,
  terminalUseSystemDefault: true,
  editorUseSystemDefault: true,
  browserUseSystemDefault: true,
  reduceMotion: false,
  computerUseAutoApproval: false,
  notificationsEnabled: true,
  notifyFailures: true,
  notifyChanges: true,
  notifyCompletions: true,
  notifyApprovals: true,
  agent: null,
  onboardingComplete: false,
  alphaNoticeDismissed: false,
  editorIncludeNoticeDismissed: null,
}

// Read fields independently: one invalid field must not erase other saved choices.
// Unknown fields remain owned by the native document and never become UI settings.
export function readSettingsOverrides(input: Record<string, unknown>): SettingsPatch {
  const result: Record<string, unknown> = {}
  for (const [key, schema] of Object.entries(settingSchemas)) {
    if (!(key in input)) continue
    const parsed = schema.safeParse(input[key])
    if (parsed.success) result[key] = parsed.data
  }
  // Before the notification categories were reworked, three per-area switches existed.
  // A saved choice to silence an area keeps silencing its successor until the new key is saved.
  const legacyOn = (key: string) => input[key] !== false
  if (!("notifyFailures" in result) && !("notifyFailures" in input)) {
    if ("notifyActions" in input || "notifyBackup" in input) result.notifyFailures = legacyOn("notifyActions") && legacyOn("notifyBackup")
  }
  if (!("notifyChanges" in result) && !("notifyChanges" in input)) {
    if ("notifyHealth" in input) result.notifyChanges = legacyOn("notifyHealth")
  }
  return result as SettingsPatch
}
