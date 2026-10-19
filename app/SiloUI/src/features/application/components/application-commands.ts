import { Activity, Bell, Boxes, CircleAlert, Code, Download, File, GitFork, History, KeyRound, Monitor, Network, Play, Plus, RotateCw, Settings2, Square, Terminal, Trash2, Upload, type LucideIcon } from "lucide-react"

import type { ApplicationActions, ApplicationSource } from "@/features/application/model/application-source"
import type { ApplicationInitialRoute } from "@/features/application/model/use-application-navigation"
import { computerTarget } from "@/features/application/model/connections"
import { computerAvailability } from "@/features/application/model/computer-availability"
import { lifecycleGuard, type LifecycleAction } from "@/features/application/model/lifecycle-guard"

/** A question the palette asks, in place, before running a command. */
export interface CommandConfirmation {
  title: string
  description: string
  confirmLabel: string
  tone: "default" | "destructive"
}

export interface ApplicationCommand {
  id: string
  label: string
  group: "Go to" | "Computers" | "Actions"
  icon: LucideIcon
  keywords?: string[]
  /** Asked inside the palette first; `run` then proceeds as confirmed. */
  confirm?: CommandConfirmation
  /** The command opens a popover or picker in the window: closing the palette leaves focus to it. */
  opensPanel?: boolean
  run: () => void
}

/** What a palette command opens on a computer's page, in place of the page's own button. */
export type ComputerCommandRequest = "editor" | "fork" | "delete"

export interface ApplicationCommandOptions {
  onImportComputer?: () => void
  onNewComputer?: () => void
  onExportComputer?: (computerName: string) => void
  /** Opens the computer's page and there its editor folder picker, Fork or Delete popover. */
  onComputerRequest?: (computerId: string, request: ComputerCommandRequest) => void
}

const computerSections = [
  { section: "files", label: "Files", icon: File, keywords: ["folders", "repositories"] },
  { section: "logs", label: "Logs", icon: Terminal, keywords: ["diagnostics", "output"] },
  { section: "network", label: "Network", icon: Network, keywords: ["ports", "connections"] },
  { section: "activity", label: "Activity", icon: Activity, keywords: ["history", "events"] },
] as const

