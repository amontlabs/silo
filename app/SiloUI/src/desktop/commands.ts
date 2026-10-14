import { invoke } from "@tauri-apps/api/core"

export function desktopCommand(command: "hide_status" | "open_main" | "quit_app", args?: Record<string, unknown>) {
  return (args === undefined ? invoke(command) : invoke(command, args)).catch((error: unknown) => console.error(`Desktop ${command} failed`, error))
}

