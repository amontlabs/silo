import { formatLocalTimestamp } from "@/lib/format-date"
import { useMemo } from "react"

import { LogDisclosure } from "@/components/log-disclosure"
import type { SiloProgressEvent } from "@/contracts/silo"

function eventLine(event: SiloProgressEvent): string {
  const download = event.downloadedBytes === undefined ? undefined
    : event.totalBytes === undefined ? `${formatBytes(event.downloadedBytes)} downloaded`
      : `${formatBytes(event.downloadedBytes)} / ${formatBytes(event.totalBytes)} downloaded`
  return [
    event.timestamp === undefined ? undefined : formatLocalTimestamp(event.timestamp),
    event.level === "error" ? "Error" : event.level === "warning" ? "Warning" : undefined,
    event.computer,
    event.message,
    download,
    event.elapsedSeconds === undefined ? undefined : `${Math.floor(event.elapsedSeconds)}s elapsed`,
  ].filter(Boolean).join("  ·  ")
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ["KiB", "MiB", "GiB", "TiB"]
  const index = Math.min(Math.floor(Math.log2(bytes) / 10) - 1, units.length - 1)
  return `${Number((bytes / 1024 ** (index + 1)).toFixed(1))} ${units[index]}`
}

function activityOutput(events: SiloProgressEvent[]): string {
  const attempts = new Map<string, string[]>()
  for (const event of events) {
    if (!event.safeForDisplay) continue
    const lines = attempts.get(event.requestId) ?? []
    lines.push(eventLine(event))
    if (event.step === "setup-failed" && event.diagnostic) lines.push(event.diagnostic)
    attempts.set(event.requestId, lines)
  }
  return [...attempts.values()].map((lines, index) => `Attempt ${index + 1}\n${lines.join("\n")}`).join("\n\n")
}

export function ActivityOutput({ events, error, embedded = false }: { events: SiloProgressEvent[]; error?: string; embedded?: boolean }) {
  const output = useMemo(() => [error, activityOutput(events)].filter(Boolean).join("\n\n"), [events, error])
  return <LogDisclosure
    title="Live activity"
    output={output}
    outputLabel="Computer activity"
    controlsLabel="Live activity controls"
    emptyMessage="No activity yet."
    embedded={embedded}
    labels={{ expand: "Expand activity", collapse: "Collapse activity", copy: "Copy activity", copied: "Activity copied", copyFailed: "Copy activity failed" }}
  />
}
