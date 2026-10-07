import type { ComponentProps } from "react"

import { cn } from "@/lib/utils"

/** A pulsing placeholder block. The pulse stops under reduced motion through the surface's own rules. */
export function Skeleton({ className, ...props }: ComponentProps<"span">) {
  return <span data-slot="skeleton" aria-hidden="true" className={cn("block animate-pulse rounded bg-muted motion-reduce:animate-none", className)} {...props} />
}
