import { formatLocalTimestamp } from "@/lib/format-date"

const divisions: { amount: number; unit: Intl.RelativeTimeFormatUnit }[] = [
  { amount: 60, unit: "second" },
  { amount: 60, unit: "minute" },
  { amount: 24, unit: "hour" },
  { amount: 7, unit: "day" },
  { amount: 4.34524, unit: "week" },
  { amount: 12, unit: "month" },
  { amount: Number.POSITIVE_INFINITY, unit: "year" },
]

/**
 * Format an ISO timestamp as a short relative phrase ("2 hours ago", "yesterday").
 * Returns an empty string for an unparseable input so callers can fall back to the
 * absolute timestamp they keep in a tooltip.
 */
export function formatRelativeTime(iso: string, now: Date = new Date()): string {
  const then = new Date(iso)
  if (Number.isNaN(then.getTime())) return ""
  // The phrase is words, so it follows the UI language (English), not the system locale.
  const formatter = new Intl.RelativeTimeFormat("en", { numeric: "auto" })
  let duration = (then.getTime() - now.getTime()) / 1000
  for (const { amount, unit } of divisions) {
    if (Math.abs(duration) < amount) return formatter.format(Math.round(duration), unit)
    duration /= amount
  }
  return formatter.format(Math.round(duration), "year")
}

/** The full local timestamp, kept in a tooltip beside the relative phrase. */
export function formatAbsoluteTime(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : formatLocalTimestamp(date)
}
