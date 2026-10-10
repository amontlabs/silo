import { LogDisclosure } from "@/components/log-disclosure"
import { splitErrorDetails } from "@/lib/error-details"
import { cn } from "@/lib/utils"

/**
 * An error as a readable summary with the full output behind a Details disclosure that can be
 * copied. Short messages render whole. Place it inside the owning alert or notification, which
 * supplies the role and tone. Pass the backend's separate `diagnostic` when it provides one.
 */
export function ErrorDetails({ message, diagnostic, fallbackSummary, className }: {
  message: string
  /** The backend's separate diagnostic output, when it provides one. */
  diagnostic?: string | null
  /** Shown when the message is only command output, e.g. "Computer changes failed." */
  fallbackSummary?: string
  className?: string
}) {
  const { summary, details } = splitErrorDetails(message, diagnostic)
  const shown = summary ?? fallbackSummary
  return (
    <div className={cn("grid min-w-0 gap-1.5", className)} data-slot="error-details">
      {shown && <p className="min-w-0 whitespace-pre-wrap break-words select-text">{shown}</p>}
      {details && <LogDisclosure
        title="Details"
        output={details}
        outputLabel="Error details"
        labels={{ expand: "Show details", collapse: "Hide details", copy: "Copy details", copied: "Details copied", copyFailed: "Copy details failed" }}
      />}
    </div>
  )
}
