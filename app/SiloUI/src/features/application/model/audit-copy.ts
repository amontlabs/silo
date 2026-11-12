import type { AuditEntry } from "@/desktop/production-schemas"
import type { ApplicationComputer } from "./application-source"

const phrases: Record<string, string> = {
  "computers.start": "Started a computer",
  "computers.stop": "Stopped a computer",
  "computers.restart": "Restarted a computer",
  "computers.forceStop": "Force-stopped a computer",
  "computers.create": "Created a computer",
  "computers.delete": "Deleted a computer",
  "computers.configuration.change": "Changed a computer's settings",
  "changes.keep": "Merged changes",
  "changes.promote": "Replaced main",
  "changes.undo": "Undid a merge",
  "changes.request": "Asked for a review",
  "changes.requests.keep": "Merged a fork",
  "changes.requests.throwAway": "Deleted a fork",
  "changes.requests.open": "Opened a fork",
  "checkpoints.create": "Took a checkpoint",
  "checkpoints.restore": "Restored a checkpoint",
  "checkpoints.delete": "Deleted a checkpoint",
  "checkpoints.fork": "Forked a checkpoint",
  "network.publish": "Published a port",
  "network.unpublish": "Stopped publishing a port",
  "secrets.save": "Saved a secret",
  "secrets.remove": "Removed a secret",
  "github.access.set": "Changed GitHub access",
  "github.configuration.save": "Changed GitHub access",
  "sshAccess.save": "Changed SSH access",
  "guest.exec": "Ran a command",
  "guest.exec.start": "Ran a command",
  "files.upload": "Uploaded files",
  "files.download": "Downloaded a file",
  "approvals.decide": "Answered a request",
  "approvals.cancel": "Withdrew a request",
  "approvals.note": "Added a note to a request",
  "settings.update": "Changed a setting",
  "recipes.build": "Built a recipe",
  "agents.install": "Installed an agent",
  "exports.create": "Exported a computer",
  "exports.import": "Imported a computer",
  "storage.reclaim": "Reclaimed storage",
  "engine.shutdown": "Shut Silo down",
  "computers.fork": "Forked a computer",
  "computers.rebuild": "Rebuilt a computer",
  "computers.dismissError": "Cleared a computer's error",
  "computers.configuration.retry": "Retried a computer's settings",
  "computers.identities.configure": "Set up a computer's identities",
  "computers.access": "Looked at how to reach a computer",
  "computers.credentials": "Looked at a computer's credentials",
  "computers.shell": "Opened a shell in a computer",
  "computers.launch": "Opened a computer",
  "changes.baseline": "Marked a starting point for changes",
  "changes.capture": "Captured a computer's changes",
  "changes.diff": "Looked at a computer's changes",
  "changes.requests.show": "Opened a review",
  "changes.requests.markKept": "Marked a fork as merged",
  "checkpoints.abandonRestore": "Gave up a restore",
  "clipboard.read": "Read the clipboard of a computer",
  "clipboard.write": "Wrote to the clipboard of a computer",
  "clipboard.transfer": "Copied between the clipboards",
  "computerUse.approval": "Answered a screen control request",
  "computerUse.setup": "Set up screen control",
  "connections.devices.connect": "Connected a device",
  "connections.devices.remove": "Removed a device",
  "connections.keys.authorize": "Allowed a device key",
  "connections.keys.revoke": "Revoked a device key",
  "connections.setEnabled": "Changed device connections",
  "desktop.action": "Used a computer's desktop",
  "display.show": "Showed a computer's screen",
  "display.close": "Closed a computer's screen",
  "editor.command": "Opened an editor",
  "exports.check": "Checked an export",
  "exports.inspect": "Looked inside an export",
  "exports.transfer": "Moved an export",
  "exports.discard": "Discarded an export",
  "exports.destination.set": "Changed where exports go",
  "github.connect": "Connected GitHub",
  "github.disconnect": "Disconnected GitHub",
  "github.configuration.retry": "Retried GitHub access",
  "github.personalToken.save": "Saved a GitHub token",
  "github.personalToken.remove": "Removed a GitHub token",
  "github.repositories.manage": "Changed GitHub repositories",
  "guest.files.write": "Wrote a file in a computer",
  "guest.files.remove": "Removed a file in a computer",
  "guest.files.read": "Read a file in a computer",
  "guest.files.list": "Listed files in a computer",
  "guest.exec.input": "Typed into a command",
  "guest.prepare": "Prepared a computer",
  "guest.ssh": "Connected to a computer over SSH",
  "guest.sshAlias.ensure": "Added an SSH shortcut",
  "guest.sshAlias.remove": "Removed an SSH shortcut",
  "guestClient.status": "Checked Silo's tools in a computer",
  "guestClient.enable": "Turned on Silo's tools in a computer",
  "guestClient.revoke": "Turned off Silo's tools in a computer",
  "logs.export": "Exported logs",
  "macos.setup.retry": "Retried setting up a macOS computer",
  "macos.template.delete": "Deleted the macOS template",
  "migration.preUpgradeBackup.delete": "Deleted the backup made before an upgrade",
  "network.policy.set": "Changed a network policy",
  "operations.cancel": "Cancelled a task",
  "operations.job.acknowledge": "Marked a background job as seen",
  "preparation.retry": "Retried preparing Silo",
  "push.start": "Pushed to GitHub",
  "recipes.save": "Saved a recipe",
  "recipes.remove": "Removed a recipe",
  "secrets.retry": "Retried saving a secret",
  "steps.record": "Recorded a step",
  "updates.download": "Downloaded an update",
  "engine.update.prepare": "Prepared Silo for an update",
}

