import * as React from "react"

import { fieldVariants, type FieldSize } from "@/components/ui/field"
import { cn } from "@/lib/utils"

/**
 * `technical` marks a field holding an identifier or technical value (names, domains, ports,
 * emails, paths, filters). It turns off macOS/iOS auto-capitalization, autocorrect, spellcheck
 * and browser autofill, which would otherwise rewrite e.g. "e2e-test" to "E2e-test". Prose
 * fields simply omit it. Explicit attributes still win.
 */
function Input({ className, type, technical = false, size, ...props }: Omit<React.ComponentProps<"input">, "size"> & { technical?: boolean; size?: FieldSize }) {
  const technicalProps = technical ? { autoCapitalize: "off", autoCorrect: "off", spellCheck: false, autoComplete: "off" } as const : undefined
  return (
    <input
      type={type}
      {...technicalProps}
      data-slot="input"
      className={cn(
        fieldVariants({ size }),
        "py-1 file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-xs file:font-medium file:text-foreground disabled:pointer-events-none disabled:bg-input/50 dark:disabled:bg-input/80",
        className
      )}
      {...props}
    />
  )
}

export { Input }
