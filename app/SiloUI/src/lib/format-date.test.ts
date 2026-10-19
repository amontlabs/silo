import { expect, it, vi } from "vitest"

import { formatDateTime, formatLongDate, formatMonthDay, formatMonthDayTime, formatLocalActivityTime, formatLocalTimeOfDay, formatLocalTimestamp } from "@/lib/format-date"

const at = new Date(2026, 9, 7, 13, 58, 5)

it("formats in English with a 24-hour clock", () => {
  expect(formatLongDate(at)).toBe("October 7, 2026")
  expect(formatMonthDay(at)).toBe("Oct 7")
  expect(formatMonthDayTime(at)).toBe("Oct 7, 13:58")
  expect(formatDateTime(at)).toBe("Oct 7, 2026, 13:58")
})

it("accepts timestamps and ISO strings", () => {
  expect(formatLongDate(at.getTime())).toBe("October 7, 2026")
  expect(formatLongDate(at.toISOString())).toBe("October 7, 2026")
})

it("reports an unreadable date instead of throwing", () => {
  expect(formatDateTime("not-a-date")).toBe("Invalid Date")
})

it("always formats with the English locale, whatever the system locale", async () => {
  vi.resetModules()
  const RealFormat = Intl.DateTimeFormat
  const locales: unknown[] = []
  vi.spyOn(Intl, "DateTimeFormat").mockImplementation(function (locale?: Intl.LocalesArgument, options?: Intl.DateTimeFormatOptions) {
    locales.push(locale)
    return new RealFormat(locale ?? "fr", options)
  } as unknown as typeof Intl.DateTimeFormat)
  try {
    const fresh = await import("@/lib/format-date")
    expect(fresh.formatMonthDay(at)).toBe("Oct 7")
    expect(locales).toEqual(["en"])
  } finally { vi.restoreAllMocks() }
})

it("formats local timestamps with the system locale", () => {
  expect(formatLocalTimestamp(at)).toBe(at.toLocaleString())
  expect(formatLocalActivityTime(at)).toBe(at.toLocaleString(undefined, { dateStyle: "short", timeStyle: "medium" }))
  expect(formatLocalTimeOfDay(at)).toBe(at.toLocaleTimeString())
})