/** Calls that only keep the record straight; when they went fine they are left out of the list. */
const bookkeeping = new Set([
  "operations.job.acknowledge",
  "exports.acknowledge",
  "exports.dismiss",
  "push.dismiss",
  "computers.dismissError",
  "migration.preUpgradeBackup.acknowledgeNotice",
  "settings.flush",
  "updates.focused",
  "engine.disconnect",
  "events.unsubscribe",
  "guestClient.status",
])

export const isAuditBookkeeping = ({ method, outcome }: Pick<AuditEntry, "method" | "outcome">) => outcome === "ok" && bookkeeping.has(method)

const areas: Record<string, string> = {
  activity: "activity", agents: "agents", approvals: "requests", audit: "activity", changes: "changes", checkpoints: "checkpoints",
  clipboard: "the clipboard", computerUse: "screen control", computers: "computers", connections: "devices", desktop: "the desktop",
  device: "this device", display: "the screen", editor: "the editor", engine: "Silo", exports: "exports", files: "files",
  github: "GitHub", guest: "a computer", guestClient: "Silo's tools", logs: "logs", macos: "macOS", migration: "storage",
  network: "the network", notifications: "notifications", operations: "tasks", preparation: "Silo", push: "GitHub", recipes: "recipes",
  secrets: "secrets", settings: "settings", sshAccess: "SSH access", steps: "steps", storage: "storage", terminal: "the terminal", updates: "updates",
}

/** What a recorded call did, in a few words; a method this list does not know reads as a request about its area, never as its own name. */
export function auditAction(method: string): string {
  const known = phrases[method]
  if (known) return known
  const area = areas[method.split(".")[0] ?? ""]
  return area ? `Made a request about ${area}` : "Made a request"
}

const outcomes: Record<AuditEntry["outcome"], string> = { ok: "Done", error: "Failed", refused: "Refused", asked: "Asked you" }
export const auditOutcome = (outcome: AuditEntry["outcome"]) => outcomes[outcome]

const byId = (computers: readonly ApplicationComputer[], id: string | undefined) =>
  id ? computers.find(({ configuration, device }) => configuration.id === id || device?.computerId === id)?.configuration.name : undefined

/** Who made a recorded call: you, another device, or the computer's name. */
export function auditWho(entry: AuditEntry, computers: readonly ApplicationComputer[]): string {
  if (entry.callerKind === "owner") return "You"
  if (entry.callerKind === "device") return "Another device"
  return byId(computers, entry.callerId) ?? "A removed computer"
}

/** The computer a recorded call named, or nothing. */
export function auditTarget(entry: AuditEntry, computers: readonly ApplicationComputer[]): string | undefined {
  if (!entry.computerId) return undefined
  return byId(computers, entry.computerId) ?? "A removed computer"
}
