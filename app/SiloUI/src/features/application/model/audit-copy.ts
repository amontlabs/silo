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
}

/** What a recorded call did, in a few words; a method this list does not know reads as its own name. */
export function auditAction(method: string): string {
  return phrases[method] ?? method
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
