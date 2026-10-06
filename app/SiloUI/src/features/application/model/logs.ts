import { hasBridgeErrorCode } from "@/contracts/bridge-error"
import { z } from "zod"
import type { ApplicationComputer } from "./application-source"

export interface LogQuery {
  computerId: string
  deviceId?: string
  query?: string
  source?: string
  since?: string
  until?: string
  cursor?: string
  limit?: number
  aroundId?: string
  /** Snapshot of the previous first page: Follow reads only records appended since. */
  follow?: string
}
export const logEntrySchema = z.object({
  id: z.string(), line: z.string(), occurredAt: z.string(), computerId: z.string(),
  computerName: z.string().optional(), deviceName: z.string().optional(),
  deviceId: z.string(), source: z.string(), session: z.string().nullish(),
  /** The time was parsed from console text the computer wrote. */
  guestTimestamp: z.boolean().optional(),
})
export const logPageSchema = z.object({
  entries: z.array(logEntrySchema), nextCursor: z.string().nullable(),
  oldestAvailableTimestamp: z.string().nullable(), newestAvailableTimestamp: z.string().nullable(),
  totalMatches: z.number(), timestampEstimated: z.boolean(),
  /** The owning device runs a Silo that cannot serve logs. */
  unsupported: z.boolean().optional(),
  /** Some records were malformed or too large and are shown as placeholders or truncated. */
  unreadableRecords: z.boolean().optional(),
  /** Snapshot this page came from; the next Follow refresh continues it. */
  snapshot: z.string().nullish(),
})
export function isUnsupportedRemote(reason: unknown): boolean {
  return hasBridgeErrorCode(reason, "unsupported_remote_operation")
}
export type LogEntry = z.infer<typeof logEntrySchema>
export type LogPage = z.infer<typeof logPageSchema>
export type LogLoader = (request: LogQuery) => Promise<LogPage>
export const LOG_ROW_HEIGHT = 52
/** Identifies a record across devices and computers; also keys its expanded state. */
export function logEntryKey(entry: Pick<LogEntry, "deviceId" | "computerId" | "id">): string {
  return `${entry.deviceId}\u0000${entry.computerId}\u0000${entry.id}`
}
export function logIdentity(computer: ApplicationComputer): Pick<LogQuery, "computerId" | "deviceId"> {
  return { computerId: computer.device?.computerId ?? computer.configuration.id, ...(computer.device && { deviceId: computer.device.id }) }
}
export function formatLog(entry: LogEntry): string {
  const device = entry.deviceName ? `${entry.deviceName} (${entry.deviceId})` : entry.deviceId
  const computer = entry.computerName ? `${entry.computerName} (${entry.computerId})` : entry.computerId
  return `${entry.occurredAt}\t${device}\t${computer}\t${entry.source}\t${entry.session ?? ""}\t${entry.line}`
}
/** Deterministic browser fixtures supply their entire history, never a production fallback. */
export function fixtureLogPage(computer: ApplicationComputer, request: LogQuery): LogPage {
  const fail = (message: string): never => { throw { code: "internal", message } }
  const encoder = new TextEncoder()
  if (encoder.encode(request.query ?? "").length > 4096) fail("Search text is too long.")
  if (request.source !== undefined && !["all", "stdout", "stderr", "output", "system", "runtime", "kernel"].includes(request.source)) fail("Unknown log source.")
  const timestamp = (value: string | undefined) => {
    if (value === undefined) return undefined
    const parsed = Date.parse(value)
    if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/i.test(value) || !Number.isFinite(parsed)) fail("Invalid log timestamp.")
    return parsed
  }
  const since = timestamp(request.since), until = timestamp(request.until)
  if (since !== undefined && until !== undefined && since > until) fail("The log time range is reversed.")
  const identity = logIdentity(computer)
  const all = computer.logs.map((log, index): LogEntry => ({ ...log, id: String(index), computerId: identity.computerId, computerName: computer.configuration.name, deviceId: identity.deviceId ?? "local", deviceName: computer.device?.name ?? "This device", source: "output", session: null }))
    .sort((a, b) => a.occurredAt === b.occurredAt ? a.id === b.id ? 0 : a.id < b.id ? 1 : -1 : a.occurredAt < b.occurredAt ? 1 : -1)
  let matches = all.filter(entry => (!request.query || entry.line.toLowerCase().includes(request.query.toLowerCase())) && (!request.source || request.source === "all" || entry.source === request.source) && (since === undefined || Date.parse(entry.occurredAt) >= since) && (until === undefined || Date.parse(entry.occurredAt) <= until))
  if (request.aroundId) {
    const index = all.findIndex(entry => entry.id === request.aroundId)
    if (index < 0) fail("The selected log record expired. Refresh the log search.")
    matches = all.slice(Math.max(0, index - 50), index + 51)
  }
  const offset = Number(request.cursor ?? 0), limit = Math.max(1, Math.min(200, request.limit ?? 200))
  const entries: LogEntry[] = []
  let bytes = 0
  for (const entry of request.aroundId ? matches : matches.slice(offset, offset + limit)) {
    let size = encoder.encode(JSON.stringify(entry)).length
    if (!request.aroundId && size > 1024 * 1024) {
      entry.line = new TextDecoder().decode(encoder.encode(entry.line).slice(0, 64 * 1024), { stream: true }) + " … [record over 1 MiB truncated]"
      size = encoder.encode(JSON.stringify(entry)).length
    }
    if (bytes + size > 1024 * 1024) {
      if (request.aroundId) fail("This context window is too large. Narrow the time range instead.")
      break
    }
    entries.push(entry)
    bytes += size
  }
  const next = offset + entries.length
  return { entries, nextCursor: !request.aroundId && next < matches.length ? String(next) : null, totalMatches: request.aroundId ? all.length : matches.length, oldestAvailableTimestamp: all.at(-1)?.occurredAt ?? null, newestAvailableTimestamp: all[0]?.occurredAt ?? null, timestampEstimated: false }
}
