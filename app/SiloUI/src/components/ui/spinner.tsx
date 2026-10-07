import type { ComponentProps } from "react"
import { LoaderCircle } from "lucide-react"

import { useReduceMotion } from "@/components/ui/reduce-motion"
import { cn } from "@/lib/utils"

const sizes = { sm: "size-3", md: "size-3.5" } as const

/**
 * The one busy indicator. It is decorative unless given a `label`, and it stops turning when
 * the system or the app asks for reduced motion.
 */
export function Spinner({ size = "md", label, reduceMotion, className, ...props }: Omit<ComponentProps<typeof LoaderCircle>, "size"> & { size?: keyof typeof sizes; label?: string; reduceMotion?: boolean }) {
  const inheritedReduceMotion = useReduceMotion()
  const still = reduceMotion ?? inheritedReduceMotion
  const labelled = Boolean(label || props["aria-label"] || props["aria-labelledby"])
  return (
    <LoaderCircle
      data-slot="spinner"
      aria-hidden={labelled ? undefined : true}
      aria-label={label}
      role={labelled ? "img" : undefined}
      className={cn("shrink-0", sizes[size], !still && "animate-spin motion-reduce:animate-none", className)}
      {...props}
    />
  )
}
