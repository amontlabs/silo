/**
 * Dates embedded in English copy ("deleted on October 15, 2026", suggested names, filter chips)
 * are always English so the system locale cannot leak month names into the sentence. Standalone
 * timestamps (activity, logs, last seen) use the system locale instead.
 */
const LOCALE = "en"

type DateInput = Date | number | string

function dateFormat(options: Intl.DateTimeFormatOptions, locale: string | undefined = LOCALE) {
  let formatter: Intl.DateTimeFormat | undefined
  return (value: DateInput): string => {
    const date = value instanceof Date ? value : new Date(value)
    if (Number.isNaN(date.getTime())) return "Invalid Date"
    // The system locale and time zone can change during a session, so only the fixed locale is cached.
    const format = locale === undefined ? new Intl.DateTimeFormat(locale, options) : formatter ??= new Intl.DateTimeFormat(locale, options)
    return format.format(date)
  }
}

/** "October 7, 2026" */
export const formatLongDate = dateFormat({ year: "numeric", month: "long", day: "numeric" })

/** "Oct 7" */
export const formatMonthDay = dateFormat({ month: "short", day: "numeric" })

/** "Oct 7, 13:58" */
export const formatMonthDayTime = dateFormat({ month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" })

/** "Oct 7, 2026, 13:58" */
export const formatDateTime = dateFormat({ year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" })

/** The system locale's full date and time, e.g. "10/7/2026, 1:58:05 PM". */
export const formatLocalTimestamp = dateFormat({ year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" }, undefined)

/** The system locale's short date and time, e.g. "10/7/26, 1:58:05 PM". */
export const formatLocalActivityTime = dateFormat({ dateStyle: "short", timeStyle: "medium" }, undefined)

/** The system locale's time of day, e.g. "1:58:05 PM". */
export const formatLocalTimeOfDay = dateFormat({ hour: "numeric", minute: "numeric", second: "numeric" }, undefined)