export function applicationCommands(source: ApplicationSource, actions: ApplicationActions, navigate: (route: ApplicationInitialRoute) => void, { onImportComputer, onNewComputer, onExportComputer, onComputerRequest }: ApplicationCommandOptions = {}): ApplicationCommand[] {
  // Lifecycle commands use the same guard as the pages: unavailable operations are reported,
  // and a request that needs a prompt asks it inside the palette.
  const guard = lifecycleGuard(source, actions)
  const destinations: { label: string; icon: LucideIcon; route: ApplicationInitialRoute; keywords?: string[] }[] = [
    { label: "All computers", icon: Boxes, route: { computerSection: "overview" }, keywords: ["overview", "computers", "configurations"] },
    ...computerSections.map(({ section, label, icon, keywords }) => ({ label, icon, route: { computerSection: section }, keywords: [...keywords] })),
    { label: "GitHub", icon: GitFork, route: { tab: "github" }, keywords: ["git", "account", "access"] },
    { label: "Secrets", icon: KeyRound, route: { tab: "secrets" }, keywords: ["tokens", "credentials"] },
    { label: "Settings", icon: Settings2, route: { settingsSection: "general" }, keywords: ["general", "preferences", "applications", "computer use", "agents", "approval"] },
    { label: "Connections", icon: Monitor, route: { settingsSection: "connections" }, keywords: ["remote", "ssh", "connections", "management", "devices"] },
    { label: "Notifications", icon: Bell, route: { settingsSection: "notifications" }, keywords: ["alerts"] },
  ]
  if (source.runtimeRepair) {
    destinations.push({ label: "System issue", icon: CircleAlert, route: { tab: "system" }, keywords: ["checks", "runtime", "installation"] })
  }
  const commands: ApplicationCommand[] = destinations.map(({ label, icon, route, keywords }) => ({
    id: `page:${label}`, label, icon, keywords, group: "Go to", run: () => navigate(route),
  }))

  if (onNewComputer) {
    commands.push({ id: "action:new-computer", label: "New computer…", icon: Plus, group: "Actions", keywords: ["create", "add", "vm"], run: onNewComputer })
  }
  if (onImportComputer) {
    commands.push({ id: "action:import-computer", label: "Import computer…", icon: Upload, group: "Actions", keywords: ["restore", "archive", "backup", "transfer"], run: onImportComputer })
  }

  for (const computer of source.computers) {
    const { id } = computer.configuration
    // Remote computers are addressed by their device target and named with their device,
    // so a remote "dev" never resolves to (or reads like) a local "dev".
    const target = computerTarget(computer)
    const name = computer.device ? `${computer.configuration.name} on ${computer.device.name}` : computer.configuration.name
    const computerKeywords = computer.device ? [computer.configuration.name, computer.device.name] : [name]
    const availability = computerAvailability(computer, source)
    commands.push({ id: `${id}:page`, label: `Open ${name}`, icon: Boxes, group: "Computers", keywords: [...computerKeywords, "computer", "details"], run: () => navigate({ computer: id }) })
    commands.push({ id: `${id}:checkpoints`, label: `Open ${name} checkpoints`, icon: History, group: "Computers", keywords: [...computerKeywords, "checkpoints", "restore", "snapshot"], run: () => navigate({ computer: id, computerTab: "checkpoints" }) })
    for (const { section, label, icon, keywords } of computerSections) {
      commands.push({
        id: `${id}:${section}`, label: `Open ${name} ${label.toLowerCase()}`, icon, group: "Computers",
        keywords: [...computerKeywords, ...keywords], run: () => navigate({ computer: id, computerSection: section }),
      })
    }
    if (availability.canOpen) {
      commands.push(
        { id: `${id}:terminal`, label: `Open ${name} in ${source.preferences.terminal}`, icon: Terminal, group: "Actions", keywords: [...computerKeywords, "terminal", "shell"], run: () => actions.openTerminal(target) },
        // Like the editor buttons, the command asks which folder to open first.
        onComputerRequest
          ? { id: `${id}:editor`, label: `Open ${name} in ${source.preferences.editor}…`, icon: Code, group: "Actions", keywords: [...computerKeywords, "editor", "code", "folder"], opensPanel: true, run: () => onComputerRequest(id, "editor") }
          : { id: `${id}:editor`, label: `Open ${name} in ${source.preferences.editor}`, icon: Code, group: "Actions", keywords: [...computerKeywords, "editor", "code"], run: () => actions.openEditor(target) },
      )
    }
    // Fork and Delete open the computer page's own popovers; they follow the page's ⋯ menu rules.
    const changing = source.computerConfigurationOperation !== null || availability.busy || computer.freshness === "stale"
    if (actions.forkCheckpoint && onComputerRequest && !changing) {
      commands.push({ id: `${id}:fork`, label: `Fork ${name}…`, icon: GitFork, group: "Actions", keywords: [...computerKeywords, "fork", "copy", "clone"], opensPanel: true, run: () => onComputerRequest(id, "fork") })
    }
    if (!computer.device && onExportComputer && !changing) {
      commands.push({ id: `${id}:export`, label: `Export ${name}…`, icon: Download, group: "Actions", keywords: [...computerKeywords, "export", "backup", "archive"], run: () => onExportComputer(computer.configuration.name) })
    }
    const offline = Boolean(computer.device && !computer.device.connected)
    if (onComputerRequest && source.computerConfigurationOperation === null && !availability.busy && !offline && computer.state !== "running") {
      commands.push({ id: `${id}:delete`, label: `Delete ${name}…`, icon: Trash2, group: "Actions", keywords: [...computerKeywords, "delete", "remove"], opensPanel: true, run: () => onComputerRequest(id, "delete") })
    }
    const lifecycle: { action: LifecycleAction; label: string; icon: LucideIcon; available: boolean }[] = [
      { action: "start", label: "Start", icon: Play, available: availability.canStart },
      { action: "stop", label: "Stop", icon: Square, available: availability.canStop },
      { action: "restart", label: "Restart", icon: RotateCw, available: availability.canRestart },
    ]
    for (const { action, label, icon, available } of lifecycle) {
      if (!available) continue
      const check = guard.check(computer, action)
      const confirm = check.kind === "confirm" ? check.prompt : undefined
      commands.push({
        id: `${id}:${label}`, label: `${label} ${name}${confirm ? "…" : ""}`, icon, group: "Actions", keywords: ["computer", ...computerKeywords], confirm,
        run: () => {
          if (confirm) guard.confirm(computer, action)
          else guard.request(computer, action)
        },
      })
    }
  }
  return commands
}
