import type { ComponentProps } from "react"

import { ErrorDetails } from "@/components/error-details"
import { cn } from "@/lib/utils"

const tones = {
  danger: "border-destructive/30 bg-destructive/10",
  warning: "border-warning/30 bg-warning/10",
} as const

const sizes = {
  md: "px-3 py-2 text-xs",
  lg: "p-4 text-sm",
} as const

/**
 * A boxed message inside a page or card: an alert by default, or a `status` for notes that need no
 * interruption. Pass `error` to show a long runtime message as a summary with copyable details.
 */
export function InlineAlert({ tone = "danger", size = "md", error, className, children, ...props }: ComponentProps<"div"> & {
  tone?: keyof typeof tones
  size?: keyof typeof sizes
  error?: ComponentProps<typeof ErrorDetails>
}) {
  return (
    <div role="alert" data-slot="inline-alert" data-tone={tone} className={cn("grid min-w-0 gap-2 rounded-md border text-foreground", tones[tone], sizes[size], className)} {...props}>
      {children}
      {error && <ErrorDetails {...error} />}
    </div>
  )
}
