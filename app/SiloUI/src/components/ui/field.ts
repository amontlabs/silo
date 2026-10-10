import { cva } from "class-variance-authority"

/** The shared look of every text field and select trigger: one radius, fill, font and focus ring. */
export const fieldVariants = cva(
  "w-full min-w-0 rounded-md border border-input bg-transparent px-2.5 text-xs shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 data-[placeholder]:text-muted-foreground dark:bg-input/30 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40",
  {
    variants: { size: { default: "h-8", sm: "h-7" } },
    defaultVariants: { size: "default" },
  },
)

export type FieldSize = "default" | "sm"
